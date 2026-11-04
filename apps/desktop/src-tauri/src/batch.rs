//! Pure sample batcher: groups queue records into UI batches flushed at
//! most 20 times a second, with a 4 Hz idle heartbeat (FR VHY378, HR2JAG).

use serde::Serialize;

use crate::stats::StatsSnapshot;

/// One queue record in SI units, timestamped in device time.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Sample {
    /// Device time, ms.
    pub t_ms: u32,
    /// VBUS, volts.
    pub vbus_v: f64,
    /// IBUS, amperes (signed).
    pub ibus_a: f64,
    /// CC1, volts.
    pub cc1_v: f64,
    /// CC2, volts.
    pub cc2_v: f64,
    /// D+, volts.
    pub dp_v: f64,
    /// D−, volts.
    pub dm_v: f64,
}

/// One batch of samples sent to the UI.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Batch {
    /// Samples in device-time order.
    pub samples: Vec<Sample>,
    /// Latest chip temperature, °C; `None` when the meter reports it invalid.
    pub temp_c: Option<f64>,
    /// Session statistics up to and including this batch.
    pub stats: StatsSnapshot,
}

/// Flush pending samples once this long has passed since the last batch
/// (at most 20 batches a second).
const SAMPLE_FLUSH_MS: u64 = 50;
/// Send an empty heartbeat batch after this long without one (4 Hz).
const HEARTBEAT_MS: u64 = 250;

/// Groups samples into batches flushed at most 20 times a second.
#[derive(Debug, Default)]
pub struct Batcher {
    pending: Vec<Sample>,
    temp_c: Option<f64>,
    /// Time of the last batch, ms.
    last_ms: u64,
}

impl Batcher {
    /// A batcher whose flush timer starts at `now_ms`.
    pub fn new(now_ms: u64) -> Batcher {
        Batcher {
            last_ms: now_ms,
            ..Batcher::default()
        }
    }

    /// Queue one sample.
    pub fn push(&mut self, sample: Sample) {
        self.pending.push(sample);
    }

    /// Set the temperature carried by the next batches.
    pub fn set_temp(&mut self, temp_c: Option<f64>) {
        self.temp_c = temp_c;
    }

    /// A batch when one is due at `now_ms`.
    pub fn poll(&mut self, now_ms: u64) -> Option<Batch> {
        let elapsed = now_ms.saturating_sub(self.last_ms);
        let due = if self.pending.is_empty() {
            elapsed >= HEARTBEAT_MS
        } else {
            elapsed >= SAMPLE_FLUSH_MS
        };
        due.then(|| self.take(now_ms))
    }

    /// Force pending samples out.
    pub fn flush(&mut self, now_ms: u64) -> Option<Batch> {
        (!self.pending.is_empty()).then(|| self.take(now_ms))
    }

    /// The earliest time the next batch can be due, ms.
    pub fn next_due_ms(&self) -> u64 {
        if self.pending.is_empty() {
            self.last_ms.saturating_add(HEARTBEAT_MS)
        } else {
            self.last_ms.saturating_add(SAMPLE_FLUSH_MS)
        }
    }

    fn take(&mut self, now_ms: u64) -> Batch {
        self.last_ms = now_ms;
        Batch {
            samples: std::mem::take(&mut self.pending),
            temp_c: self.temp_c,
            stats: StatsSnapshot::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(t_ms: u32) -> Sample {
        Sample {
            t_ms,
            vbus_v: 5.0,
            ibus_a: 1.0,
            cc1_v: 1.68,
            cc2_v: 0.0,
            dp_v: 0.6,
            dm_v: 0.6,
        }
    }

    // ---- AC-HR2JAG.1 (supersedes AC-VHY378.5's 16.7 ms / 50 ms): flush
    // samples at most 20 times a second, heartbeat every 250 ms ----

    #[test]
    fn batcher_waits_50_ms_before_flushing_samples() {
        let mut b = Batcher::new(1_000);
        b.push(s(1));
        b.push(s(2));
        assert_eq!(b.poll(1_017), None, "flushed at 60 Hz");
        assert_eq!(b.poll(1_049), None, "flushed before 50 ms");
        let batch = b.poll(1_050).expect("no flush after 50 ms");
        let t: Vec<u32> = batch.samples.iter().map(|x| x.t_ms).collect();
        assert_eq!(t, vec![1, 2]);
        b.push(s(3));
        assert_eq!(b.poll(1_099), None, "two flushes under 50 ms apart");
        assert!(b.poll(1_100).is_some());
    }

    #[test]
    fn batcher_sends_an_empty_batch_every_250_ms_without_samples() {
        let mut b = Batcher::new(0);
        assert_eq!(b.poll(50), None, "heartbeat at the old 20 Hz");
        assert_eq!(b.poll(249), None);
        let batch = b.poll(250).expect("no heartbeat batch at 250 ms");
        assert!(batch.samples.is_empty());
        assert_eq!(b.poll(499), None);
        assert!(b.poll(500).is_some());
    }

    #[test]
    fn next_due_is_50_ms_with_samples_and_250_ms_idle() {
        let mut b = Batcher::new(1_000);
        assert_eq!(b.next_due_ms(), 1_250);
        b.push(s(1));
        assert_eq!(b.next_due_ms(), 1_050);
    }

    #[test]
    fn batcher_carries_the_latest_temperature() {
        let mut b = Batcher::new(0);
        b.set_temp(Some(30.5));
        b.push(s(1));
        assert_eq!(b.poll(50).unwrap().temp_c, Some(30.5));
        b.set_temp(Some(31.0));
        b.push(s(2));
        assert_eq!(b.poll(100).unwrap().temp_c, Some(31.0));
        b.set_temp(None);
        b.push(s(3));
        assert_eq!(b.poll(150).unwrap().temp_c, None);
    }

    #[test]
    fn batcher_flush_forces_pending_samples_out() {
        let mut b = Batcher::new(0);
        assert_eq!(b.flush(1), None, "flush with nothing pending");
        b.push(s(7));
        let batch = b.flush(2).expect("pending sample not flushed");
        assert_eq!(batch.samples.len(), 1);
        assert_eq!(batch.samples[0].t_ms, 7);
        assert_eq!(b.poll(10), None);
    }

    #[test]
    fn ten_seconds_at_1000_sps_is_at_most_200_batches_in_order() {
        let mut b = Batcher::new(0);
        let mut out = Vec::new();
        let mut times = Vec::new();
        for t in 1..=10_000u32 {
            b.push(s(t));
            if let Some(batch) = b.poll(u64::from(t)) {
                times.push(u64::from(t));
                out.extend(batch.samples.iter().map(|x| x.t_ms));
            }
        }
        if let Some(batch) = b.flush(10_001) {
            times.push(10_001);
            out.extend(batch.samples.iter().map(|x| x.t_ms));
        }
        assert!(times.len() <= 200, "{} batches", times.len());
        assert!(times.len() >= 190, "{} batches: below 20 Hz", times.len());
        // The final forced flush at session end is exempt from the spacing.
        let polled = &times[..times.len() - 1];
        assert!(
            polled.windows(2).all(|w| w[1] - w[0] >= 50),
            "batches under 50 ms apart"
        );
        assert_eq!(out, (1..=10_000).collect::<Vec<_>>());
    }

    #[test]
    fn ten_idle_seconds_are_at_most_40_heartbeats() {
        let mut b = Batcher::new(0);
        let mut beats = 0;
        for t in 1..=10_000u64 {
            if let Some(batch) = b.poll(t) {
                assert!(batch.samples.is_empty());
                beats += 1;
            }
        }
        assert!(beats <= 40, "{beats} heartbeats in 10 s");
        assert!(beats >= 39, "only {beats} heartbeats in 10 s");
    }
}
