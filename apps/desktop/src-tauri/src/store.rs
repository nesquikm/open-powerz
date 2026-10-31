//! Session data shared by every connection to one meter (FR AH2AEJ,
//! VHY378): the history, its wall-clock start, the statistics and the
//! session timeline. A reconnect of the same serial gets the same store, so
//! the history and the statistics go on across the outage.
//!
//! Queries run on the caller's thread under the store lock; the worker
//! takes the lock only briefly per poll to append.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use powerz_protocol::decode::QueueRecord;
use powerz_protocol::session::SnapshotCadence;

use crate::history::{History, HistoryGap, Records, Series, SeriesBuckets};
use crate::stats::{Stats, StatsSnapshot};

/// Maps device time onto a session time that never goes backwards.
///
/// The meter restarts its clock after a reboot or a replug (and may on a
/// queue restart). Session time is device time plus an offset; when a
/// record would land before the last one, the offset changes so the
/// session goes on at the last time plus the wall-clock time since it
/// (at least 1 ms).
#[derive(Debug, Default)]
struct Timeline {
    offset: i64,
    /// Session time and wall-clock time of the last record, ms.
    last: Option<(u32, u64)>,
    /// The next record starts a new run (a new connection or a queue
    /// restart).
    restart: bool,
}

impl Timeline {
    /// Session time of a record at `device_ms` received at wall-clock
    /// `now_ms`, and whether it starts a new run (the first record of a
    /// connection, or the first after the clock jumped back).
    fn map(&mut self, device_ms: u32, now_ms: u64) -> (u32, bool) {
        let device = i64::from(device_ms);
        let mut t = clamp_ms(device + self.offset);
        let mut jumped = false;
        let restart = std::mem::take(&mut self.restart);
        // A new run never starts at or before the last record, so the break before it
        // always has a length (a clock that has not moved since the last run included).
        if let Some((last, wall)) = self.last
            && (t < last || (restart && t <= last))
        {
            let step = now_ms.saturating_sub(wall).max(1);
            let target = u64::from(last).saturating_add(step);
            t = u32::try_from(target).unwrap_or(u32::MAX);
            self.offset = i64::from(t) - device;
            jumped = true;
        }
        let new_run = restart || jumped;
        self.last = Some((t, now_ms));
        (t, new_run)
    }
}

/// `ms` clamped to the `u32` range of record times.
fn clamp_ms(ms: i64) -> u32 {
    u32::try_from(ms.max(0)).unwrap_or(u32::MAX)
}

/// One record as [`SessionData::append`] stored it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Appended {
    /// The record in session time.
    pub(crate) record: QueueRecord,
    /// Session time of the record before it.
    pub(crate) before_ms: Option<u32>,
    /// It starts a new measuring run.
    pub(crate) new_run: bool,
    /// Wall-clock time of the record, ms: the session's one `Unix` anchor
    /// plus its session time, so a recording and an export agree.
    pub(crate) unix_ms: u64,
    /// The latest valid snapshot temperature at the record, °C.
    pub(crate) temp_c: Option<f64>,
}

/// Temperature changes over the history: `(index of the first record it
/// applies to, °C, last session time it applies to)`. The temperature
/// changes about twice a second, far less often than records arrive, so it
/// is not stored per record.
/// The indexes count from the history's start, so it is valid only while
/// the history is never trimmed from the front: a clear empties both.
pub(crate) type TempLog = Vec<(usize, f64, i64)>;

/// The temperature at history index `i` (session time `t_ms`) in `log`
/// (sorted by index), moving `cursor` forward; indexes must be asked for in
/// increasing order. Empty once `t_ms` is past the snapshot's age limit.
pub(crate) fn temp_at(
    log: &[(usize, f64, i64)],
    cursor: &mut usize,
    i: usize,
    t_ms: u32,
) -> Option<f64> {
    while log.get(*cursor + 1).is_some_and(|&(at, ..)| at <= i) {
        *cursor += 1;
    }
    log.get(*cursor)
        .filter(|&&(at, _, until)| at <= i && i64::from(t_ms) <= until)
        .map(|&(_, t, _)| t)
}

/// What one session keeps across its connections.
#[derive(Debug, Default)]
pub(crate) struct SessionData {
    /// Every record of the session, in session time.
    history: History,
    /// Wall-clock time at the history's first record, ms.
    history_unix_ms: Option<u64>,
    /// Statistics since the session start or the last reset.
    stats: Stats,
    timeline: Timeline,
    /// Wall-clock time at session time 0, ms, fixed by the session's first
    /// record; clearing the history keeps it.
    unix_anchor_ms: Option<i64>,
    /// The latest valid snapshot temperature, °C, and the wall-clock time
    /// it was read at, ms.
    temp_c: Option<(f64, u64)>,
    /// Temperature changes over the history.
    temps: TempLog,
}

impl SessionData {
    /// Move `r` onto the session timeline and append it to the history and
    /// the statistics.
    pub(crate) fn append(&mut self, r: &QueueRecord, now_ms: u64) -> Appended {
        let before_ms = self.timeline.last.map(|(t, _)| t);
        let (t, new_run) = self.timeline.map(r.time_ms(), now_ms);
        let record = r.with_time_ms(t);
        if new_run {
            // Nothing was measured across an outage, a clock jump or a
            // queue restart.
            self.stats.break_run();
            self.history.break_run();
        }
        self.stats.push(&record);
        let anchor = *self
            .unix_anchor_ms
            .get_or_insert(i64::try_from(now_ms).unwrap_or(i64::MAX) - i64::from(t));
        let unix_ms = u64::try_from(anchor.saturating_add(i64::from(t)).max(0)).unwrap_or(0);
        self.history_unix_ms.get_or_insert(unix_ms);
        // The snapshot in session time, plus its age limit (AC-CWWKJ6.4).
        let temp = self.temp_c.map(|(temp, read_ms)| {
            let read_t = i64::try_from(read_ms)
                .unwrap_or(i64::MAX)
                .saturating_sub(anchor);
            (temp, SnapshotCadence::expiry_ms(read_t))
        });
        if let Some((temp, until)) = temp {
            // `until` is the read time plus the age limit.
            let read_t = until.saturating_sub(SnapshotCadence::expiry_ms(0));
            match self.temps.last_mut() {
                // The same temperature, read again before the last reading
                // expired: the entry stays current for longer, so the log
                // grows with temperature changes, not with snapshots.
                Some((_, t, u)) if *t == temp && read_t <= *u => *u = (*u).max(until),
                _ => self.temps.push((self.history.len(), temp, until)),
            }
        }
        self.history.push(&record);
        Appended {
            record,
            before_ms,
            new_run,
            unix_ms,
            temp_c: temp
                .filter(|&(_, until)| i64::from(t) <= until)
                .map(|(temp, _)| temp),
        }
    }

    /// The statistics so far.
    pub(crate) fn stats(&self) -> StatsSnapshot {
        self.stats.snapshot()
    }
}

/// A session's data, shared by its connections and the commands that
/// query it.
#[derive(Debug, Clone, Default)]
pub struct SessionStore(Arc<Mutex<SessionData>>);

impl SessionStore {
    /// An empty session.
    pub fn new() -> SessionStore {
        SessionStore::default()
    }

    pub(crate) fn lock(&self) -> MutexGuard<'_, SessionData> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The next record starts a new run: a connection or a queue restart
    /// begins, so the interval before it was not measured.
    pub(crate) fn start_run(&self) {
        self.lock().timeline.restart = true;
    }

    /// The timeline's current offset, ms: session time minus device time.
    /// Anything else timed by the meter's clock (PD records, PD status)
    /// goes onto the samples' session time by adding it.
    pub(crate) fn offset_ms(&self) -> i64 {
        self.lock().timeline.offset
    }

    /// A valid snapshot read `temp_c` °C at wall-clock time `now_ms`: the
    /// records from now on carry it, up to [`SnapshotCadence::MAX_AGE`] after it.
    pub(crate) fn set_temp(&self, temp_c: f64, now_ms: u64) {
        self.lock().temp_c = Some((temp_c, now_ms));
    }

    /// Number of records in the history.
    pub fn history_len(&self) -> usize {
        self.lock().history.len()
    }

    /// The newest record of the history.
    pub(crate) fn latest_record(&self) -> Option<QueueRecord> {
        let data = self.lock();
        data.history.get(data.history.len().checked_sub(1)?)
    }

    /// Min / max buckets of the history over `t0..=t1` (session time).
    pub fn history_range(
        &self,
        series: &[Series],
        t0: u64,
        t1: u64,
        buckets: usize,
    ) -> Vec<SeriesBuckets> {
        self.lock().history.range(series, t0, t1, buckets)
    }

    /// [`Self::history_range`] widened to the record just outside each end
    /// that falls between records, so the chart runs its lines to the view's
    /// edges at any zoom (AC-9CFMD3.4).
    pub fn history_range_to_edges(
        &self,
        series: &[Series],
        t0: u64,
        t1: u64,
        buckets: usize,
    ) -> Vec<SeriesBuckets> {
        let store = self.lock();
        let (t0, t1) = store.history.widen_to_neighbours(t0, t1);
        store.history.range(series, t0, t1, buckets)
    }

    /// The gaps between measuring runs overlapping the open range `t0..t1`
    /// (session time); a gap that only touches an end is left out.
    pub fn history_gaps(&self, t0: u64, t1: u64) -> Vec<HistoryGap> {
        self.lock().history.gaps(t0, t1)
    }

    /// The statistics so far.
    pub fn stats(&self) -> StatsSnapshot {
        self.lock().stats()
    }

    /// Zero the statistics; they count again from the next record.
    pub fn reset_stats(&self) {
        self.lock().stats.reset();
    }

    /// Discard the history. The timeline goes on, so session time never
    /// goes backwards for the UI.
    pub fn clear_history(&self) {
        let mut data = self.lock();
        data.history.clear();
        data.history_unix_ms = None;
        data.temps.clear();
    }

    /// The history so far, sharing its stored samples.
    pub fn snapshot(&self) -> Records {
        self.lock().history.snapshot()
    }

    /// What an export needs: the history so far and the wall-clock time at
    /// its first record (`now_ms` when it is empty) and its temperature
    /// changes.
    pub(crate) fn export_source(&self, now_ms: u64) -> (Records, u64, TempLog) {
        let data = self.lock();
        (
            data.history.snapshot(),
            data.history_unix_ms.unwrap_or(now_ms),
            data.temps.clone(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_timeline_follows_device_time() {
        let mut t = Timeline::default();
        assert_eq!(t.map(1_000, 0), (1_000, false));
        assert_eq!(t.map(1_000, 0), (1_000, false));
        assert_eq!(t.map(1_020, 20), (1_020, false));
    }

    #[test]
    fn a_clock_that_jumps_back_continues_by_wall_clock_time() {
        let mut t = Timeline::default();
        t.map(90_000, 5_000);
        // The meter rebooted 3 s later: its clock reads 10 ms.
        assert_eq!(t.map(10, 8_000), (93_000, true));
        assert_eq!(t.map(30, 8_020), (93_020, false));
        // A jump inside one poll still moves forward by at least 1 ms.
        assert_eq!(t.map(5, 8_020), (93_021, true));
    }

    #[test]
    fn a_new_run_never_starts_at_or_before_the_last_record() {
        // A reconnect whose first record reads the very time the last run ended (a
        // clock that has not moved) still starts strictly later, so the break between
        // the runs has a length and the history reports it (AC-272WSN.3).
        let mut t = Timeline::default();
        t.map(500, 1_000);
        t.restart = true;
        assert_eq!(t.map(500, 1_000), (501, true));
        assert_eq!(t.map(500, 1_000), (501, false));
    }

    #[test]
    fn the_first_record_of_a_connection_starts_a_new_run() {
        let mut t = Timeline {
            restart: true,
            ..Timeline::default()
        };
        assert_eq!(t.map(100, 0), (100, true));
        assert_eq!(t.map(101, 1), (101, false));
        t.restart = true;
        // A clock that kept running keeps its offset.
        assert_eq!(t.map(5_000, 4_900), (5_000, true));
    }

    #[test]
    fn session_time_saturates_instead_of_wrapping() {
        let mut t = Timeline::default();
        t.map(u32::MAX - 1, 0);
        assert_eq!(t.map(0, 10_000), (u32::MAX, true));
    }

    #[test]
    fn the_temperature_log_grows_with_changes_not_with_snapshots() {
        use powerz_protocol::decode::Rate;
        let store = SessionStore::new();
        let rec = |t: u32| QueueRecord::from_raw(t, 5_000_000, 0, [0; 4], Rate::Sps1000);
        // One hour of snapshots every 500 ms, all reading 30 °C, one record each.
        for i in 0..7_200u32 {
            let t = i * 500;
            store.set_temp(30.0, 1_000 + u64::from(t));
            store.lock().append(&rec(t), 1_000 + u64::from(t));
        }
        let (records, _, temps) = store.export_source(0);
        assert_eq!(temps.len(), 1, "one entry per temperature change");
        let mut cursor = 0;
        for (i, r) in records.iter().enumerate() {
            assert_eq!(
                temp_at(&temps, &mut cursor, i, r.time_ms()),
                Some(30.0),
                "row {i}"
            );
        }
        // A pause longer than the age limit still empties the rows inside it.
        store
            .lock()
            .append(&rec(7_200 * 500 + 2_500), 1_000 + 7_200 * 500 + 2_500);
        store.set_temp(30.0, 1_000 + 7_200 * 500 + 3_000);
        store
            .lock()
            .append(&rec(7_200 * 500 + 3_000), 1_000 + 7_200 * 500 + 3_000);
        let (records, _, temps) = store.export_source(0);
        let n = records.len();
        let mut cursor = 0;
        let got: Vec<_> = (0..n)
            .map(|i| {
                temp_at(
                    &temps,
                    &mut cursor,
                    i,
                    records.get(i).map_or(0, |r| r.time_ms()),
                )
            })
            .collect();
        assert_eq!(
            got[n - 2],
            None,
            "the row in the pause kept a stale temperature"
        );
        assert_eq!(got[n - 1], Some(30.0));
    }
}
