//! Memory-read blocks: device-info (0x420, 0x4420), OTP (0x03000C00) and
//! MCU UID (0x40010450). See `docs/protocol.md`.
//!
//! Test fixtures here are synthetic: made-up model, version, serial and UID.

use super::{DecodeError, c_str, need};
use std::fmt;

/// Length of a device-info or OTP block.
const BLOCK_LEN: usize = 64;
/// Length of the MCU UID.
const UID_LEN: usize = 12;
/// Width of each string field in the info block.
const FIELD_LEN: usize = 12;

/// A decoded 64-byte device-info block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfoBlock {
    model: String,
    version: String,
    date: String,
    extra: String,
}

impl DeviceInfoBlock {
    /// Decode a 64-byte info block; `Ok(None)` when the first u32 is
    /// 0xFFFFFFFF ("none").
    pub fn decode(block: &[u8]) -> Result<Option<DeviceInfoBlock>, DecodeError> {
        need("device-info block", block, BLOCK_LEN)?;
        if block[..4] == [0xFF; 4] {
            return Ok(None);
        }
        let field = |off: usize| c_str(&block[off..off + FIELD_LEN]);
        Ok(Some(DeviceInfoBlock {
            model: field(0x10),
            version: field(0x1C),
            date: field(0x28),
            extra: field(0x34),
        }))
    }

    /// Model string at 0x10.
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Version string at 0x1C.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Date string at 0x28.
    pub fn date(&self) -> &str {
        &self.date
    }

    /// Extra string at 0x34.
    pub fn extra(&self) -> &str {
        &self.extra
    }
}

/// The 64-byte OTP block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OtpBlock {
    raw: [u8; BLOCK_LEN],
}

impl OtpBlock {
    /// Decode the 64-byte OTP block.
    pub fn decode(block: &[u8]) -> Result<OtpBlock, DecodeError> {
        need("OTP block", block, BLOCK_LEN)?;
        let mut raw = [0u8; BLOCK_LEN];
        raw.copy_from_slice(&block[..BLOCK_LEN]);
        Ok(OtpBlock { raw })
    }

    /// Serial number: the first space-separated ASCII token.
    pub fn serial(&self) -> String {
        let end = self
            .raw
            .iter()
            .position(|&b| b == b' ' || b == 0 || b == 0xFF)
            .unwrap_or(BLOCK_LEN);
        String::from_utf8_lossy(&self.raw[..end]).into_owned()
    }
}

/// The 12-byte MCU unique ID; `Display` prints it as hex.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Uid(pub [u8; UID_LEN]);

impl Uid {
    /// Decode a UID block, which must be exactly 12 bytes.
    pub fn decode(block: &[u8]) -> Result<Uid, DecodeError> {
        let raw: [u8; UID_LEN] = block.try_into().map_err(|_| DecodeError::Length {
            what: "UID",
            expected: UID_LEN,
            got: block.len(),
        })?;
        Ok(Uid(raw))
    }
}

impl fmt::Display for Uid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|b| write!(f, "{b:02x}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Write `s` NUL-terminated at `off`.
    fn put_str(block: &mut [u8], off: usize, s: &str) {
        block[off..off + s.len()].copy_from_slice(s.as_bytes());
        block[off + s.len()] = 0;
    }

    /// Synthetic app-info block laid out like 0x4420 (made-up values).
    fn synthetic_info() -> Vec<u8> {
        let mut b = vec![0xFFu8; 64];
        b[0x00..0x04].copy_from_slice(&0x4000u32.to_le_bytes());
        b[0x04..0x08].copy_from_slice(&0x0001_2345u32.to_le_bytes());
        b[0x08..0x0C].copy_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
        b[0x0C..0x10].copy_from_slice(&0u32.to_le_bytes());
        put_str(&mut b, 0x10, "TESTMODEL");
        put_str(&mut b, 0x1C, "9.8.7");
        put_str(&mut b, 0x28, "Jan 01 2000");
        put_str(&mut b, 0x34, "XTRA");
        b
    }

    /// Synthetic OTP block: serial, hex id, Unix time, tag, 0xFF padding.
    fn synthetic_otp() -> Vec<u8> {
        let text = b"123456 00112233445566778899aabbccddeeff 946684800 ZZ";
        let mut b = vec![0xFFu8; 64];
        b[..text.len()].copy_from_slice(text);
        b[text.len()] = 0;
        b
    }

    // ---- AC-E88WR5.7: info / OTP / UID blocks ----

    #[test]
    fn decode_info_block_four_strings() {
        let info = DeviceInfoBlock::decode(&synthetic_info()).unwrap().unwrap();
        assert_eq!(info.model(), "TESTMODEL");
        assert_eq!(info.version(), "9.8.7");
        assert_eq!(info.date(), "Jan 01 2000");
        assert_eq!(info.extra(), "XTRA");
    }

    #[test]
    fn decode_info_block_strings_stop_at_nul_not_at_field_end() {
        let mut b = synthetic_info();
        // Short strings with junk after the NUL inside each 12-byte field.
        for off in [0x10usize, 0x1C, 0x28, 0x34] {
            b[off..off + 12].copy_from_slice(b"AB\0junkjunk!");
        }
        let info = DeviceInfoBlock::decode(&b).unwrap().unwrap();
        assert_eq!(info.model(), "AB");
        assert_eq!(info.version(), "AB");
        assert_eq!(info.date(), "AB");
        assert_eq!(info.extra(), "AB");
    }

    #[test]
    fn decode_info_block_first_word_ffffffff_is_none() {
        let mut b = synthetic_info();
        b[0x00..0x04].copy_from_slice(&[0xFF; 4]);
        assert_eq!(DeviceInfoBlock::decode(&b).unwrap(), None);
        assert_eq!(DeviceInfoBlock::decode(&[0xFFu8; 64]).unwrap(), None);
    }

    #[test]
    fn decode_info_block_short_is_error() {
        let b = synthetic_info();
        for len in [0usize, 4, 16, 63] {
            assert!(
                matches!(
                    DeviceInfoBlock::decode(&b[..len]),
                    Err(DecodeError::Length { got, .. }) if got == len
                ),
                "len {len} should be a length error"
            );
        }
    }

    #[test]
    fn decode_otp_serial_is_first_token() {
        let otp = OtpBlock::decode(&synthetic_otp()).unwrap();
        assert_eq!(otp.serial(), "123456");
    }

    #[test]
    fn decode_otp_serial_other_value() {
        let mut b = vec![0u8; 64];
        let text = b"SN0042XY abcdef";
        b[..text.len()].copy_from_slice(text);
        assert_eq!(OtpBlock::decode(&b).unwrap().serial(), "SN0042XY");
    }

    #[test]
    fn decode_otp_short_is_error() {
        let b = synthetic_otp();
        for len in [0usize, 6, 63] {
            assert!(
                matches!(
                    OtpBlock::decode(&b[..len]),
                    Err(DecodeError::Length { got, .. }) if got == len
                ),
                "len {len} should be a length error"
            );
        }
    }

    #[test]
    fn decode_uid_formats_as_hex() {
        let raw = [
            0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF, 0x00, 0x0F, 0xF0, 0xFF,
        ];
        let uid = Uid::decode(&raw).unwrap();
        assert_eq!(uid, Uid(raw));
        assert_eq!(uid.to_string(), "0123456789abcdef000ff0ff");
        assert_eq!(Uid([0; 12]).to_string(), "000000000000000000000000");
    }

    #[test]
    fn decode_uid_wrong_length_is_error() {
        let raw = [0x5Au8; 12];
        for len in [0usize, 4, 11] {
            assert!(
                matches!(
                    Uid::decode(&raw[..len]),
                    Err(DecodeError::Length { got, .. }) if got == len
                ),
                "len {len} should be a length error"
            );
        }
    }
}
