//! SETTINGS block (`ATT_SETTINGS`, 180 bytes, two CRC-32 blocks).
//! See `docs/protocol.md`.

use super::{DecodeError, c_str, need, u32_at};

/// Length of the SETTINGS section payload.
const LEN: usize = 180;

/// The 180-byte SETTINGS payload with its CRC verdicts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    raw: [u8; LEN],
    block_a_crc_ok: bool,
    block_b_crc_ok: bool,
}

/// CRC-32 of `b[data]` equals the little-endian u32 stored at `crc_off`.
fn crc_ok(b: &[u8], data: core::ops::Range<usize>, crc_off: usize) -> bool {
    crc32fast::hash(&b[data]) == u32_at(b, crc_off)
}

/// RAM overlay inside block A (device session 1, FW 2.0.6) [inferred].
const OVERLAY: std::ops::Range<usize> = 0x10..0x14;

impl Settings {
    /// Decode the 180-byte SETTINGS section payload.
    pub fn decode(payload: &[u8]) -> Result<Settings, DecodeError> {
        need("SETTINGS", payload, LEN)?;
        let mut raw = [0u8; LEN];
        raw.copy_from_slice(&payload[..LEN]);
        // Bytes 0x10..0x14 are a RAM overlay (the boot time after a reboot)
        // that the stored CRC does not cover; it was computed over 0xFFFFFFFF.
        let mut flash = raw;
        flash[OVERLAY].fill(0xFF);
        Ok(Settings {
            block_a_crc_ok: crc_ok(&raw, 0x00..0x5C, 0x5C) || crc_ok(&flash, 0x00..0x5C, 0x5C),
            block_b_crc_ok: crc_ok(&raw, 0x60..0xB0, 0xB0),
            raw,
        })
    }

    /// Block A (0x00..0x5B) matches the CRC-32 at 0x5C.
    pub fn block_a_crc_ok(&self) -> bool {
        self.block_a_crc_ok
    }

    /// Block B (0x60..0xAF) matches the CRC-32 at 0xB0.
    pub fn block_b_crc_ok(&self) -> bool {
        self.block_b_crc_ok
    }

    /// Meter queue rate code: bits 2..3 of the u32 at 0x60.
    pub fn rate_code(&self) -> u8 {
        ((u32_at(&self.raw, 0x60) >> 2) & 0b11) as u8
    }

    /// NUL-terminated device name at 0x70.
    pub fn name(&self) -> String {
        c_str(&self.raw[0x70..0xB0])
    }

    /// The raw 180 payload bytes.
    pub fn raw(&self) -> &[u8] {
        &self.raw
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::test_util::section_payload;

    const SETTINGS_188: &str = include_str!("../../tests/vectors/settings_188.hex");

    fn captured() -> Vec<u8> {
        let p = section_payload(SETTINGS_188);
        assert_eq!(p.len(), 180);
        p
    }

    // ---- Hardening: RAM overlay at 0x10..0x14 (device session 1) ----

    #[test]
    fn decode_block_a_crc_ignores_the_boot_time_overlay() {
        // The captured block has 0xFFFFFFFF at 0x10; after a reboot the meter
        // shows its boot time there while the stored CRC is unchanged.
        let mut p = captured();
        assert_eq!(&p[0x10..0x14], &[0xFF; 4]);
        p[0x10..0x14].copy_from_slice(&0x6AD7_84A2u32.to_le_bytes());
        let s = Settings::decode(&p).unwrap();
        assert!(s.block_a_crc_ok());
        assert_eq!(&s.raw()[0x10..0x14], &0x6AD7_84A2u32.to_le_bytes());
    }

    #[test]
    fn decode_block_a_real_corruption_still_fails_with_overlay_present() {
        let mut p = captured();
        p[0x10..0x14].copy_from_slice(&0x6AD7_84A2u32.to_le_bytes());
        p[0x20] ^= 0x01;
        assert!(!Settings::decode(&p).unwrap().block_a_crc_ok());
    }

    // ---- AC-E88WR5.6: CRCs, rate code, name, raw bytes ----

    #[test]
    fn decode_captured_settings_passes_both_crcs() {
        let p = captured();
        let s = Settings::decode(&p).unwrap();
        assert!(s.block_a_crc_ok());
        assert!(s.block_b_crc_ok());
        assert_eq!(s.rate_code(), 2);
        assert_eq!(s.name(), "POWER-Z");
        assert_eq!(s.raw(), &p[..]);
    }

    #[test]
    fn decode_corrupt_block_a_fails_only_crc_a() {
        let mut p = captured();
        p[0x30] ^= 0x01;
        let s = Settings::decode(&p).unwrap();
        assert!(!s.block_a_crc_ok());
        assert!(s.block_b_crc_ok());

        let mut p = captured();
        p[0x5C] ^= 0x80; // the stored CRC itself
        let s = Settings::decode(&p).unwrap();
        assert!(!s.block_a_crc_ok());
        assert!(s.block_b_crc_ok());
    }

    #[test]
    fn decode_corrupt_block_b_fails_only_crc_b() {
        let mut p = captured();
        p[0x7A] ^= 0x01; // inside the name area, past the NUL
        let s = Settings::decode(&p).unwrap();
        assert!(s.block_a_crc_ok());
        assert!(!s.block_b_crc_ok());

        let mut p = captured();
        p[0xB3] ^= 0x01;
        let s = Settings::decode(&p).unwrap();
        assert!(s.block_a_crc_ok());
        assert!(!s.block_b_crc_ok());
    }

    #[test]
    fn decode_flags_change_breaks_only_crc_b() {
        // 0x60 flags lie in block B; changing them keeps block A valid.
        let mut p = captured();
        p[0x60] ^= 0x01;
        let s = Settings::decode(&p).unwrap();
        assert!(s.block_a_crc_ok());
        assert!(!s.block_b_crc_ok());
    }

    #[test]
    fn decode_rate_code_reads_bits_2_and_3_of_0x60() {
        let mut p = captured();
        for code in 0..=3u32 {
            let flags = 0xFFFF_FFF3 | (code << 2);
            p[0x60..0x64].copy_from_slice(&flags.to_le_bytes());
            assert_eq!(Settings::decode(&p).unwrap().rate_code(), code as u8);
        }
    }

    #[test]
    fn decode_name_stops_at_nul() {
        let mut p = captured();
        p[0x70..0x80].copy_from_slice(b"ABC\0XYZ\0\0\0\0\0\0\0\0\0");
        assert_eq!(Settings::decode(&p).unwrap().name(), "ABC");
    }

    #[test]
    fn decode_short_settings_is_error() {
        let p = captured();
        for len in [0usize, 4, 96, 179] {
            assert!(
                matches!(
                    Settings::decode(&p[..len]),
                    Err(DecodeError::Length { got, .. }) if got == len
                ),
                "len {len} should be a length error"
            );
        }
    }
}
