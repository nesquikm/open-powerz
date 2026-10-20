//! Synthetic contents of the four memory blocks the meter lets us read.
//!
//! Every identifier here is made up: serial [`SIM_SERIAL`](crate::SIM_SERIAL)
//! and an arbitrary UID pattern, never a real meter's.

/// Hardware / boot-loader info block.
pub(crate) const HARDWARE_INFO: (u32, u32) = (0x0000_0420, 64);
/// Application firmware info block.
pub(crate) const FIRMWARE_INFO: (u32, u32) = (0x0000_4420, 64);
/// OTP block (serial; first 16 bytes are the level-2 credential).
pub(crate) const OTP: (u32, u32) = (0x0300_0C00, 64);
/// MCU unique ID (the level-1 credential).
pub(crate) const UID: (u32, u32) = (0x4001_0450, 12);

/// Made-up 12-byte MCU UID.
pub(crate) const SIM_UID: [u8; 12] = [
    0x5A, 0x17, 0xC3, 0x0D, 0x9E, 0x42, 0x6B, 0xF1, 0x08, 0xA4, 0x3C, 0xE5,
];

/// Bytes of the OTP block used as the level-2 credential.
pub(crate) const OTP_CREDENTIAL_LEN: usize = 16;

/// Write `s` into `block` at `off` (12-byte NUL-padded string field).
fn put_str(block: &mut [u8], off: usize, s: &str) {
    block[off..off + 12].fill(0);
    block[off..off + s.len()].copy_from_slice(s.as_bytes());
}

/// A 64-byte info block with the four string fields at 0x10/0x1C/0x28/0x34.
fn info_block(head: [u32; 3], version: &str, date: &str) -> Vec<u8> {
    let mut b = vec![0xFF; 64];
    for (i, w) in head.iter().enumerate() {
        b[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
    }
    b[12..16].fill(0);
    put_str(&mut b, 0x10, "KM003C");
    put_str(&mut b, 0x1C, version);
    put_str(&mut b, 0x28, date);
    put_str(&mut b, 0x34, "SIM");
    b
}

/// The synthetic OTP block: `SERIAL <hex id> <time> SIM`, 0xFF padded.
fn otp_block() -> Vec<u8> {
    let text = format!(
        "{} 0123456789abcdef0123456789abcdef 1700000000 SIM",
        crate::SIM_SERIAL
    );
    let mut b = vec![0xFF; 64];
    b[..text.len()].copy_from_slice(text.as_bytes());
    b
}

/// Plain contents of `(addr, len)`, or `None` for anything not allowed.
pub(crate) fn block(addr: u32, len: u32) -> Option<Vec<u8>> {
    match (addr, len) {
        HARDWARE_INFO => Some(info_block([0x0000_0000, 0x4000, 0], "2.5", "2024.01.01")),
        FIRMWARE_INFO => Some(info_block(
            [0x4000, 0x0002_0000, 0x1234_5678],
            "2.0.6",
            "2025.06.01",
        )),
        OTP => Some(otp_block()),
        UID => Some(SIM_UID.to_vec()),
        _ => None,
    }
}

/// The level-2 credential: the first 16 bytes of the OTP block.
pub(crate) fn otp_credential() -> Vec<u8> {
    otp_block()[..OTP_CREDENTIAL_LEN].to_vec()
}
