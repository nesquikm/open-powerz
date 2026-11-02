//! Per-session history of every received queue record (FR AH2AEJ).
//!
//! Records are kept columnar in raw device units (time, VBUS, IBUS, CC1, CC2,
//! D+, D−: 20 bytes a sample) in fixed-size chunks, so appending never copies
//! what is already stored and capacity never doubles past what is needed:
//! 10 million samples take about 200 MB, plus a min/max pyramid over the
//! six channels and the per-record power (VBUS × IBUS). The rate (which sets the CC / D±
//! scale) is kept as runs, one entry per rate change. Chunks are shared, so
//! an export snapshot costs a few pointers, not a copy of the samples.

use std::sync::Arc;

use powerz_protocol::decode::{QueueRecord, Rate};
use powerz_record::Integrator;
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
    /// VBUS × IBUS of each record, W.
    Power,
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

/// What the chart ruler shows between its two times (FR AXHK2T), from the raw
/// samples in range, never from the drawn buckets.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct RangeStats {
    /// Records in range.
    pub samples: u64,
    /// Charge by the trapezoid rule between neighbouring records, Ah (signed).
    pub charge_ah: f64,
    /// Energy by the trapezoid rule between neighbouring records, Wh (signed).
    pub energy_wh: f64,
    pub vbus_avg_v: f64,
    pub ibus_avg_a: f64,
    /// Mean of VBUS × IBUS over the records, W.
    pub power_avg_w: f64,
}

/// What segment marks follow (FR AXHK2T, AC-AXHK2T.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MarkKind {
    Charge,
    Energy,
}

/// What E.D. is a share of (FR AXHK2T, AC-AXHK2T.5).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EdScope {
    /// The whole history's energy, summed over its runs (never across a run
    /// break); the window is clamped to the history.
    #[default]
    Session,
    /// The current run's energy; the window is clamped to the run.
    Run,
}

/// What [`Records::run_progress`] is asked for.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct RunProgressQuery {
    /// The running total the marks follow.
    pub kind: MarkKind,
    /// Mark every `step_pct` percent of the total; 0% and 100% are never
    /// marked, and a step outside `0 < step < 100` marks nothing.
    pub step_pct: f64,
    /// How many evenly spaced E.D. points to sample (at most [`MAX_BUCKETS`]);
    /// about the plot width. 0 samples none.
    pub ed_points: usize,
    /// The E.D. window, clamped to the scope's records (either order); all
    /// of them when left out.
    #[serde(default)]
    pub ed_t0_ms: Option<u64>,
    #[serde(default)]
    pub ed_t1_ms: Option<u64>,
    /// What E.D. is a share of; the session when left out. Marks always
    /// follow the current run.
    #[serde(default)]
    pub ed_scope: EdScope,
}

/// One segment mark: where the run's running total first reached `percent`
/// of its final value.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct RunMark {
    /// Session (or file) time, ms, interpolated between records.
    pub t_ms: f64,
    pub percent: f64,
}

/// Segment marks and E.D. of the current run (FR AXHK2T, AC-AXHK2T.4, .5),
/// integrated over the raw samples, never the drawn buckets.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RunProgress {
    /// Time of the history's (or file's) first record, ms.
    pub session_t0_ms: u64,
    /// Time of the run's first and last record, ms.
    pub run_t0_ms: u64,
    pub run_t1_ms: u64,
    /// Records in the run.
    pub samples: u64,
    /// The run's charge and energy by the trapezoid rule (signed).
    pub charge_ah: f64,
    pub energy_wh: f64,
    /// The followed total (charge or energy, per the query) is above zero.
    /// When it is not (an idle or discharging run, or a single record),
    /// `marks` is empty: there is no share of it to mark.
    pub marks_total_positive: bool,
    pub marks: Vec<RunMark>,
    /// The E.D. total (the session's or the run's energy, per the query's
    /// `ed_scope`), Wh.
    pub ed_total_wh: f64,
    /// `ed_total_wh` is above zero. When it is not, `ed_t_ms` and `ed_pct`
    /// are empty.
    pub energy_total_positive: bool,
    /// E.D. sample times, ms, evenly spaced over the window.
    pub ed_t_ms: Vec<f64>,
    /// The running energy at each of `ed_t_ms` as a percentage of
    /// `ed_total_wh`.
    pub ed_pct: Vec<f64>,
}

/// One pass of [`Records::integrate`].
struct Integrated {
    /// The running energy (Wh) at each asked time.
    at_wh: Vec<f64>,
    /// Energy from the first record of the pass to the last, Wh.
    total_wh: f64,
    /// Charge and energy of the current run alone.
    run_ah: f64,
    run_wh: f64,
}

/// Most marks one [`Records::run_progress`] answer holds.
const MAX_MARKS: usize = 1000;

/// One record's time (ms), current (A) and power (W), for integrating.
#[derive(Debug, Clone, Copy)]
struct Point {
    t: f64,
    i: f64,
    p: f64,
}

impl Point {
    fn of(r: &QueueRecord) -> Point {
        let (v, i) = (r.vbus_v(), r.ibus_a());
        Point {
            t: f64::from(r.time_ms()),
            i,
            p: v * i,
        }
    }

    /// The point at time `t` on the straight line through `self` and `b` (in
    /// either order), as the trapezoid rule assumes between two records.
    fn towards(self, b: Point, t: f64) -> Point {
        let w = if b.t != self.t {
            (t - self.t) / (b.t - self.t)
        } else {
            0.0
        };
        Point {
            t,
            i: self.i + (b.i - self.i) * w,
            p: self.p + (b.p - self.p) * w,
        }
    }

    /// Charge (Ah) and energy (Wh) from `self` to `b` by the trapezoid rule.
    fn trapezoid(self, b: Point) -> (f64, f64) {
        let hours = (b.t - self.t).max(0.0) / 3_600_000.0;
        ((self.i + b.i) / 2.0 * hours, (self.p + b.p) / 2.0 * hours)
    }
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
        self.iter_range(0, self.len).map(|(_, r)| r)
    }

    /// Records `i0..i1` with their indices, in order. Walks the chunks and
    /// the rate runs alongside, so each record costs O(1), not a search.
    fn iter_range(&self, i0: usize, i1: usize) -> impl Iterator<Item = (usize, QueueRecord)> + '_ {
        let i1 = i1.min(self.len);
        let mut run = self
            .rates
            .partition_point(|&(start, _)| start <= i0)
            .saturating_sub(1);
        (i0..i1).filter_map(move |i| {
            while self
                .rates
                .get(run + 1)
                .is_some_and(|&(start, _)| start <= i)
            {
                run += 1;
            }
            let &(_, rate) = self.rates.get(run)?;
            let r = self
                .chunks
                .get(i >> CHUNK_SHIFT)?
                .get(i & CHUNK_MASK, rate)?;
            Some((i, r))
        })
    }

    /// Record `i` starts a new measuring run.
    fn starts_run(&self, i: usize) -> bool {
        self.breaks.binary_search(&i).is_ok()
    }

    /// Record indices `i0..i1` with `t0 <= time <= t1` (times never decrease).
    fn index_range(&self, t0: u64, t1: u64) -> (usize, usize) {
        let first = |pred: &dyn Fn(u64) -> bool| {
            let (mut lo, mut hi) = (0, self.len);
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

    /// The ruler's stats over the records with `t0 <= time <= t1` (either
    /// order); `None` when no record is in range.
    ///
    /// Charge and energy are the trapezoid rule over `t0..t1` itself: between
    /// neighbouring records in range as [`Integrator`] counts them, plus the
    /// parts from each end to its nearest record, along the line to the
    /// record just outside (so ΔAh is ΔT × the average for a steady load).
    /// Nothing counts across a run break, edge parts included. Averages are
    /// over the records in range.
    pub fn range_stats(&self, t0: u64, t1: u64) -> Option<RangeStats> {
        let (t0, t1) = if t1 < t0 { (t1, t0) } else { (t0, t1) };
        let (i0, i1) = self.index_range(t0, t1);
        if i0 >= i1 {
            return None;
        }
        let mut next_break = self.breaks.partition_point(|&b| b <= i0);
        let mut integrator = Integrator::new();
        let (mut charge_ah, mut energy_wh) = (0.0, 0.0);
        let (mut samples, mut v_sum, mut i_sum, mut p_sum) = (0u64, 0.0, 0.0, 0.0);
        for (i, r) in self.iter_range(i0, i1) {
            if self.breaks.get(next_break) == Some(&i) {
                integrator.break_run();
                next_break += 1;
            }
            let (v, a) = (r.vbus_v(), r.ibus_a());
            (charge_ah, energy_wh) = integrator.push(r.time_ms(), v, a);
            samples += 1;
            v_sum += v;
            i_sum += a;
            p_sum += v * a;
        }
        // The part from `t0` to the first record in range, and from the last
        // one to `t1`, each within one run.
        let edges = [
            (
                i0.checked_sub(1).filter(|_| !self.starts_run(i0)),
                i0,
                t0 as f64,
            ),
            (
                Some(i1).filter(|&o| o < self.len && !self.starts_run(o)),
                i1 - 1,
                t1 as f64,
            ),
        ];
        for (outside, inside, at) in edges {
            let (Some(o), Some(inner)) = (outside.and_then(|o| self.get(o)), self.get(inside))
            else {
                continue;
            };
            let (o, inner) = (Point::of(&o), Point::of(&inner));
            let end = o.towards(inner, at);
            let (dq, de) = if end.t < inner.t {
                end.trapezoid(inner)
            } else {
                inner.trapezoid(end)
            };
            charge_ah += dq;
            energy_wh += de;
        }
        let n = samples as f64;
        (samples > 0).then(|| RangeStats {
            samples,
            charge_ah,
            energy_wh,
            vbus_avg_v: v_sum / n,
            ibus_avg_a: i_sum / n,
            power_avg_w: p_sum / n,
        })
    }

    /// Segment marks and E.D.: the marks follow the current run (the records
    /// since the last run break; a break with no record after it yet is not
    /// counted), the whole history when there is none. E.D. is a share of the
    /// session's energy, or of the run's when the query asks. `None` with no
    /// record.
    ///
    /// Charge and energy are integrated over the raw records by the trapezoid
    /// rule, never across a run break. A mark sits where the running total
    /// first reaches its step, linearly between the two records around it.
    /// Each E.D. point is the running energy at its time (the trapezoid rule
    /// up to it) over the E.D. total. A total that is not above zero gives
    /// no marks or E.D., and says so in the answer's flags.
    pub fn run_progress(&self, q: &RunProgressQuery) -> Option<RunProgress> {
        let start = self
            .breaks
            .iter()
            .rev()
            .find(|&&b| b < self.len)
            .copied()
            .unwrap_or(0);
        let last = self.len.checked_sub(1)?;
        let (run_t0_ms, run_t1_ms) = (self.time_at(start), self.time_at(last));
        let from = match q.ed_scope {
            EdScope::Session => 0,
            EdScope::Run => start,
        };
        let ed_t_ms = ed_times(q, self.time_at(from), run_t1_ms);

        // Pass 1: the totals, and the running energy at each E.D. time.
        let Integrated {
            at_wh,
            total_wh,
            run_ah: charge_ah,
            run_wh: energy_wh,
        } = self.integrate(from, start, &ed_t_ms);

        let followed = match q.kind {
            MarkKind::Charge => charge_ah,
            MarkKind::Energy => energy_wh,
        };
        let marks_total_positive = followed > 0.0 && followed.is_finite();
        let marks = if marks_total_positive {
            self.marks(start, q.kind, followed, &mark_percents(q.step_pct))
        } else {
            Vec::new()
        };
        let energy_total_positive = total_wh > 0.0 && total_wh.is_finite();
        let (ed_t_ms, ed_pct) = if energy_total_positive {
            let pct = at_wh.iter().map(|wh| 100.0 * wh / total_wh).collect();
            (ed_t_ms, pct)
        } else {
            (Vec::new(), Vec::new())
        };
        Some(RunProgress {
            session_t0_ms: self.time_at(0),
            run_t0_ms,
            run_t1_ms,
            samples: (self.len - start) as u64,
            charge_ah,
            energy_wh,
            marks_total_positive,
            marks,
            ed_total_wh: total_wh,
            energy_total_positive,
            ed_t_ms,
            ed_pct,
        })
    }

    /// One pass over records `from..`: the running energy at each of `times`
    /// (ascending), nothing counted across a run break, the energy over the
    /// whole pass, and the charge and energy of the run that starts at record
    /// `run_start` (`>= from`, with no break after it).
    fn integrate(&self, from: usize, run_start: usize, times: &[f64]) -> Integrated {
        let mut at_wh = Vec::with_capacity(times.len());
        let mut next_break = self.breaks.partition_point(|&b| b <= from);
        let mut integrator = Integrator::new();
        let (mut ah, mut wh) = (0.0, 0.0);
        let mut before_run = (0.0, 0.0);
        let mut prev: Option<Point> = None;
        for (i, r) in self.iter_range(from, self.len) {
            if self.breaks.get(next_break) == Some(&i) {
                integrator.break_run();
                prev = None;
                next_break += 1;
            }
            if i == run_start {
                before_run = (ah, wh);
            }
            let b = Point::of(&r);
            while let Some(&t) = times.get(at_wh.len()).filter(|&&t| t <= b.t) {
                let part = prev.map_or(0.0, |a| a.trapezoid(a.towards(b, t)).1);
                at_wh.push(wh + part);
            }
            (ah, wh) = integrator.push(r.time_ms(), r.vbus_v(), r.ibus_a());
            prev = Some(b);
        }
        at_wh.resize(times.len(), wh);
        Integrated {
            at_wh,
            total_wh: wh,
            run_ah: ah - before_run.0,
            run_wh: wh - before_run.1,
        }
    }

    /// Pass 2 of [`Self::run_progress`]: where the running `kind` total of
    /// records `start..` first reaches each of `percents` of `total` (> 0).
    fn marks(&self, start: usize, kind: MarkKind, total: f64, percents: &[f64]) -> Vec<RunMark> {
        let mut marks = Vec::with_capacity(percents.len());
        let mut integrator = Integrator::new();
        let mut prev: Option<(f64, f64)> = None;
        for (_, r) in self.iter_range(start, self.len) {
            let Some(&percent) = percents.get(marks.len()) else {
                break;
            };
            let (ah, wh) = integrator.push(r.time_ms(), r.vbus_v(), r.ibus_a());
            let (t, c) = (
                f64::from(r.time_ms()),
                if kind == MarkKind::Charge { ah } else { wh },
            );
            let mut percent = Some(percent);
            while let Some(p) = percent {
                let target = total * p / 100.0;
                if c < target {
                    break;
                }
                let t_ms = match prev {
                    Some((t0, c0)) if c0 < target => t0 + (target - c0) / (c - c0) * (t - t0),
                    _ => t,
                };
                marks.push(RunMark { t_ms, percent: p });
                percent = percents.get(marks.len()).copied();
            }
            prev = Some((t, c));
        }
        marks
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
const SERIES: usize = 7;
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

    /// The ruler's stats over the records with `t0 <= time <= t1`; see
    /// [`Records::range_stats`]. Reads the history only.
    pub fn range_stats(&self, t0: u64, t1: u64) -> Option<RangeStats> {
        self.records.range_stats(t0, t1)
    }

    /// Segment marks and E.D. of the current run; see
    /// [`Records::run_progress`]. Reads the history only.
    pub fn run_progress(&self, q: &RunProgressQuery) -> Option<RunProgress> {
        self.records.run_progress(q)
    }

    /// `t0..=t1` widened to the record just outside each end that falls
    /// between records, so a chart drawing the range runs its lines to the
    /// edges at any zoom (AC-9CFMD3.4); an end on a record, or past the
    /// first or newest record, stays as it is.
    pub fn widen_to_neighbours(&self, t0: u64, t1: u64) -> (u64, u64) {
        if t1 < t0 {
            return (t0, t1);
        }
        let len = self.records.len;
        let (i0, i1) = self.index_range(t0, t1);
        let lo = if i0 > 0 && (i0 == len || self.time_at(i0) > t0) {
            self.time_at(i0 - 1)
        } else {
            t0
        };
        let hi = if i1 < len && (i1 == 0 || self.time_at(i1 - 1) < t1) {
            self.time_at(i1)
        } else {
            t1
        };
        (lo, hi)
    }

    /// Time of record `i`, ms.
    fn time_at(&self, i: usize) -> u64 {
        self.records.time_at(i)
    }

    /// Record indices `i0..i1` with `t0 <= time <= t1` (times never decrease).
    fn index_range(&self, t0: u64, t1: u64) -> (usize, usize) {
        self.records.index_range(t0, t1)
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

/// The percentages to mark at every `step_pct`, 0% and 100% left out; none
/// for a step outside `0 < step < 100`, at most [`MAX_MARKS`].
fn mark_percents(step_pct: f64) -> Vec<f64> {
    if !(step_pct > 0.0 && step_pct < 100.0) {
        return Vec::new();
    }
    (1..=MAX_MARKS)
        .map(|k| k as f64 * step_pct)
        // A tolerance keeps a step like 100 / 3 from marking 100%.
        .take_while(|&p| p < 100.0 - 1e-9)
        .collect()
}

/// `q.ed_points` (at most [`MAX_BUCKETS`]) evenly spaced times over the E.D.
/// window clamped to `run_t0..=run_t1` (the scope's records); none when it
/// misses them.
fn ed_times(q: &RunProgressQuery, run_t0: u64, run_t1: u64) -> Vec<f64> {
    let n = q.ed_points.min(MAX_BUCKETS);
    let (a, b) = (q.ed_t0_ms.unwrap_or(run_t0), q.ed_t1_ms.unwrap_or(run_t1));
    let (a, b) = (a.min(b).max(run_t0), a.max(b).min(run_t1));
    if n == 0 || a > b {
        return Vec::new();
    }
    let (a, b) = (a as f64, b as f64);
    if n == 1 {
        return vec![b];
    }
    let step = (b - a) / (n - 1) as f64;
    (0..n)
        .map(|k| if k + 1 == n { b } else { a + step * k as f64 })
        .collect()
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
        Series::Power => 6,
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
        (r.vbus_v() * r.ibus_a()) as f32,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sequential_walk_matches_record_by_record_lookup() {
        // Across chunk boundaries and rate changes, from any start.
        let mut r = Records::default();
        let n = CHUNK_LEN * 2 + 77;
        for i in 0..n {
            let rate = match i / 40_000 {
                0 => Rate::Sps1000,
                1 => Rate::Sps10,
                _ => Rate::Sps1000,
            };
            let t = u32::try_from(i).unwrap_or(u32::MAX);
            r.push(&QueueRecord::from_raw(
                t,
                5_000_000,
                1_000_000,
                [100, 200, 300, 400],
                rate,
            ));
        }
        for i0 in [0, 1, 39_999, 40_000, CHUNK_LEN - 1, CHUNK_LEN, n - 1] {
            let walked: Vec<_> = r.iter_range(i0, n).collect();
            let looked_up: Vec<_> = (i0..n).filter_map(|i| r.get(i).map(|x| (i, x))).collect();
            assert_eq!(walked, looked_up, "from {i0}");
        }
        assert_eq!(r.iter_range(5, n + 10).count(), n - 5);
    }

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
