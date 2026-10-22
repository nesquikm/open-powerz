//! PD_PACKET chunk decoder: the 12-byte status block followed by captured
//! PD records (`b0 | u32 ts ms | u8 sop/event | ...`). Pure functions over
//! byte slices. See `docs/protocol.md`.

use super::{DecodeError, i16_at, need, u16_at, u32_at};

/// Size of the status block that starts every PD_PACKET chunk. \[device\]
pub const STATUS_LEN: usize = 12;

/// The status block of a PD_PACKET chunk. \[device\]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PdStatus {
    /// Meter time, ms.
    pub time_ms: u32,
    /// VBUS, mV.
    pub vbus_mv: u16,
    /// IBUS, mA, signed.
    pub ibus_ma: i16,
    /// CC1, mV.
    pub cc1_mv: u16,
    /// CC2, mV.
    pub cc2_mv: u16,
}

impl PdStatus {
    /// Decode the status block from the first [`STATUS_LEN`] bytes of `p`.
    pub fn decode(p: &[u8]) -> Result<PdStatus, DecodeError> {
        need("PD status block", p, STATUS_LEN)?;
        Ok(PdStatus {
            time_ms: u32_at(p, 0),
            vbus_mv: u16_at(p, 4),
            ibus_ma: i16_at(p, 6),
            cc1_mv: u16_at(p, 8),
            cc2_mv: u16_at(p, 10),
        })
    }
}

/// CC line of an attach or detach event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CcLine {
    /// CC1.
    Cc1,
    /// CC2.
    Cc2,
    /// Any other value, kept raw.
    Other(u8),
}

/// Cause of a receive-error event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiveErrorCause {
    /// BMC decoding error.
    Bmc,
    /// Wrong pulse count.
    PulseNumber,
    /// Frequency error.
    Freq,
    /// K-code error.
    Kcode,
    /// SOP error.
    Sop,
    /// EOP error.
    Eop,
    /// CRC check failed.
    CrcCheck,
    /// Data count error.
    DataCount,
    /// Unknown error.
    Unknown,
    /// Retries exhausted.
    Retries,
    /// Any other value, kept raw.
    Other(u8),
}

/// Protocol named by a monitor-type event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonitorType {
    /// USB PD.
    Pd,
    /// UFCS.
    Ufcs,
    /// VIVO.
    Vivo,
    /// Any other value, kept raw.
    Other(u8),
}

/// A decoded event record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PdEvent {
    /// Sink attached on a CC line.
    Attach(CcLine),
    /// Sink detached from a CC line.
    Detach(CcLine),
    /// Bad message ID.
    BadMessageId,
    /// Hard Reset.
    HardReset,
    /// Cable Reset.
    CableReset,
    /// Receive error.
    ReceiveError(ReceiveErrorCause),
    /// Monitor type.
    MonitorType(MonitorType),
    /// Big message.
    BigMessage,
    /// Unknown event byte, kept raw.
    Unknown(u8),
}

/// A captured PD message: SOP, PD header and raw data objects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdMessage {
    /// SOP kind (0 SOP, 1 SOP', ...).
    pub sop: u8,
    /// 16-bit PD message header.
    pub header: u16,
    /// Bytes after the PD header.
    pub body: Vec<u8>,
}

/// What a record holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PdRecordKind {
    /// PD message record (b0 bit 7).
    Message(PdMessage),
    /// Big message record (b0 bit 5).
    BigMessage(PdMessage),
    /// Event record.
    Event(PdEvent),
}

/// One record of a PD_PACKET chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdRecord {
    /// Meter time of the record, ms.
    pub time_ms: u32,
    /// Sent by the meter itself (b0 bit 6).
    pub from_meter: bool,
    /// The record's bytes, b0 included.
    pub raw: Vec<u8>,
    /// Decoded content.
    pub kind: PdRecordKind,
}

impl PdRecord {
    /// This record as PD decoder input: messages (big ones included) with
    /// their SOP, header and body; attach, detach, hard reset and cable
    /// reset as events. Other events give `None`.
    pub fn pd_input(&self) -> Option<crate::pd::Input<'_>> {
        use crate::pd::Input;
        match &self.kind {
            PdRecordKind::Message(m) | PdRecordKind::BigMessage(m) => Some(Input::Message {
                sop: m.sop,
                header: m.header,
                body: &m.body,
            }),
            PdRecordKind::Event(PdEvent::Attach(_)) => Some(Input::Attach),
            PdRecordKind::Event(PdEvent::Detach(_)) => Some(Input::Detach),
            PdRecordKind::Event(PdEvent::HardReset) => Some(Input::HardReset),
            PdRecordKind::Event(PdEvent::CableReset) => Some(Input::CableReset),
            PdRecordKind::Event(_) => None,
        }
    }
}

/// A parsed PD_PACKET chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdChunk {
    /// The status block.
    pub status: PdStatus,
    /// Records in the order they came.
    pub records: Vec<PdRecord>,
    /// Records skipped because they ran past the chunk.
    pub skipped: usize,
}

impl PdChunk {
    /// Parse a PD_PACKET section payload.
    pub fn parse(p: &[u8]) -> Result<PdChunk, DecodeError> {
        let status = PdStatus::decode(p)?;
        let mut records = Vec::new();
        let mut skipped = 0;
        let mut rest = &p[STATUS_LEN..];
        while let Some(&b0) = rest.first() {
            let Some(len) = record_len(rest) else {
                // The record (or its extra-length field) runs past the chunk:
                // nothing after it can be trusted. [static]
                skipped += 1;
                break;
            };
            let (rec, tail) = rest.split_at(len);
            rest = tail;
            match decode_record(b0, rec) {
                Some(r) => records.push(r),
                None => skipped += 1,
            }
        }
        Ok(PdChunk {
            status,
            records,
            skipped,
        })
    }
}

/// Length of the record at the start of `rest`, or `None` if it runs past
/// the end of `rest`. Lengths per `b0`: [static]
/// - bit 7: `(b0 & 0x3F) + 1` (PD message);
/// - bit 5: `(b0 & 0x1F) + 1 + u16 at +6` (big message);
/// - otherwise `(b0 & 0x1F) + 1` (event).
fn record_len(rest: &[u8]) -> Option<usize> {
    let b0 = *rest.first()?;
    let len = if b0 & 0x80 != 0 {
        usize::from(b0 & 0x3F) + 1
    } else {
        let base = usize::from(b0 & 0x1F) + 1;
        if b0 & 0x20 != 0 {
            if base < 8 {
                // Too short to hold the extra length; skip the base part.
                base
            } else {
                let extra = rest.get(6..8)?;
                base + usize::from(u16::from_le_bytes([extra[0], extra[1]]))
            }
        } else {
            base
        }
    };
    (len <= rest.len()).then_some(len)
}

/// Decode one record of exactly its own length. `None` when it is too short
/// to hold the fields its kind needs.
fn decode_record(b0: u8, rec: &[u8]) -> Option<PdRecord> {
    if rec.len() < 6 {
        return None;
    }
    let time_ms = u32_at(rec, 1);
    let kind = if b0 & 0x80 != 0 || b0 & 0x20 != 0 {
        let body_at = if b0 & 0x80 != 0 { 8 } else { 10 };
        if rec.len() < body_at {
            return None;
        }
        let header_at = body_at - 2;
        let m = PdMessage {
            sop: rec[5],
            header: u16_at(rec, header_at),
            body: rec[body_at..].to_vec(),
        };
        if b0 & 0x80 != 0 {
            PdRecordKind::Message(m)
        } else {
            PdRecordKind::BigMessage(m)
        }
    } else {
        PdRecordKind::Event(decode_event(rec[5]))
    };
    Some(PdRecord {
        time_ms,
        from_meter: b0 & 0x40 != 0,
        raw: rec.to_vec(),
        kind,
    })
}

/// Decode an event byte: low nibble code, high nibble parameter. [static]
fn decode_event(b: u8) -> PdEvent {
    let param = b >> 4;
    let cc = || match param {
        1 => CcLine::Cc1,
        2 => CcLine::Cc2,
        n => CcLine::Other(n),
    };
    match b & 0x0F {
        1 => PdEvent::Attach(cc()),
        2 => PdEvent::Detach(cc()),
        3 => PdEvent::BadMessageId,
        4 => PdEvent::HardReset,
        5 => PdEvent::CableReset,
        6 => PdEvent::ReceiveError(match param {
            0 => ReceiveErrorCause::Bmc,
            1 => ReceiveErrorCause::PulseNumber,
            2 => ReceiveErrorCause::Freq,
            3 => ReceiveErrorCause::Kcode,
            4 => ReceiveErrorCause::Sop,
            5 => ReceiveErrorCause::Eop,
            6 => ReceiveErrorCause::CrcCheck,
            7 => ReceiveErrorCause::DataCount,
            8 => ReceiveErrorCause::Unknown,
            9 => ReceiveErrorCause::Retries,
            n => ReceiveErrorCause::Other(n),
        }),
        7 => PdEvent::MonitorType(match param {
            0 => MonitorType::Pd,
            1 => MonitorType::Ufcs,
            2 => MonitorType::Vivo,
            n => MonitorType::Other(n),
        }),
        8 => PdEvent::BigMessage,
        _ => PdEvent::Unknown(b),
    }
}

/// Turns meter record times into ms since the capture start.
#[derive(Debug, Clone, Default)]
pub struct CaptureClock {
    /// Meter time of the last record seen, `None` before the first one.
    last: Option<u32>,
    /// Ms since the capture start at `last`.
    elapsed: u64,
}

impl CaptureClock {
    /// A clock that starts at the first record it sees.
    pub fn new() -> CaptureClock {
        CaptureClock::default()
    }

    /// Ms since the capture start for a record at meter time `time_ms`.
    ///
    /// A forward step (modulo 2^32, so a u32 wraparound counts as forward)
    /// adds to the elapsed time. A step backwards means the meter clock
    /// restarted: the clock resyncs to the new time without going back.
    /// Records arriving a few ms out of order are treated the same way, so
    /// the elapsed time can run a few ms ahead of the meter; it never goes
    /// back. \[inferred\]
    pub fn elapsed_ms(&mut self, time_ms: u32) -> u64 {
        if let Some(last) = self.last {
            let step = time_ms.wrapping_sub(last);
            if step < 1 << 31 {
                self.elapsed += u64::from(step);
            }
        }
        self.last = Some(time_ms);
        self.elapsed
    }
}
