//! Session data shared by every connection to one meter (FR AH2AEJ,
//! VHY378): the history, its wall-clock start, the statistics and the
//! session timeline. A reconnect of the same serial gets the same store, so
//! the history and the statistics go on across the outage.
//!
//! Queries run on the caller's thread under the store lock; the worker
//! takes the lock only briefly per poll to append.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use powerz_protocol::decode::QueueRecord;

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
        if let Some((last, wall)) = self.last
            && t < last
        {
            let step = now_ms.saturating_sub(wall).max(1);
            let target = u64::from(last).saturating_add(step);
            t = u32::try_from(target).unwrap_or(u32::MAX);
            self.offset = i64::from(t) - device;
            jumped = true;
        }
        let new_run = std::mem::take(&mut self.restart) || jumped;
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
        self.history_unix_ms.get_or_insert(now_ms);
        self.history.push(&record);
        Appended {
            record,
            before_ms,
            new_run,
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

    /// Number of records in the history.
    pub fn history_len(&self) -> usize {
        self.lock().history.len()
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
    }

    /// The history so far, sharing its stored samples.
    pub fn snapshot(&self) -> Records {
        self.lock().history.snapshot()
    }

    /// What an export needs: the history so far and the wall-clock time at
    /// its first record (`now_ms` when it is empty).
    pub(crate) fn export_source(&self, now_ms: u64) -> (Records, u64) {
        let data = self.lock();
        (
            data.history.snapshot(),
            data.history_unix_ms.unwrap_or(now_ms),
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
}
