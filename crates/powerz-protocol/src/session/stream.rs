//! ADC-queue streaming: start / stop and polling with gap detection.

use std::time::Duration;

use super::{Clock, Error, Session};
use crate::decode::{QueueRecord, Rate};
use crate::frame::{self, Attribute};
use crate::transport::Transport;

/// Size of one ADC-queue record in a PUT_DATA section.
const RECORD_LEN: usize = 20;

/// A hole in the queue stream: two consecutive records more than one sample
/// interval apart. Gaps are reported, never filled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gap {
    /// Device time of the record before the hole.
    pub from_ms: u32,
    /// Device time of the record after the hole.
    pub to_ms: u32,
    /// Samples the rate implies for `from_ms..to_ms` (`(to - from) / interval`).
    pub expected: u32,
    /// Samples actually received for that span (the one at `to_ms`).
    pub received: u32,
}

/// Result of one [`Session::poll_stream`].
#[derive(Debug, Clone, PartialEq)]
pub struct StreamPoll {
    /// Decoded records in device-time order.
    pub records: Vec<QueueRecord>,
    /// Gaps found before or between these records.
    pub gaps: Vec<Gap>,
    /// The rate the session switched to during this poll, if any: after
    /// [`RATE_SWITCH_RUN`] records in a row at the steady, longer interval
    /// of another known rate `[inferred]` (a settings write re-applies the
    /// meter's own rate to the running queue). Reported once; later records
    /// decode, and gaps are measured, at that rate.
    pub rate_switch: Option<Rate>,
}

/// Records in a row at another rate's interval before the session switches
/// to that rate `[inferred]`.
pub const RATE_SWITCH_RUN: u32 = 5;

/// A run of consecutive records at one steady interval longer than the
/// expected one.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct RateRun {
    /// The known rate whose interval the run matches.
    rate: Option<Rate>,
    /// Records in the run so far.
    count: u32,
}

/// Interval, ms, between two records at `rate`.
fn interval_ms(rate: Rate) -> u32 {
    1000 / rate.sps().max(1)
}

/// Allowed jitter around an interval: a tenth of it `[inferred]`.
fn jitter_ms(interval: u32) -> u32 {
    interval / 10
}

/// The known rate, slower than `interval`, whose interval `delta` matches
/// within its jitter.
fn slower_rate_of(delta: u32, interval: u32) -> Option<Rate> {
    Rate::ALL.into_iter().find(|&r| {
        let i = interval_ms(r);
        i > interval && delta.abs_diff(i) <= jitter_ms(i)
    })
}

/// When a streaming client reads an ADC snapshot ([`Session::read_adc`],
/// e.g. for the chip temperature) between queue polls: at most once per
/// [`SnapshotCadence::PERIOD`], the first one at once. The caller reads the
/// snapshot after a poll so requests stay strictly one at a time.
#[derive(Debug, Clone, Copy, Default)]
pub struct SnapshotCadence {
    /// Time of the last snapshot taken, ms.
    last_ms: Option<u64>,
}

impl SnapshotCadence {
    /// Minimum time between two snapshots; well under the 1 s a UI needs
    /// for a fresh temperature.
    pub const PERIOD: Duration = Duration::from_millis(500);

    /// How long a snapshot value (e.g. the temperature) stays current after
    /// it was read; records later than that carry none.
    pub const MAX_AGE: Duration = Duration::from_millis(2_000);

    /// The last time, ms, a snapshot read at `read_ms` is still current
    /// ([`Self::MAX_AGE`] after it, on the same clock).
    pub fn expiry_ms(read_ms: i64) -> i64 {
        read_ms.saturating_add(i64::try_from(Self::MAX_AGE.as_millis()).unwrap_or(i64::MAX))
    }

    /// `true` when a snapshot is due at `now_ms`; it is then counted as
    /// taken, whether or not the read succeeds.
    pub fn take(&mut self, now_ms: u64) -> bool {
        let period = u64::try_from(Self::PERIOD.as_millis()).unwrap_or(u64::MAX);
        if self
            .last_ms
            .is_some_and(|t| now_ms.saturating_sub(t) < period)
        {
            return false;
        }
        self.last_ms = Some(now_ms);
        true
    }
}

/// Helper that drives [`Session::poll_stream`] at a safe period.
#[derive(Debug, Clone, Copy)]
pub struct Streamer;

impl Streamer {
    /// Poll period for `rate`: at most 30 ms at 1000 SPS, at most 200 ms
    /// otherwise.
    pub fn poll_period(rate: Rate) -> Duration {
        match rate {
            Rate::Sps1000 => Duration::from_millis(20),
            _ => Duration::from_millis(100),
        }
    }

    /// Poll `session` every [`Streamer::poll_period`] for `rate`, handing each
    /// poll to `on_poll` until it returns `false` or a poll fails.
    pub fn run<T: Transport, C: Clock>(
        session: &mut Session<T, C>,
        rate: Rate,
        mut on_poll: impl FnMut(&StreamPoll) -> bool,
    ) -> Result<(), Error> {
        loop {
            let poll = session.poll_stream()?;
            if !on_poll(&poll) {
                return Ok(());
            }
            session.clock.sleep(Streamer::poll_period(rate));
        }
    }
}

impl<T: Transport, C: Clock> Session<T, C> {
    /// Start the ADC queue at `rate`, authenticating first when needed.
    ///
    /// A running stream is stopped first.
    pub fn start_stream(&mut self, rate: Rate) -> Result<(), Error> {
        if self.auth_level == 0 {
            self.authenticate(1)?;
        }
        if self.stream.is_some() {
            self.stop_stream()?;
        }
        let request = frame::start_queue(self.counter.next(), rate.code());
        self.expect_accept(&request, &format!("{:#04x}", request[0]))?;
        self.stream = Some(rate);
        self.last_record_ms = None;
        self.rate_run = RateRun::default();
        Ok(())
    }

    /// Stop the ADC queue.
    pub fn stop_stream(&mut self) -> Result<(), Error> {
        let request = frame::stop_queue(self.counter.next());
        self.expect_accept(&request, &format!("{:#04x}", request[0]))?;
        self.stream = None;
        self.last_record_ms = None;
        self.rate_run = RateRun::default();
        Ok(())
    }

    /// Poll the ADC queue once. Works without auth: the meter then serves
    /// records at its own rate.
    ///
    /// The record rate is the started stream's, else the meter's own rate as
    /// last seen in an ADC snapshot or SETTINGS. With no known rate, records
    /// decode with 1 mV CC/D± units and no gaps are reported.
    ///
    /// When records keep arriving at the steady interval of a slower known
    /// rate, the session switches to it after [`RATE_SWITCH_RUN`] in a row
    /// and reports it in [`StreamPoll::rate_switch`] `[inferred]`; the run's
    /// records in this poll and all later ones decode at the new rate.
    pub fn poll_stream(&mut self) -> Result<StreamPoll, Error> {
        let known = self.stream.or(self.meter_rate);
        let rate = known.unwrap_or(Rate::Sps10);
        let sections = self.get_data(Attribute::ADC_QUEUE)?;
        let mut decoded = Vec::new();
        for section in &sections {
            if section.att != Attribute::ADC_QUEUE {
                continue;
            }
            // [device] A backlog reply can arrive shorter than it declares:
            // keep the whole records, drop a partial last one. The missing
            // samples surface as a gap on the next record that arrives.
            for chunk in section.payload.as_chunks::<RECORD_LEN>().0 {
                decoded.push((QueueRecord::decode(chunk, rate)?, chunk));
            }
        }
        decoded.sort_by_key(|(r, _)| r.time_ms());
        let mut records: Vec<QueueRecord> = decoded.iter().map(|(r, _)| *r).collect();
        let (gaps, rate_switch) = match known {
            Some(rate) => {
                let (gaps, switch) = self.track_queue(&records, rate);
                if let Some((from, new_rate)) = switch {
                    for (slot, (_, chunk)) in records.iter_mut().zip(&decoded).skip(from) {
                        *slot = QueueRecord::decode(chunk.as_slice(), new_rate)?;
                    }
                    if self.stream.is_some() {
                        self.stream = Some(new_rate);
                    } else {
                        self.meter_rate = Some(new_rate);
                    }
                }
                (gaps, switch.map(|(_, r)| r))
            }
            None => {
                self.last_record_ms = records.last().map(QueueRecord::time_ms);
                (Vec::new(), None)
            }
        };
        Ok(StreamPoll {
            records,
            gaps,
            rate_switch,
        })
    }

    /// Gaps between the last record seen and `records` (in device-time
    /// order) at `rate`, plus a rate switch: the index of the first record in
    /// `records` of the run that made it and the new rate. Advances the
    /// last-record time and the run.
    fn track_queue(
        &mut self,
        records: &[QueueRecord],
        rate: Rate,
    ) -> (Vec<Gap>, Option<(usize, Rate)>) {
        let mut interval = interval_ms(rate);
        let mut gaps = Vec::new();
        let mut switch = None;
        let mut run_start = 0;
        for (k, r) in records.iter().enumerate() {
            let to_ms = r.time_ms();
            if let Some(from_ms) = self.last_record_ms {
                let delta = to_ms.saturating_sub(from_ms);
                if delta > interval + jitter_ms(interval) {
                    gaps.push(Gap {
                        from_ms,
                        to_ms,
                        expected: delta / interval,
                        received: 1,
                    });
                }
                match slower_rate_of(delta, interval) {
                    Some(r) if self.rate_run.rate == Some(r) => self.rate_run.count += 1,
                    Some(r) => {
                        self.rate_run = RateRun {
                            rate: Some(r),
                            count: 1,
                        };
                        run_start = k;
                    }
                    None => self.rate_run = RateRun::default(),
                }
                if let Some(new_rate) = self.rate_run.rate
                    && self.rate_run.count >= RATE_SWITCH_RUN
                {
                    switch = Some((run_start, new_rate));
                    interval = interval_ms(new_rate);
                    self.rate_run = RateRun::default();
                }
            }
            self.last_record_ms = Some(to_ms);
        }
        (gaps, switch)
    }
}

/// Gaps between `last_ms` (the device time of the last record seen) and
/// `records`, which must be in device-time order; advances `last_ms`.
pub(super) fn gaps_after(
    last_ms: &mut Option<u32>,
    records: &[QueueRecord],
    interval: u32,
) -> Vec<Gap> {
    let mut gaps = Vec::new();
    for r in records {
        let to_ms = r.time_ms();
        if let Some(from_ms) = *last_ms {
            let delta = to_ms.saturating_sub(from_ms);
            if delta > interval {
                gaps.push(Gap {
                    from_ms,
                    to_ms,
                    expected: delta / interval,
                    received: 1,
                });
            }
        }
        *last_ms = Some(to_ms);
    }
    gaps
}
