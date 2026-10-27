//! Offline records ("files") stored on the meter: the RecordList entries
//! returned by GET_DATA `0x200` and the 16-byte samples of a record.
//! Pure functions over byte slices. See `docs/protocol.md`.
//!
//! Entry layout \[static\], checked on the meter \[device\]: name
//! (NUL-terminated, at most 16 bytes) at 0x00, u8 interval at 0x11, u16
//! point count at 0x12, u32 capacity in points at 0x14, u32 duration at
//! 0x18, i32 charge (µAh) at 0x1C, i32 energy (µWh) at 0x20, u32 flash
//! offset at 0x24. Charge and energy are stored signed (negative for one
//! current direction); the meter's screen shows their magnitude. Byte 0x10
//! and bytes 0x28..0x30 are unknown and kept only in [`RecordEntry::raw`];
//! there is no start timestamp.

use super::settings::Tag;
use super::{DecodeError, c_str, i32_at, micro, need, u16_at, u32_at};

/// Size of one RecordList entry. \[static\]
pub const ENTRY_LEN: usize = 48;

/// Size of one record sample. \[static\]
pub const SAMPLE_LEN: usize = 16;

/// Memory-read address of flash offset 0 of the record area. \[static\]
pub const FLASH_WINDOW_BASE: u32 = 0x9810_0000;

/// Most bytes one memory read (0x44) returns: a longer read gets its
/// confirmation and the first 256 bytes, then nothing. \[device\]
pub const MEMORY_READ_MAX_LEN: u32 = 256;

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
    /// Capacity of the record in points (the meter shows `points/capacity`).
    /// \[device\]
    pub capacity_points: u32,
    /// Duration in seconds.
    pub duration_s: u32,
    /// Charge in µAh, as stored: signed, negative for one current direction.
    /// \[device\]
    pub charge_uah: i32,
    /// Energy in µWh, as stored: signed like the charge. \[device\]
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
            capacity_points: u32_at(&raw, 0x14),
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

    /// Charge in Ah as the meter shows it: the magnitude of the stored value.
    pub fn charge_magnitude_ah(&self) -> f64 {
        f64::from(self.charge_uah.unsigned_abs()) / 1e6
    }

    /// Energy in Wh as the meter shows it: the magnitude of the stored value.
    pub fn energy_magnitude_wh(&self) -> f64 {
        f64::from(self.energy_uwh.unsigned_abs()) / 1e6
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

    /// The memory reads that download [`read_window`](Self::read_window):
    /// consecutive `(addr, len)` chunks of at most [`MEMORY_READ_MAX_LEN`]
    /// bytes that tile the window exactly; empty when there is no window.
    pub fn read_chunks(&self) -> Vec<(u32, u32)> {
        let Some((addr, len)) = self.read_window() else {
            return Vec::new();
        };
        (0..len)
            .step_by(MEMORY_READ_MAX_LEN as usize)
            .map(|k| (addr + k, (len - k).min(MEMORY_READ_MAX_LEN)))
            .collect()
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
    /// Cumulative charge in µAh (signed).
    pub charge_uah: i32,
    /// Cumulative energy in µWh (signed).
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

/// The documented sample fields: four little-endian i32 values, one sample
/// per interval. Confirmed against a recording made on the meter
/// \[device\]: IBUS, charge and energy are signed (negative for one current
/// direction), charge and energy are running totals, and the last sample's
/// totals equal the meter's record screen.
pub const SAMPLE_FIELDS: &[SampleField] = &[
    SampleField {
        key: "vbus",
        offset: 0,
        tag: Tag::Confirmed,
    },
    SampleField {
        key: "ibus",
        offset: 4,
        tag: Tag::Confirmed,
    },
    SampleField {
        key: "charge",
        offset: 8,
        tag: Tag::Confirmed,
    },
    SampleField {
        key: "energy",
        offset: 12,
        tag: Tag::Confirmed,
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
