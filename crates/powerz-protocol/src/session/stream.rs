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
        Ok(())
    }

    /// Stop the ADC queue.
    pub fn stop_stream(&mut self) -> Result<(), Error> {
        let request = frame::stop_queue(self.counter.next());
        self.expect_accept(&request, &format!("{:#04x}", request[0]))?;
        self.stream = None;
        self.last_record_ms = None;
        Ok(())
    }

    /// Poll the ADC queue once. Works without auth: the meter then serves
    /// records at its own rate.
    ///
    /// The record rate is the started stream's, else the meter's own rate as
    /// last seen in an ADC snapshot or SETTINGS. With no known rate, records
    /// decode with 1 mV CC/D± units and no gaps are reported.
    pub fn poll_stream(&mut self) -> Result<StreamPoll, Error> {
        let known = self.stream.or(self.meter_rate);
        let rate = known.unwrap_or(Rate::Sps10);
        let mut records = Vec::new();
        for section in self.get_data(Attribute::ADC_QUEUE)? {
            if section.att != Attribute::ADC_QUEUE {
                continue;
            }
            for chunk in section.payload.chunks(RECORD_LEN) {
                records.push(QueueRecord::decode(chunk, rate)?);
            }
        }
        records.sort_by_key(QueueRecord::time_ms);
        let gaps = match known {
            Some(rate) => self.track_gaps(&records, 1000 / rate.sps()),
            None => {
                self.last_record_ms = records.last().map(QueueRecord::time_ms);
                Vec::new()
            }
        };
        Ok(StreamPoll { records, gaps })
    }

    /// Gaps between the last record seen and `records`, which must be in
    /// device-time order; advances the last-record time.
    fn track_gaps(&mut self, records: &[QueueRecord], interval: u32) -> Vec<Gap> {
        let mut gaps = Vec::new();
        for r in records {
            let to_ms = r.time_ms();
            if let Some(from_ms) = self.last_record_ms {
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
            self.last_record_ms = Some(to_ms);
        }
        gaps
    }
}
