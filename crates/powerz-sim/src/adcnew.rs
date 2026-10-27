//! The simulated ADCNEW push stream on the CDC persona (FR 1HHT02):
//! `02` / `02 <att>` start it, `01` keeps it alive, `03` stops it, and it
//! ends one hour after the last start or keep-alive. Frames are produced
//! lazily up to the current uptime, like the ADC queue.

use powerz_protocol::adcnew::crc8;

use crate::queue::{RECORD_LEN, interval_ms, record};
use crate::{Sample, Scenario};

/// The stream ends this long after the last `02` or `01`. [doc]
const EXPIRY_MS: u64 = 60 * 60 * 1000;
/// Frame type of every stream frame.
const TYPE_FRAME: u8 = 0x02;
/// Record frames carry att `0x10 | rate code`. [device]
const RECORD_ATT_BASE: u8 = 0x10;
/// Att of the first frame after a bare `02`. [device]
const FIRST_FRAME_ATT: u8 = 0x03;

/// Records per frame for a rate code: 11 at 1000/s, else 1. [device]
fn records_per_frame(code: u8) -> usize {
    if code == 3 { 11 } else { 1 }
}

/// One frame: header (type, att, body words, CRC-8) plus body, with a
/// broken CRC if `corrupt`.
pub(crate) fn encode(att: u8, body: &[u8], corrupt: bool) -> Vec<u8> {
    let words = u8::try_from(body.len() / 4).unwrap_or(u8::MAX);
    let mut crc = crc8(body);
    if corrupt {
        crc ^= 0xFF;
    }
    let mut out = vec![TYPE_FRAME, att, words, crc];
    out.extend_from_slice(body);
    out
}

/// A running ADCNEW stream.
#[derive(Debug)]
pub(crate) struct AdcnewStream {
    /// Rate code of the records.
    code: u8,
    /// Uptime of the next record to produce.
    next_ms: u64,
    /// The stream ends at this uptime (exclusive).
    expires_ms: u64,
    /// Records due but not yet sent (a frame not yet full).
    partial: Vec<[u8; RECORD_LEN]>,
    /// Payload of the att-0x03 first frame, still to send.
    first: Option<Vec<u8>>,
}

impl AdcnewStream {
    /// A stream at rate `code` started at uptime `now`; `first` is the body
    /// of the att-0x03 first frame (bare `02` only).
    pub(crate) fn start(code: u8, now: u64, first: Option<Vec<u8>>) -> AdcnewStream {
        AdcnewStream {
            code,
            next_ms: now,
            expires_ms: now + EXPIRY_MS,
            partial: Vec::new(),
            first,
        }
    }

    /// Whether the stream still runs at uptime `now`.
    pub(crate) fn active(&self, now: u64) -> bool {
        now < self.expires_ms
    }

    /// Keep-alive at uptime `now`: the hour starts again.
    pub(crate) fn refresh(&mut self, now: u64) {
        self.expires_ms = now + EXPIRY_MS;
    }

    /// Every frame due up to uptime `now`, as `(att, body)` pairs.
    pub(crate) fn due(&mut self, scenario: Scenario, now: u64) -> Vec<(u8, Vec<u8>)> {
        let mut out = Vec::new();
        if let Some(body) = self.first.take() {
            out.push((FIRST_FRAME_ATT, body));
        }
        let end = now.min(self.expires_ms.saturating_sub(1));
        let step = interval_ms(self.code);
        let per = records_per_frame(self.code);
        let att = RECORD_ATT_BASE | self.code;
        while self.next_ms <= end {
            let t = self.next_ms;
            self.partial.push(record(&scenario.sample(t), t, self.code));
            self.next_ms += step;
            if self.partial.len() == per {
                out.push((att, self.partial.drain(..).flatten().collect()));
            }
        }
        if !self.active(now) && !self.partial.is_empty() {
            out.push((att, self.partial.drain(..).flatten().collect()));
        }
        out
    }
}

/// Body of the att-0x03 first frame: an ADC section payload of `s`
/// `[inferred]` (the real frame chains ADC and queue sections).
pub(crate) fn first_frame_body(s: &Sample, rate_code: u8) -> Vec<u8> {
    crate::device::adc_payload(s, rate_code)
}
