//! Offline records ("files") stored on the meter: the RecordList entries
//! returned by GET_DATA `0x200` and the 16-byte samples of a record.
//! Pure functions over byte slices. See `docs/protocol.md`.
//!
//! Entry layout \[static\]: name (NUL-terminated, at most 16 bytes) at 0x00,
//! u8 interval at 0x11, u16 point count at 0x12, u32 duration at 0x18,
//! i32 charge (µAh) at 0x1C, i32 energy (µWh) at 0x20, u32 flash offset at
//! 0x24. Byte 0x10 and bytes 0x14..0x18 and 0x28..0x30 are unknown and kept
//! only in [`RecordEntry::raw`].

use super::settings::Tag;
use super::{DecodeError, c_str, i32_at, micro, need, u16_at, u32_at};

/// Size of one RecordList entry. \[static\]
pub const ENTRY_LEN: usize = 48;

/// Size of one record sample. \[static\]
pub const SAMPLE_LEN: usize = 16;

/// Memory-read address of flash offset 0 of the record area. \[static\]
pub const FLASH_WINDOW_BASE: u32 = 0x9810_0000;

/// Size of the record area in flash. \[inferred\]
pub const RECORD_AREA_LEN: u32 = 0x20_0000;

/// Longest record name in bytes. \[inferred\]
const NAME_LEN: usize = 16;

/// One decoded RecordList entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordEntry {
    /// Record name (NUL-terminated ASCII on the wire, decoded lossily).
    pub name: String,
    /// Sample interval in seconds.
    pub interval_s: u8,
    /// Number of 16-byte samples.
    pub points: u16,
    /// Duration in seconds.
    pub duration_s: u32,
    /// Charge in µAh (signed).
    pub charge_uah: i32,
    /// Energy in µWh (signed).
    pub energy_uwh: i32,
    /// Offset of the samples in the record area.
    pub offset: u32,
    /// All 48 bytes of the entry, unknown ones included.
    pub raw: [u8; ENTRY_LEN],
}

impl RecordEntry {
    /// Decode one entry from the first [`ENTRY_LEN`] bytes of `buf`.
    pub fn decode(buf: &[u8]) -> Result<RecordEntry, DecodeError> {
        need("record entry", buf, ENTRY_LEN)?;
        let mut raw = [0u8; ENTRY_LEN];
        raw.copy_from_slice(&buf[..ENTRY_LEN]);
        Ok(RecordEntry {
            name: c_str(&raw[..NAME_LEN]),
            interval_s: raw[0x11],
            points: u16_at(&raw, 0x12),
            duration_s: u32_at(&raw, 0x18),
            charge_uah: i32_at(&raw, 0x1C),
            energy_uwh: i32_at(&raw, 0x20),
            offset: u32_at(&raw, 0x24),
            raw,
        })
    }

    /// Charge in Ah.
    pub fn charge_ah(&self) -> f64 {
        micro(self.charge_uah)
    }

    /// Energy in Wh.
    pub fn energy_wh(&self) -> f64 {
        micro(self.energy_uwh)
    }

    /// Memory-read window `(addr, len)` of this record's samples:
    /// [`FLASH_WINDOW_BASE`] + offset, points × [`SAMPLE_LEN`] bytes.
    /// `None` for a record with no samples or one that does not lie wholly
    /// inside the record area.
    pub fn read_window(&self) -> Option<(u32, u32)> {
        if self.points == 0 {
            return None;
        }
        let len = u32::from(self.points) * SAMPLE_LEN as u32;
        let end = self.offset.checked_add(len)?;
        if end > RECORD_AREA_LEN {
            return None;
        }
        Some((FLASH_WINDOW_BASE + self.offset, len))
    }

    /// Time of sample `index` in seconds from the record start.
    /// \[inferred\]: samples are evenly spaced by the entry's interval.
    pub fn sample_time_s(&self, index: usize) -> u64 {
        u64::from(self.interval_s).saturating_mul(index as u64)
    }
}

/// Decode a RecordList payload: a whole number of [`ENTRY_LEN`]-byte
/// entries. An empty payload is an empty list.
pub fn decode_record_list(payload: &[u8]) -> Result<Vec<RecordEntry>, DecodeError> {
    if !payload.len().is_multiple_of(ENTRY_LEN) {
        return Err(DecodeError::Length {
            what: "record list",
            expected: payload.len().next_multiple_of(ENTRY_LEN),
            got: payload.len(),
        });
    }
    payload
        .as_chunks::<ENTRY_LEN>()
        .0
        .iter()
        .map(|e| RecordEntry::decode(e))
        .collect()
}

/// One 16-byte record sample.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordSample {
    /// VBUS in µV.
    pub vbus_uv: i32,
    /// IBUS in µA (signed).
    pub ibus_ua: i32,
    /// Cumulative charge in µAh.
    pub charge_uah: i32,
    /// Cumulative energy in µWh.
    pub energy_uwh: i32,
    /// The raw 16 bytes.
    pub raw: [u8; SAMPLE_LEN],
}

impl RecordSample {
    /// Decode one sample.
    pub fn decode(buf: &[u8]) -> Result<RecordSample, DecodeError> {
        need("record sample", buf, SAMPLE_LEN)?;
        let mut raw = [0u8; SAMPLE_LEN];
        raw.copy_from_slice(&buf[..SAMPLE_LEN]);
        Ok(RecordSample {
            vbus_uv: i32_at(&raw, 0),
            ibus_ua: i32_at(&raw, 4),
            charge_uah: i32_at(&raw, 8),
            energy_uwh: i32_at(&raw, 12),
            raw,
        })
    }

    /// VBUS in V.
    pub fn vbus_v(&self) -> f64 {
        micro(self.vbus_uv)
    }

    /// IBUS in A.
    pub fn ibus_a(&self) -> f64 {
        micro(self.ibus_ua)
    }

    /// Charge in Ah.
    pub fn charge_ah(&self) -> f64 {
        micro(self.charge_uah)
    }

    /// Energy in Wh.
    pub fn energy_wh(&self) -> f64 {
        micro(self.energy_uwh)
    }
}

/// A documented sample field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SampleField {
    /// Stable key.
    pub key: &'static str,
    /// Byte offset in the sample.
    pub offset: usize,
    /// Confidence.
    pub tag: Tag,
}

/// The documented sample fields: four little-endian i32 values, layout
/// seen statically and not yet compared with a recording made on the meter.
pub const SAMPLE_FIELDS: &[SampleField] = &[
    SampleField {
        key: "vbus",
        offset: 0,
        tag: Tag::Inferred,
    },
    SampleField {
        key: "ibus",
        offset: 4,
        tag: Tag::Inferred,
    },
    SampleField {
        key: "charge",
        offset: 8,
        tag: Tag::Inferred,
    },
    SampleField {
        key: "energy",
        offset: 12,
        tag: Tag::Inferred,
    },
];

/// Decode the samples of `entry` from `data`.
pub fn decode_samples(entry: &RecordEntry, data: &[u8]) -> Result<Vec<RecordSample>, DecodeError> {
    let expected = usize::from(entry.points) * SAMPLE_LEN;
    if data.len() != expected {
        return Err(DecodeError::Length {
            what: "record samples",
            expected,
            got: data.len(),
        });
    }
    data.as_chunks::<SAMPLE_LEN>()
        .0
        .iter()
        .map(|s| RecordSample::decode(s))
        .collect()
}
