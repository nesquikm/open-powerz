//! FR W8SHXB: the pure pieces of the logo write the web edition needs from the
//! protocol core, exposed without changing what the desktop sends.
//!
//! - AC-W8SHXB.3: `transport::logo_chunk_frame` is the guarded, encrypted 0x4A
//!   frame `transact_logo_chunk` sends: the guard runs first (a chunk outside
//!   0x300000–0x31D4FF, off the 256-byte step or of another type is refused
//!   before a frame exists), then the payload after the 4-byte header is
//!   AES-encrypted under key 0.
//! - AC-W8SHXB.6: `session::settings_save` is the pure part of
//!   `check_settings_saved`: saved (with the duration), needs a menu save, or
//!   calibration changed, from one SETTINGS block and a calibration snapshot.

use std::time::Duration;

use powerz_protocol::crypto::{self, KEYS};
use powerz_protocol::decode::settings::{CalibrationSnapshot, Settings};
use powerz_protocol::logo::{self, LogoMode, Rgba};
use powerz_protocol::session::{SettingsSave, logo_chunk_plaintext, settings_save};
use powerz_protocol::transport::guard::{LOGO_REGION_END, LOGO_REGION_START};
use powerz_protocol::transport::{Error as TError, Transport, logo_chunk_frame};

const BASE: u32 = LOGO_REGION_START;

fn image(w: u32, h: u32) -> Rgba {
    let mut px = Vec::new();
    for y in 0..h {
        for x in 0..w {
            px.extend([(x * 5) as u8, (y * 3) as u8, ((x + y) * 2) as u8, 255]);
        }
    }
    Rgba { w, h, px }
}

fn padded_blob() -> Vec<u8> {
    logo::encode(&image(64, 48), LogoMode::Opaque)
        .expect("encodes")
        .padded()
}

/// Header kept, payload encrypted under key 0.
fn expected_frame(plain: &[u8]) -> Vec<u8> {
    let mut f = plain[..4].to_vec();
    f.extend(crypto::encrypt(&KEYS[0], &plain[4..]));
    f
}

// ---- AC-W8SHXB.3: the chunk frame ----

#[test]
fn w8shxb_logo_chunk_frame_encrypts_the_payload_after_the_header() {
    let padded = padded_blob();
    for (k, data) in padded.chunks(256).enumerate() {
        let plain = logo_chunk_plaintext(k as u8 + 1, BASE + (k * 256) as u32, data);
        let frame = logo_chunk_frame(&plain).expect("a chunk inside the region");
        assert_eq!(frame, expected_frame(&plain), "chunk {k}");
        assert_eq!(frame.len(), 4 + 16 + 256, "chunk {k}");
    }
    // Remove: 16 bytes of 0xFF at the region start.
    let plain = logo_chunk_plaintext(9, BASE, &[0xFF; 16]);
    assert_eq!(logo_chunk_frame(&plain), Ok(expected_frame(&plain)));
}

#[test]
fn w8shxb_logo_chunk_frame_refuses_before_a_frame_exists() {
    let padded = padded_blob();
    let data = &padded[256..512];
    for addr in [
        LOGO_REGION_END,
        LOGO_REGION_END + 256,
        BASE - 256,
        BASE + 1,
        BASE + 0x10,
        0x0000_0000,
        0x9830_0000,
    ] {
        let plain = logo_chunk_plaintext(1, addr, data);
        let r = logo_chunk_frame(&plain);
        assert!(
            matches!(r, Err(TError::Forbidden { .. })),
            "{addr:#x}: {r:?}"
        );
    }
    // Another frame type behind the same path.
    let mut plain = logo_chunk_plaintext(1, BASE + 256, data);
    plain[0] = 0x48;
    assert!(matches!(
        logo_chunk_frame(&plain),
        Err(TError::Forbidden { .. })
    ));
    // Nothing but a header.
    assert!(logo_chunk_frame(&[0x4A, 1, 1, 0]).is_err());
    assert!(logo_chunk_frame(&[]).is_err());
}

/// A transport that only records what it was handed.
#[derive(Default)]
struct Wire {
    sent: Vec<Vec<u8>>,
}

impl Transport for Wire {
    fn transact(&mut self, request: &[u8], _timeout: Duration) -> Result<Vec<u8>, TError> {
        self.sent.push(request.to_vec());
        Ok(vec![0x07, request.get(1).copied().unwrap_or(0), 0, 0])
    }
    fn read(&mut self, _timeout: Duration) -> Result<Vec<u8>, TError> {
        Err(TError::Timeout)
    }
}

#[test]
fn w8shxb_transact_logo_chunk_sends_exactly_the_logo_chunk_frame() {
    let padded = padded_blob();
    let mut wire = Wire::default();
    let plain = logo_chunk_plaintext(3, BASE + 512, &padded[512..768]);
    wire.transact_logo_chunk(&plain, Duration::from_millis(1000))
        .expect("sent");
    assert_eq!(wire.sent, vec![logo_chunk_frame(&plain).expect("frame")]);
    // A refused chunk sends nothing.
    let bad = logo_chunk_plaintext(4, LOGO_REGION_END, &padded[..256]);
    assert!(
        wire.transact_logo_chunk(&bad, Duration::from_millis(1000))
            .is_err()
    );
    assert_eq!(wire.sent.len(), 1);
}

// ---- AC-W8SHXB.6: the settings-saved check ----

fn hex(s: &str) -> Vec<u8> {
    let s: String = s
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .flat_map(|l| l.chars())
        .filter(|c| !c.is_whitespace())
        .collect();
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}

/// The 180-byte SETTINGS payload of the recorded 188-byte reply.
fn payload() -> Vec<u8> {
    let reply = hex(include_str!("vectors/settings_188.hex"));
    assert_eq!(reply.len(), 188);
    reply[8..].to_vec()
}

#[test]
fn w8shxb_logo_settings_save_is_saved_with_the_duration_when_both_blocks_hold() {
    let s = Settings::decode(&payload()).expect("decodes");
    assert!(s.block_a_crc_ok() && s.block_b_crc_ok());
    let duration = s.meter_settings().expect("fields").startup_logo.value;
    assert_eq!(
        settings_save(&s, &s.calibration_snapshot()).ok(),
        Some(SettingsSave::Saved(duration))
    );
}

#[test]
fn w8shxb_logo_settings_save_needs_a_menu_save_when_a_block_fails_its_check() {
    let p = payload();
    let snapshot = Settings::decode(&p)
        .expect("decodes")
        .calibration_snapshot();
    // A calibration byte and a CRC byte of block A (the decoder rejects
    // both: the RAM-overlay tolerance does not cover them), then block B's
    // data and CRC.
    for at in [0x20, 0x5C, 0x70, 0xB0] {
        let mut bad = p.clone();
        bad[at] ^= 0x01;
        let s = Settings::decode(&bad).expect("decodes");
        assert!(
            !(s.block_a_crc_ok() && s.block_b_crc_ok()),
            "byte {at:#x} flipped fails a block check"
        );
        assert_eq!(
            settings_save(&s, &snapshot).ok(),
            Some(SettingsSave::NeedsMenuSave),
            "byte {at:#x} flipped"
        );
    }
}

#[test]
fn w8shxb_logo_settings_save_sees_changed_calibration() {
    let s = Settings::decode(&payload()).expect("decodes");
    let mut other = s.calibration_snapshot().0;
    other[7] ^= 0x40;
    assert_eq!(
        settings_save(&s, &CalibrationSnapshot(other)).ok(),
        Some(SettingsSave::CalibrationChanged)
    );
}
