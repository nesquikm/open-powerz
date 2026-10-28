//! Meter settings, read-only (FR 1HHT01): typed, tagged fields of the
//! 180-byte SETTINGS block, the CRC refusal, and the read-only command set.
//!
//! Field map: research control notes §3.4 (PDM settings dialog, static
//! analysis). The hardware FR's screen check (2026-10-27) confirmed seven
//! fields; rotation (the screen showed 0° with auto-rotate on while the bits
//! read 90°) and the calibration values (not shown on the meter) stay
//! `[inferred]`.

mod common;

use common::*;
use powerz_protocol::crypto;
use powerz_protocol::decode::settings::{
    DimTimeout, Language, MeterSettings, Rotation, StartupLogo, Tag, Theme,
};
use powerz_protocol::decode::{DecodeError, Rate, Settings};
use powerz_protocol::session::{Error, Session};
use powerz_protocol::transport::mock::{MockTransport, Step};

/// The 180-byte payload of the captured FW 2.0.6 block.
fn captured() -> Vec<u8> {
    let frame = vector(SETTINGS_188);
    assert_eq!(frame.len(), 188);
    frame[8..].to_vec()
}

/// Recompute both stored CRCs so a synthetic block is valid.
fn fix_crcs(p: &mut [u8]) {
    let a = crypto::crc32(&p[0x00..0x5C]);
    p[0x5C..0x60].copy_from_slice(&a.to_le_bytes());
    let b = crypto::crc32(&p[0x60..0xB0]);
    p[0xB0..0xB4].copy_from_slice(&b.to_le_bytes());
}

fn word(p: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(p[off..off + 4].try_into().unwrap())
}

fn set_word(p: &mut [u8], off: usize, v: u32) {
    p[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

/// The captured block with `bits` (a `width`-bit field at `shift` of the u32
/// at `off`) replaced by `code`, CRCs fixed.
fn with_bits(off: usize, shift: u32, width: u32, code: u32) -> Vec<u8> {
    let mut p = captured();
    let mask = ((1u32 << width) - 1) << shift;
    let w = (word(&p, off) & !mask) | ((code << shift) & mask);
    set_word(&mut p, off, w);
    fix_crcs(&mut p);
    p
}

fn decode(p: &[u8]) -> MeterSettings {
    MeterSettings::decode(p).unwrap_or_else(|e| panic!("decode failed: {e}"))
}

// ---- AC-1HHT01.1: both CRC-32 blocks checked; a mismatch is reported ----

#[test]
fn ac1_captured_block_passes_both_crcs_and_decodes() {
    let s = Settings::decode(&captured()).unwrap();
    let m = s.meter_settings().expect("valid block decodes");
    assert_eq!(m.language.value, Language::English);
}

#[test]
fn ac1_block_a_crc_mismatch_is_reported_instead_of_decoding() {
    let mut p = captured();
    p[0x00] ^= 0x01; // flips the language bit; block A no longer matches
    let err = MeterSettings::decode(&p).expect_err("bad block A must not decode");
    match &err {
        DecodeError::Crc { what } => assert!(what.contains("block A"), "{what}"),
        other => panic!("expected a CRC error, got {other:?}"),
    }
    assert!(err.to_string().contains("CRC"), "{err}");
    let s = Settings::decode(&p).unwrap();
    assert!(matches!(s.meter_settings(), Err(DecodeError::Crc { .. })));
}

#[test]
fn ac1_block_b_crc_mismatch_is_reported_instead_of_decoding() {
    let mut p = captured();
    p[0x60] ^= 0x01; // rotation bits live in block B
    match MeterSettings::decode(&p) {
        Err(DecodeError::Crc { what }) => assert!(what.contains("block B"), "{what}"),
        other => panic!("expected a block B CRC error, got {other:?}"),
    }
}

#[test]
fn ac1_corrupt_stored_crc_is_a_mismatch() {
    for off in [0x5C, 0xB3] {
        let mut p = captured();
        p[off] ^= 0x80;
        assert!(
            matches!(MeterSettings::decode(&p), Err(DecodeError::Crc { .. })),
            "stored CRC byte {off:#x} corrupted"
        );
    }
}

#[test]
fn ac1_boot_time_overlay_does_not_count_as_a_mismatch() {
    let mut p = captured();
    p[0x10..0x14].copy_from_slice(&0x6AD7_84A2u32.to_le_bytes());
    assert_eq!(decode(&p).brightness.value, 50);
}

#[test]
fn ac1_short_block_is_a_length_error() {
    let p = captured();
    assert!(matches!(
        MeterSettings::decode(&p[..179]),
        Err(DecodeError::Length { got: 179, .. })
    ));
}

// ---- AC-1HHT01.2: field decode, calibration raw, every field tagged ----

#[test]
fn ac2_captured_block_decodes_every_field() {
    let m = decode(&captured());
    assert_eq!(m.language.value, Language::English);
    assert_eq!(m.brightness.value, 50);
    assert_eq!(m.dim_timeout.value, DimTimeout::Off);
    assert_eq!(m.startup_logo.value, StartupLogo::Off);
    assert_eq!(m.theme.value, Theme::Blue);
    assert!(m.auto_rotate.value);
    assert_eq!(m.rotation.value, Rotation::Deg90);
    assert_eq!(m.sample_rate.value, Rate::Sps50);
}

#[test]
fn ac2_calibration_values_are_raw_signed_numbers() {
    let m = decode(&captured());
    assert_eq!(m.voltage_cal.value, [606; 5]);
    assert_eq!(m.current_cal.value, [996_668; 5]);
    assert_eq!(m.offset_cal.value, 80);

    let mut p = captured();
    set_word(&mut p, 0x1C, (-5i32) as u32);
    set_word(&mut p, 0x40, (-1_000_000i32) as u32);
    set_word(&mut p, 0x58, (-80i32) as u32);
    fix_crcs(&mut p);
    let m = decode(&p);
    assert_eq!(m.voltage_cal.value[0], -5);
    assert_eq!(m.current_cal.value[4], -1_000_000);
    assert_eq!(m.offset_cal.value, -80);
}

#[test]
fn ac2_language_bit_0() {
    assert_eq!(
        decode(&with_bits(0x00, 0, 1, 0)).language.value,
        Language::Chinese
    );
    assert_eq!(
        decode(&with_bits(0x00, 0, 1, 1)).language.value,
        Language::English
    );
}

#[test]
fn ac2_brightness_bits_3_to_9_full_range() {
    for b in [1u32, 37, 100] {
        assert_eq!(decode(&with_bits(0x00, 3, 7, b)).brightness.value, b as u8);
    }
    // Out-of-range raw values are reported, not clamped, and never panic.
    for b in [0u32, 101, 127] {
        let m = decode(&with_bits(0x00, 3, 7, b));
        assert_eq!(m.brightness.value, b as u8);
        let row = m
            .rows()
            .into_iter()
            .find(|r| r.key == "brightness")
            .unwrap();
        assert!(row.value.contains(&b.to_string()), "{}", row.value);
    }
    // Neighbouring bits do not leak into the field.
    let m = decode(&with_bits(0x00, 10, 2, 3));
    assert_eq!(m.brightness.value, 50);
}

#[test]
fn ac2_dim_timeout_bits_10_11() {
    let want = [
        DimTimeout::Off,
        DimTimeout::Secs15,
        DimTimeout::Secs30,
        DimTimeout::Secs60,
    ];
    for (code, w) in want.into_iter().enumerate() {
        assert_eq!(
            decode(&with_bits(0x00, 10, 2, code as u32))
                .dim_timeout
                .value,
            w
        );
    }
}

#[test]
fn ac2_startup_logo_bits_14_15() {
    let want = [
        StartupLogo::Off,
        StartupLogo::Secs1,
        StartupLogo::Secs3,
        StartupLogo::Secs5,
    ];
    for (code, w) in want.into_iter().enumerate() {
        assert_eq!(
            decode(&with_bits(0x00, 14, 2, code as u32))
                .startup_logo
                .value,
            w
        );
    }
}

#[test]
fn ac2_theme_bits_16_18_with_unknown_code_reported_raw() {
    let want = [
        Theme::Blue,
        Theme::Green,
        Theme::Pink,
        Theme::Orange,
        Theme::Yellow,
        Theme::DarkGreen,
        Theme::Cyan,
    ];
    for (code, w) in want.into_iter().enumerate() {
        assert_eq!(decode(&with_bits(0x00, 16, 3, code as u32)).theme.value, w);
    }
    let m = decode(&with_bits(0x00, 16, 3, 7));
    assert_eq!(m.theme.value, Theme::Unknown(7));
    let row = m.rows().into_iter().find(|r| r.key == "theme").unwrap();
    assert!(row.value.contains('7'), "{}", row.value);
}

#[test]
fn ac2_auto_rotate_bit_27() {
    assert!(!decode(&with_bits(0x00, 27, 1, 0)).auto_rotate.value);
    assert!(decode(&with_bits(0x00, 27, 1, 1)).auto_rotate.value);
    // Bit 26 is not the auto-rotate bit.
    let mut p = with_bits(0x00, 27, 1, 0);
    let w = word(&p, 0) | (1 << 26);
    set_word(&mut p, 0, w);
    fix_crcs(&mut p);
    assert!(!decode(&p).auto_rotate.value);
}

#[test]
fn ac2_rotation_bits_0_1_of_0x60() {
    let want = [
        Rotation::Deg0,
        Rotation::Deg90,
        Rotation::Deg180,
        Rotation::Deg270,
    ];
    for (code, w) in want.into_iter().enumerate() {
        assert_eq!(
            decode(&with_bits(0x60, 0, 2, code as u32)).rotation.value,
            w
        );
    }
}

#[test]
fn ac2_sample_rate_bits_2_3_of_0x60() {
    for rate in Rate::ALL {
        let m = decode(&with_bits(0x60, 2, 2, u32::from(rate.code())));
        assert_eq!(m.sample_rate.value, rate);
    }
}

/// Keys of the fields the screen check confirmed (2026-10-27, 1HHT04).
const CONFIRMED: [&str; 7] = [
    "language",
    "brightness",
    "dim_timeout",
    "startup_logo",
    "theme",
    "auto_rotate",
    "sample_rate",
];

#[test]
fn ac5_screen_checked_fields_are_confirmed_rotation_and_calibration_stay_inferred() {
    let m = decode(&captured());
    let confirmed = [
        m.language.tag,
        m.brightness.tag,
        m.dim_timeout.tag,
        m.startup_logo.tag,
        m.theme.tag,
        m.auto_rotate.tag,
        m.sample_rate.tag,
    ];
    assert!(
        confirmed.iter().all(|t| *t == Tag::Confirmed),
        "{confirmed:?}"
    );
    let inferred = [
        m.rotation.tag,
        m.voltage_cal.tag,
        m.current_cal.tag,
        m.offset_cal.tag,
    ];
    assert!(inferred.iter().all(|t| *t == Tag::Inferred), "{inferred:?}");
    assert_eq!(Tag::Inferred.as_str(), "inferred");
    assert_eq!(Tag::Confirmed.as_str(), "confirmed");
}

#[test]
fn ac2_rows_list_every_field_with_label_value_and_tag() {
    let rows = decode(&captured()).rows();
    let keys: Vec<&str> = rows.iter().map(|r| r.key).collect();
    assert_eq!(
        keys,
        vec![
            "language",
            "brightness",
            "dim_timeout",
            "startup_logo",
            "theme",
            "auto_rotate",
            "rotation",
            "sample_rate",
            "voltage_cal",
            "current_cal",
            "offset_cal",
        ]
    );
    let value = |k: &str| {
        rows.iter()
            .find(|r| r.key == k)
            .map(|r| r.value.clone())
            .unwrap()
    };
    assert_eq!(value("language"), "English");
    assert_eq!(value("brightness"), "50%");
    assert_eq!(value("dim_timeout"), "off");
    assert_eq!(value("startup_logo"), "off");
    assert_eq!(value("theme"), "blue");
    assert_eq!(value("auto_rotate"), "on");
    assert_eq!(value("rotation"), "90°");
    assert_eq!(value("sample_rate"), "50 SPS");
    assert_eq!(value("voltage_cal"), "606, 606, 606, 606, 606");
    assert_eq!(
        value("current_cal"),
        "996668, 996668, 996668, 996668, 996668"
    );
    assert_eq!(value("offset_cal"), "80");
    for r in &rows {
        assert!(!r.label.is_empty(), "{} has no label", r.key);
        let want = if CONFIRMED.contains(&r.key) {
            Tag::Confirmed
        } else {
            Tag::Inferred
        };
        assert_eq!(r.tag, want, "{}", r.key);
    }
}

#[test]
fn ac2_other_enum_labels() {
    let rows = |p: &[u8]| decode(p).rows();
    let value = |p: &[u8], k: &str| {
        rows(p)
            .into_iter()
            .find(|r| r.key == k)
            .map(|r| r.value)
            .unwrap()
    };
    assert_eq!(value(&with_bits(0x00, 0, 1, 0), "language"), "Chinese");
    assert_eq!(value(&with_bits(0x00, 10, 2, 1), "dim_timeout"), "15 s");
    assert_eq!(value(&with_bits(0x00, 10, 2, 3), "dim_timeout"), "60 s");
    assert_eq!(value(&with_bits(0x00, 14, 2, 3), "startup_logo"), "5 s");
    assert_eq!(value(&with_bits(0x00, 16, 3, 5), "theme"), "dark green");
    assert_eq!(value(&with_bits(0x00, 27, 1, 0), "auto_rotate"), "off");
    assert_eq!(value(&with_bits(0x60, 0, 2, 3), "rotation"), "270°");
    assert_eq!(value(&with_bits(0x60, 2, 2, 3), "sample_rate"), "1000 SPS");
}

#[test]
fn ac2_any_valid_block_decodes_without_panicking() {
    // Exhaustive over every byte value in the two settings words.
    for b in 0..=255u8 {
        for off in [0x00usize, 0x01, 0x02, 0x03, 0x60] {
            let mut p = captured();
            p[off] = b;
            fix_crcs(&mut p);
            let m = decode(&p);
            assert_eq!(m.rows().len(), 11);
        }
    }
}

// ---- AC-1HHT01.6: reading settings sends only GET_DATA 0x008 ----

#[test]
fn ac6_session_read_meter_settings_sends_one_get_data_settings_only() {
    let mut s = Session::open(MockTransport::new(vec![Step::transact(
        get_data(1, 0x008),
        vector(SETTINGS_188),
    )]));
    let m = s.read_meter_settings().unwrap();
    assert_eq!(m.brightness.value, 50);
    assert_eq!(s.transport().written(), &[get_data(1, 0x008)]);
    assert_eq!(s.transport().remaining(), 0);
    for f in s.transport().written() {
        assert_ne!(f[0] & 0x7F, 0x0A, "settings write sent");
        assert_ne!(f[0] & 0x7F, 0x48, "SET_DATA sent");
    }
}

#[test]
fn ac6_session_read_meter_settings_reports_crc_mismatch() {
    let mut frame = vector(SETTINGS_188);
    frame[8 + 0x30] ^= 0x01;
    let mut s = Session::open(MockTransport::new(vec![Step::transact(
        get_data(1, 0x008),
        frame,
    )]));
    assert!(matches!(
        s.read_meter_settings(),
        Err(Error::Decode(DecodeError::Crc { .. }))
    ));
}
