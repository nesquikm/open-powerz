//! The simulated ADC queue: always running, at the meter rate or the rate
//! set by 0x0E, records produced lazily up to the current uptime.

use std::collections::VecDeque;

use crate::{Sample, Scenario};

/// Records one reply (and the meter's buffer) holds: the 6-bit `chunk`.
pub(crate) const MAX_RECORDS: usize = 63;
/// Length of one queue record.
pub(crate) const RECORD_LEN: usize = 20;

/// Sample interval in ms for a queue rate code (4 behaves like 0).
fn interval_ms(code: u8) -> u64 {
    match code {
        1 => 100,
        2 => 20,
        3 => 1,
        _ => 500,
    }
}

/// The meter's ADC queue.
#[derive(Debug)]
pub(crate) struct AdcQueue {
    /// Rate code the queue runs at.
    code: u8,
    /// Uptime of the next record to produce.
    next_ms: u64,
    /// Buffered records, oldest first.
    records: VecDeque<[u8; RECORD_LEN]>,
}

impl AdcQueue {
    /// A queue running at `code` from uptime 0.
    pub(crate) fn new(code: u8) -> AdcQueue {
        AdcQueue {
            code,
            next_ms: 0,
            records: VecDeque::new(),
        }
    }

    /// Rate code the queue runs at.
    pub(crate) fn code(&self) -> u8 {
        self.code
    }

    /// Switch to `code` at uptime `now`: the buffer restarts at the new rate
    /// with a record at `now`.
    pub(crate) fn set_rate(&mut self, code: u8, now: u64) {
        self.code = code;
        self.records.clear();
        self.next_ms = now;
    }

    /// Produce every record due up to `now`; the oldest are dropped when the
    /// buffer overflows.
    pub(crate) fn fill(&mut self, scenario: Scenario, now: u64) {
        if self.next_ms > now {
            return;
        }
        let step = interval_ms(self.code);
        let due = (now - self.next_ms) / step + 1;
        if due > MAX_RECORDS as u64 {
            self.records.clear();
            self.next_ms += (due - MAX_RECORDS as u64) * step;
        }
        while self.next_ms <= now {
            let t = self.next_ms;
            self.records
                .push_back(record(&scenario.sample(t), t, self.code));
            if self.records.len() > MAX_RECORDS {
                self.records.pop_front();
            }
            self.next_ms += step;
        }
    }

    /// Take up to [`MAX_RECORDS`] of the oldest records.
    pub(crate) fn take(&mut self) -> Vec<[u8; RECORD_LEN]> {
        let n = self.records.len().min(MAX_RECORDS);
        self.records.drain(..n).collect()
    }
}

/// Encode one 20-byte record of `s` at uptime `t_ms`.
fn record(s: &Sample, t_ms: u64, code: u8) -> [u8; RECORD_LEN] {
    let micro = |v: f64| ((v * 1e6).round() as i32).to_le_bytes();
    let scale = if matches!(code, 0 | 4) {
        10_000.0
    } else {
        1_000.0
    };
    let line = |v: f64| ((v * scale).round().clamp(0.0, 65535.0) as u16).to_le_bytes();
    let mut r = [0u8; RECORD_LEN];
    r[0x00..0x04].copy_from_slice(&(t_ms as u32).to_le_bytes());
    r[0x04..0x08].copy_from_slice(&micro(s.vbus_v));
    r[0x08..0x0C].copy_from_slice(&micro(s.ibus_a));
    for (off, v) in [
        (0x0C, s.cc1_v),
        (0x0E, s.cc2_v),
        (0x10, s.dp_v),
        (0x12, s.dm_v),
    ] {
        r[off..off + 2].copy_from_slice(&line(v));
    }
    r
}
