//! Record decoders: ADC snapshot, ADC-queue records, rate codes, the
//! SETTINGS block and the device-info / OTP / UID memory blocks. Pure
//! functions over byte slices; values are kept as raw integers with typed
//! accessors returning SI units. See `docs/protocol.md`.

pub mod adc;
pub mod head;
pub mod info;
pub mod pd_packet;
pub mod queue;
pub mod rate;
pub mod records;
pub mod settings;

pub use adc::AdcSnapshot;
pub use info::{DeviceInfoBlock, OtpBlock, Uid};
pub use queue::QueueRecord;
pub use rate::Rate;
pub use settings::Settings;

/// Errors from decoding records.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    /// The input is shorter than (or not one of) the lengths the record allows.
    #[error("{what}: expected {expected} bytes, got {got}")]
    Length {
        /// Record being decoded.
        what: &'static str,
        /// Minimum (or exact) length the record needs.
        expected: usize,
        /// Length actually supplied.
        got: usize,
    },
    /// The frame is of another type than the record expects.
    #[error("{what}: expected frame type {expected:#04x}, got {got:#04x}")]
    UnexpectedType {
        /// Record being decoded.
        what: &'static str,
        /// Frame type the record needs.
        expected: u8,
        /// Frame type actually supplied (extend bit masked off).
        got: u8,
    },
    /// A stored CRC-32 does not match its data block; nothing was decoded.
    #[error("{what}: CRC mismatch")]
    Crc {
        /// Block whose CRC failed (e.g. `"SETTINGS block A"`).
        what: &'static str,
    },
}

/// Fail with [`DecodeError::Length`] unless `buf` holds at least `min` bytes.
fn need(what: &'static str, buf: &[u8], min: usize) -> Result<(), DecodeError> {
    if buf.len() < min {
        return Err(DecodeError::Length {
            what,
            expected: min,
            got: buf.len(),
        });
    }
    Ok(())
}

/// Little-endian `u16` at `off`; the caller has checked the length.
fn u16_at(p: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([p[off], p[off + 1]])
}

/// Little-endian `i16` at `off`; the caller has checked the length.
fn i16_at(p: &[u8], off: usize) -> i16 {
    i16::from_le_bytes([p[off], p[off + 1]])
}

/// Little-endian `u32` at `off`; the caller has checked the length.
fn u32_at(p: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([p[off], p[off + 1], p[off + 2], p[off + 3]])
}

/// Little-endian `i32` at `off`; the caller has checked the length.
fn i32_at(p: &[u8], off: usize) -> i32 {
    i32::from_le_bytes([p[off], p[off + 1], p[off + 2], p[off + 3]])
}

/// Convert a raw micro-unit value (µV, µA) to the base unit.
fn micro(raw: i32) -> f64 {
    f64::from(raw) / 1e6
}

/// Read a NUL-terminated string from `bytes` (or up to its end), lossily.
fn c_str(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

#[cfg(test)]
pub(crate) mod test_util {
    /// Parse a `tests/vectors/*.hex` fixture (comment lines start with `#`).
    pub(crate) fn hex_vector(text: &str) -> Vec<u8> {
        let hex: String = text
            .lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .flat_map(|l| l.chars().filter(|c| !c.is_whitespace()))
            .collect();
        hex_bytes(&hex)
    }

    /// Parse a plain hex string, ignoring whitespace.
    pub(crate) fn hex_bytes(hex: &str) -> Vec<u8> {
        let hex: String = hex.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(hex.len().is_multiple_of(2), "odd hex length");
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect()
    }

    /// Payload of the single section of a captured PUT_DATA fixture.
    pub(crate) fn section_payload(text: &str) -> Vec<u8> {
        let frame = hex_vector(text);
        let sections = crate::frame::parse_put_data(&frame).unwrap();
        sections[0].payload.to_vec()
    }

    /// Assert two floats agree to 1e-12.
    #[track_caller]
    pub(crate) fn approx(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-12,
            "expected {expected}, got {actual}"
        );
    }
}
