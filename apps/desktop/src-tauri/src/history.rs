//! Per-session history of every received queue record (FR AH2AEJ).
//!
//! Records are kept columnar in raw device units (time, VBUS, IBUS, CC1, CC2,
//! D+, D−: 20 bytes a sample) in fixed-size chunks, so appending never copies
//! what is already stored and capacity never doubles past what is needed:
//! 10 million samples take about 200 MB. The rate (which sets the CC / D±
//! scale) is kept as runs, one entry per rate change. Chunks are shared, so
//! an export snapshot costs a few pointers, not a copy of the samples.

use std::sync::Arc;

use powerz_protocol::decode::{QueueRecord, Rate};
use serde::{Deserialize, Serialize};

/// Samples per chunk (a power of two).
const CHUNK_SHIFT: u32 = 16;
const CHUNK_LEN: usize = 1 << CHUNK_SHIFT;
const CHUNK_MASK: usize = CHUNK_LEN - 1;

/// Most buckets one [`History::range`] query returns per series.
pub const MAX_BUCKETS: usize = 8192;

/// One fixed-size block of samples, one column per field.
#[derive(Debug)]
struct Chunk {
    time_ms: Vec<u32>,
    vbus_uv: Vec<i32>,
    ibus_ua: Vec<i32>,
    cc1: Vec<u16>,
    cc2: Vec<u16>,
    dp: Vec<u16>,
    dm: Vec<u16>,
}

impl Chunk {
    /// An empty chunk. Its columns grow by doubling up to exactly
    /// [`CHUNK_LEN`] (a power of two), never past it, so a short session
    /// stays small and the copies stay bounded per chunk.
    fn new() -> Chunk {
        Chunk {
            time_ms: Vec::new(),
            vbus_uv: Vec::new(),
            ibus_ua: Vec::new(),
            cc1: Vec::new(),
            cc2: Vec::new(),
            dp: Vec::new(),
            dm: Vec::new(),
        }
    }

    fn push(&mut self, r: &QueueRecord) {
        let [cc1, cc2, dp, dm] = r.raw_lines();
        self.time_ms.push(r.time_ms());
        self.vbus_uv.push(r.vbus_uv());
        self.ibus_ua.push(r.ibus_ua());
        self.cc1.push(cc1);
        self.cc2.push(cc2);
        self.dp.push(dp);
        self.dm.push(dm);
    }

    fn get(&self, i: usize, rate: Rate) -> Option<QueueRecord> {
        Some(QueueRecord::from_raw(
            *self.time_ms.get(i)?,
            *self.vbus_uv.get(i)?,
            *self.ibus_ua.get(i)?,
            [
                *self.cc1.get(i)?,
                *self.cc2.get(i)?,
                *self.dp.get(i)?,
                *self.dm.get(i)?,
            ],
            rate,
        ))
    }

    /// Allocated bytes of the columns.
    fn memory_bytes(&self) -> usize {
        use std::mem::size_of;
        self.time_ms.capacity() * size_of::<u32>()
            + self.vbus_uv.capacity() * size_of::<i32>()
            + self.ibus_ua.capacity() * size_of::<i32>()
            + (self.cc1.capacity() + self.cc2.capacity() + self.dp.capacity() + self.dm.capacity())
                * size_of::<u16>()
    }
}

/// A copy keeps the column capacities, so a copied open chunk still never
/// grows past [`CHUNK_LEN`].
impl Clone for Chunk {
    fn clone(&self) -> Chunk {
        fn copy<T: Copy>(v: &Vec<T>) -> Vec<T> {
            let mut c = Vec::with_capacity(v.capacity());
            c.extend_from_slice(v);
            c
        }
        Chunk {
            time_ms: copy(&self.time_ms),
            vbus_uv: copy(&self.vbus_uv),
            ibus_ua: copy(&self.ibus_ua),
            cc1: copy(&self.cc1),
            cc2: copy(&self.cc2),
            dp: copy(&self.dp),
            dm: copy(&self.dm),
        }
    }
}

/// A chart series in the history.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Series {
    Vbus,
    Ibus,
    Dp,
    Dm,
    Cc1,
    Cc2,
}

/// The min and max of one series over one time bucket, in SI units.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Bucket {
    /// Device time of the bucket, ms.
    pub t_ms: u64,
    pub min: f32,
    pub max: f32,
}

/// A stretch of session time with no records: the time between the last
/// record of one measuring run and the first of the next (an unplug, a
/// disconnect, a clock jump or a queue restart).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct HistoryGap {
    /// Time of the last record before the gap, ms.
    pub from_ms: u64,
    /// Time of the first record after it, ms.
    pub to_ms: u64,
}

/// The buckets of one series.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeriesBuckets {
    pub series: Series,
    pub buckets: Vec<Bucket>,
}

/// The records of a history without its pyramid. A clone shares the
/// stored chunks; the history copies its open chunk before the next append.
#[derive(Debug, Clone, Default)]
pub struct Records {
    chunks: Vec<Arc<Chunk>>,
    /// `(first index, rate)` for each run of records at one rate.
    rates: Vec<(usize, Rate)>,
    /// Indices of the records that start a new measuring run (after an
    /// outage or a clock jump), in order.
    breaks: Vec<usize>,
    len: usize,
}

impl Records {
    /// Number of records.
    pub fn len(&self) -> usize {
        self.len
    }

    /// No records.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Record `i`, exactly as it was pushed.
    pub fn get(&self, i: usize) -> Option<QueueRecord> {
        if i >= self.len {
            return None;
        }
        let run = self.rates.partition_point(|&(start, _)| start <= i);
        let (_, rate) = *self.rates.get(run.checked_sub(1)?)?;
        self.chunks.get(i >> CHUNK_SHIFT)?.get(i & CHUNK_MASK, rate)
    }

    /// Every record, in order.
    pub fn iter(&self) -> impl Iterator<Item = QueueRecord> + '_ {
        (0..self.len).filter_map(|i| self.get(i))
    }

    /// Every record, in order, with whether it starts a new measuring run
    /// (nothing was measured between it and the record before).
    pub fn iter_runs(&self) -> impl Iterator<Item = (QueueRecord, bool)> + '_ {
        let mut breaks = self.breaks.iter().peekable();
        self.iter().enumerate().map(move |(i, r)| {
            let new_run = breaks.next_if(|&&b| b == i).is_some();
            (r, new_run)
        })
    }

    fn push(&mut self, r: &QueueRecord) {
        if self.len & CHUNK_MASK == 0 && self.len >> CHUNK_SHIFT == self.chunks.len() {
            self.chunks.push(Arc::new(Chunk::new()));
        }
        if let Some(chunk) = self.chunks.last_mut() {
            Arc::make_mut(chunk).push(r);
        }
        if self.rates.last().is_none_or(|&(_, rate)| rate != r.rate()) {
            self.rates.push((self.len, r.rate()));
        }
        self.len += 1;
    }

    /// Time of record `i`, ms.
    fn time_at(&self, i: usize) -> u64 {
        self.chunks
            .get(i >> CHUNK_SHIFT)
            .and_then(|c| c.time_ms.get(i & CHUNK_MASK))
            .map_or(u64::MAX, |&t| u64::from(t))
    }

    /// Bytes allocated for the records.
    fn memory_bytes(&self) -> usize {
        use std::mem::size_of;
        // Each chunk sits in its own allocation behind two reference counts.
        let per_chunk = size_of::<Chunk>() + 2 * size_of::<usize>();
        self.chunks
            .iter()
            .map(|c| c.memory_bytes() + per_chunk)
            .sum::<usize>()
            + self.chunks.capacity() * size_of::<Arc<Chunk>>()
            + self.rates.capacity() * size_of::<(usize, Rate)>()
            + self.breaks.capacity() * size_of::<usize>()
    }
}

/// Every record of a session, columnar, in raw device units.
#[derive(Debug, Clone, Default)]
pub struct History {
    records: Records,
    /// Min / max pyramid: level `k` holds one entry per block of
    /// [`block_len`]`(k)` records. Level 0 covers every record; a higher
    /// level's entry holds only its child blocks that are complete.
    pyramid: Vec<Vec<Entry>>,
}

/// Records per block at the finest pyramid level.
const PYRAMID_BASE: usize = 64;
/// Number of chart series.
const SERIES: usize = 6;
/// `(min, max)` of every series over one block.
type Entry = [(f32, f32); SERIES];

impl History {
    /// An empty history.
    pub fn new() -> History {
        History::default()
    }

    /// Append one record; amortized constant time.
    pub fn push(&mut self, r: &QueueRecord) {
        self.records.push(r);
        self.extend_pyramid(r);
    }

    /// The next record pushed starts a new measuring run: nothing was
    /// measured between it and the record before (an outage or a clock jump).
    pub fn break_run(&mut self) {
        let next = self.records.len;
        if self.records.breaks.last() != Some(&next) {
            self.records.breaks.push(next);
        }
    }

    /// The records so far, sharing the stored chunks (for an export).
    pub fn snapshot(&self) -> Records {
        self.records.clone()
    }

    /// Fold the newest record (number `self.len() - 1`) into the pyramid:
    /// into its level-0 block, and each block it completes into the level
    /// above.
    fn extend_pyramid(&mut self, r: &QueueRecord) {
        let v = values(r);
        let mut entry: Entry = std::array::from_fn(|s| (v[s], v[s]));
        let n = self.records.len;
        let mut k = 0;
        loop {
            let size = block_len(k);
            let b = (n - 1) / size;
            if self.pyramid.len() == k {
                self.pyramid.push(Vec::new());
            }
            let Some(level) = self.pyramid.get_mut(k) else {
                return;
            };
            match level.get_mut(b) {
                Some(e) => {
                    for (s, x) in e.iter_mut().enumerate() {
                        *x = merge_pair(*x, entry[s]);
                    }
                }
                None => level.push(entry),
            }
            // A completed block goes up one level.
            if !n.is_multiple_of(size) {
                return;
            }
            let Some(&done) = level.get(b) else {
                return;
            };
            entry = done;
            k += 1;
        }
    }

    /// Number of records.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// No records yet.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Record `i`, exactly as it was pushed.
    pub fn get(&self, i: usize) -> Option<QueueRecord> {
        self.records.get(i)
    }

    /// Every record, in order.
    pub fn iter(&self) -> impl Iterator<Item = QueueRecord> + '_ {
        self.records.iter()
    }

    /// Drop every record and free the memory.
    pub fn clear(&mut self) {
        *self = History::new();
    }

    /// Bytes allocated by the store (capacity, not just length).
    pub fn memory_bytes(&self) -> usize {
        use std::mem::size_of;
        self.records.memory_bytes()
            + self
                .pyramid
                .iter()
                .map(|l| l.capacity() * size_of::<Entry>())
                .sum::<usize>()
    }

    /// Min / max buckets of `series` over `t0..=t1`, at most `buckets` each
    /// (and never more than [`MAX_BUCKETS`]). Each requested series gets
    /// one answer, in request order; a repeated series is computed once.
    ///
    /// Picks the coarsest pyramid level that still gives at least `buckets`
    /// blocks in range, merges whole blocks into buckets and reads raw
    /// samples only at the two edges of the range.
    pub fn range(&self, series: &[Series], t0: u64, t1: u64, buckets: usize) -> Vec<SeriesBuckets> {
        let buckets = buckets.min(MAX_BUCKETS);
        let (i0, i1) = self.index_range(t0, t1);
        let plan = (buckets > 0 && i0 < i1).then(|| self.plan(i0, i1, buckets));
        // Where each series was first answered.
        let mut first: [Option<usize>; SERIES] = [None; SERIES];
        let mut out: Vec<SeriesBuckets> = Vec::with_capacity(series.len());
        for &s in series {
            let slot = &mut first[series_index(s)];
            let buckets = match slot.and_then(|k| out.get(k)) {
                Some(done) => done.buckets.clone(),
                None => {
                    *slot = Some(out.len());
                    plan.as_ref()
                        .map(|plan| self.buckets_of(s, plan))
                        .unwrap_or_default()
                }
            };
            out.push(SeriesBuckets { series: s, buckets });
        }
        out
    }

    /// The gaps between measuring runs that overlap the open range `t0..t1`,
    /// oldest first; a gap that only touches an end is left out.
    pub fn gaps(&self, t0: u64, t1: u64) -> Vec<HistoryGap> {
        self.records
            .breaks
            .iter()
            .filter(|&&b| b > 0 && b < self.records.len)
            .map(|&b| HistoryGap {
                from_ms: self.time_at(b - 1),
                to_ms: self.time_at(b),
            })
            .filter(|g| g.to_ms > t0 && g.from_ms < t1)
            .collect()
    }

    /// Time of record `i`, ms.
    fn time_at(&self, i: usize) -> u64 {
        self.records.time_at(i)
    }

    /// Record indices `i0..i1` with `t0 <= time <= t1` (times never decrease).
    fn index_range(&self, t0: u64, t1: u64) -> (usize, usize) {
        let first = |pred: &dyn Fn(u64) -> bool| {
            let (mut lo, mut hi) = (0, self.records.len);
            while lo < hi {
                let mid = lo + (hi - lo) / 2;
                if pred(self.time_at(mid)) {
                    lo = mid + 1;
                } else {
                    hi = mid;
                }
            }
            lo
        };
        let i0 = first(&|t| t < t0);
        let i1 = first(&|t| t <= t1);
        (i0, i1.max(i0))
    }

    /// How to cut `i0..i1` into at most `buckets` buckets.
    fn plan(&self, i0: usize, i1: usize, buckets: usize) -> Plan {
        // Unit sizes: raw samples, then each pyramid level's block size.
        let mut unit = 1;
        for level in 0..self.pyramid.len() {
            let size = block_len(level);
            if i1.div_ceil(size) - i0 / size >= buckets {
                unit = size;
            } else {
                break;
            }
        }
        let units = i1.div_ceil(unit) - i0 / unit;
        let per = units.div_ceil(buckets).max(1);
        Plan {
            i0,
            i1,
            base: i0 / unit * unit,
            stride: unit * per,
        }
    }

    fn buckets_of(&self, series: Series, plan: &Plan) -> Vec<Bucket> {
        let s = series_index(series);
        let mut out = Vec::new();
        let mut lo = plan.i0;
        while lo < plan.i1 {
            // Interior boundaries sit a whole number of strides past the
            // first unit, so on block boundaries of the chosen level.
            let hi = (plan.base + ((lo - plan.base) / plan.stride + 1) * plan.stride).min(plan.i1);
            // A bucket always holds records; one that did not would have no
            // min or max to send.
            if let Some((min, max)) = self.span(s, self.pyramid.len(), lo, hi) {
                out.push(Bucket {
                    t_ms: self.time_at(lo),
                    min,
                    max,
                });
            }
            lo = hi;
        }
        out
    }

    /// Min and max of series `s` over records `lo..hi`, using blocks of
    /// pyramid levels below `level` (none: raw samples); `None` when the
    /// span holds no records.
    fn span(&self, s: usize, level: usize, lo: usize, hi: usize) -> Option<(f32, f32)> {
        let len = self.records.len;
        let mut acc = None;
        let Some(k) = level.checked_sub(1) else {
            for i in lo..hi {
                if let Some(v) = self.value_at(s, i) {
                    acc = merge(acc, Some((v, v)));
                }
            }
            return acc;
        };
        let size = block_len(k);
        // Records the level-`k` entries hold: all of them at level 0, only
        // whole child blocks above it.
        let covered = if k == 0 {
            len
        } else {
            let child = block_len(k - 1);
            len / child * child
        };
        let mut b = lo / size;
        while b * size < hi {
            let start = b * size;
            let end = ((b + 1) * size).min(len);
            let whole = start >= lo && end <= hi && end <= covered;
            match self.pyramid.get(k).and_then(|l| l.get(b)) {
                Some(entry) if whole => acc = merge(acc, Some(entry[s])),
                _ => acc = merge(acc, self.span(s, k, start.max(lo), end.min(hi))),
            }
            b += 1;
        }
        acc
    }

    /// The value of series `s` for record `i`, as the f64 accessor cast to f32.
    fn value_at(&self, s: usize, i: usize) -> Option<f32> {
        self.get(i).map(|r| values(&r)[s])
    }
}

/// Records per block at pyramid level `k`: 64·4^k.
fn block_len(k: usize) -> usize {
    PYRAMID_BASE << (2 * k)
}

/// The records one query covers and the records per bucket.
struct Plan {
    i0: usize,
    i1: usize,
    /// Start of the first unit (block or record) the range touches.
    base: usize,
    stride: usize,
}

fn merge_pair(a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    (a.0.min(b.0), a.1.max(b.1))
}

/// The min / max over both spans, either of which may be empty.
fn merge(a: Option<(f32, f32)>, b: Option<(f32, f32)>) -> Option<(f32, f32)> {
    match (a, b) {
        (Some(a), Some(b)) => Some(merge_pair(a, b)),
        (a, b) => a.or(b),
    }
}

/// Column of `series` in a pyramid entry.
fn series_index(series: Series) -> usize {
    match series {
        Series::Vbus => 0,
        Series::Ibus => 1,
        Series::Dp => 2,
        Series::Dm => 3,
        Series::Cc1 => 4,
        Series::Cc2 => 5,
    }
}

/// The values of every series for `r`, in SI units, in [`series_index`] order.
fn values(r: &QueueRecord) -> [f32; SERIES] {
    [
        r.vbus_v() as f32,
        r.ibus_a() as f32,
        r.dp_v() as f32,
        r.dm_v() as f32,
        r.cc1_v() as f32,
        r.cc2_v() as f32,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_span_has_no_min_or_max() {
        let mut h = History::new();
        for t in 0..1_000u32 {
            h.push(&QueueRecord::from_raw(
                t,
                5_000_000,
                1_000_000,
                [0; 4],
                Rate::Sps1000,
            ));
        }
        for level in 0..=h.pyramid.len() {
            assert_eq!(h.span(0, level, 500, 500), None, "level {level}");
        }
        assert_eq!(h.span(0, 0, 10, 20), Some((5.0, 5.0)));
    }
}
