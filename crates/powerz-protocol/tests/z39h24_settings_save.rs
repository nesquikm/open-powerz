//! FR Z39H24: the read-only settings-save check and wait, against scripted
//! `MockTransport` sessions that pin every byte sent.
//!
//! AC-Z39H24.4 the check (saved / needs a menu save / calibration changed,
//! one SETTINGS read); AC-Z39H24.5 the paced wait on the session clock;
//! AC-Z39H24.6 the error helpers and texts that carry the menu-save steps.
//! Every reply is the captured FW 2.0.6 SETTINGS block, edited here and its
//! CRCs refreshed (or deliberately left stale).

mod common;

use std::time::Duration;

use common::*;
use powerz_protocol::crypto;
use powerz_protocol::decode::settings::{CalibrationSnapshot, MENU_SAVE_STEPS, StartupLogo};
use powerz_protocol::decode::{DecodeError, Settings};
use powerz_protocol::session::{
    Error, SETTINGS_SAVE_POLL_GAP, Session, SettingsSave, logo_flag_request,
};
use powerz_protocol::transport::Error as TError;
use powerz_protocol::transport::mock::{MockTransport, Step};

const SETTINGS_MASK: u16 = 0x008;

/// The captured reply frame: 8 header bytes, then the 180-byte block.
fn captured() -> Vec<u8> {
    vector(SETTINGS_188)
}

/// The captured 180-byte block.
fn block() -> Vec<u8> {
    captured()[8..188].to_vec()
}

/// `block` with both CRCs recomputed.
fn with_crcs(mut b: Vec<u8>) -> Vec<u8> {
    let a = crypto::crc32(&b[0x00..0x5C]);
    b[0x5C..0x60].copy_from_slice(&a.to_le_bytes());
    let bb = crypto::crc32(&b[0x60..0xB0]);
    b[0xB0..0xB4].copy_from_slice(&bb.to_le_bytes());
    b
}

/// `block` with startup code `code` in bits 14..15 of the word at 0x00,
/// CRCs refreshed.
fn with_startup(b: Vec<u8>, code: u32) -> Vec<u8> {
    let mut b = b;
    let mut w0 = u32::from_le_bytes(b[0..4].try_into().unwrap());
    w0 = (w0 & !(0b11 << 14)) | ((code & 0b11) << 14);
    b[0..4].copy_from_slice(&w0.to_le_bytes());
    with_crcs(b)
}

/// A valid block at 3 s.
fn saved_3s() -> Vec<u8> {
    with_startup(block(), 2)
}

/// What the meter serves after a 0x1C write: the duration bits moved, but
/// CRC A is the one from before.
fn stale_a(before: &[u8], code: u32) -> Vec<u8> {
    let crc_a = before[0x5C..0x60].to_vec();
    let mut b = with_startup(before.to_vec(), code);
    b[0x5C..0x60].copy_from_slice(&crc_a);
    b
}

/// A block whose block B CRC does not match.
fn bad_b(b: &[u8]) -> Vec<u8> {
    let mut b = b.to_vec();
    b[0xB0] ^= 0x01;
    b
}

/// A valid block whose calibration bytes (0x1C..0x5C) differ from `b`.
fn calibration_moved(b: &[u8]) -> Vec<u8> {
    let mut b = b.to_vec();
    b[0x20] ^= 0x01;
    with_crcs(b)
}

/// The reply frame carrying `block` for request id `id`.
fn reply(id: u8, block: &[u8]) -> Vec<u8> {
    let mut f = with_id(captured()[..8].to_vec(), id);
    f.extend_from_slice(block);
    f
}

/// One scripted SETTINGS read with request id `id`.
fn read_step(id: u8, block: &[u8]) -> Step {
    Step::transact(get_data(id, SETTINGS_MASK), reply(id, block))
}

/// Snapshot of `block`'s calibration bytes, through the library.
fn snapshot_of(block: &[u8]) -> CalibrationSnapshot {
    Settings::decode(block).unwrap().calibration_snapshot()
}

fn assert_only_settings_reads(written: &[Vec<u8>]) {
    for (k, f) in written.iter().enumerate() {
        assert_eq!(
            f,
            &get_data(f[1], SETTINGS_MASK),
            "frame {k} is not a SETTINGS read: {f:02x?}"
        );
    }
}

// ---------------------------------------------------------- AC-Z39H24.4

#[test]
fn z39h24_calibration_snapshot_is_bytes_0x1c_to_0x5c() {
    let b = block();
    let snap = snapshot_of(&b);
    assert_eq!(snap.0.len(), 64);
    assert_eq!(&snap.0[..], &b[0x1C..0x5C]);
    assert_eq!(snap, CalibrationSnapshot(b[0x1C..0x5C].try_into().unwrap()));
    // The duration bits are not part of it.
    assert_eq!(snapshot_of(&saved_3s()), snap);
    assert_ne!(snapshot_of(&calibration_moved(&b)), snap);
}

#[test]
fn z39h24_check_reports_saved_with_the_fresh_duration() {
    let before = block();
    let mut s = Session::open(MockTransport::new(vec![read_step(1, &saved_3s())]));
    let r = s.check_settings_saved(&snapshot_of(&before));
    assert_eq!(r, Ok(SettingsSave::Saved(StartupLogo::Secs3)));
    assert_eq!(s.transport().written(), &[get_data(1, SETTINGS_MASK)]);
    assert_eq!(s.transport().remaining(), 0);
}

#[test]
fn z39h24_check_reports_needs_menu_save_for_a_stale_block_a() {
    let before = block();
    let mut s = Session::open(MockTransport::new(vec![read_step(1, &stale_a(&before, 2))]));
    let r = s.check_settings_saved(&snapshot_of(&before));
    assert_eq!(r, Ok(SettingsSave::NeedsMenuSave));
    assert_eq!(s.transport().written(), &[get_data(1, SETTINGS_MASK)]);
}

#[test]
fn z39h24_check_reports_needs_menu_save_for_a_bad_block_b() {
    let before = block();
    let mut s = Session::open(MockTransport::new(vec![read_step(1, &bad_b(&saved_3s()))]));
    let r = s.check_settings_saved(&snapshot_of(&before));
    assert_eq!(r, Ok(SettingsSave::NeedsMenuSave));
    assert_eq!(s.transport().written().len(), 1);
}

#[test]
fn z39h24_check_reports_calibration_changed_when_valid_blocks_differ() {
    let before = block();
    let after = calibration_moved(&saved_3s());
    let mut s = Session::open(MockTransport::new(vec![read_step(1, &after)]));
    let r = s.check_settings_saved(&snapshot_of(&before));
    assert_eq!(r, Ok(SettingsSave::CalibrationChanged));
    assert_eq!(s.transport().written(), &[get_data(1, SETTINGS_MASK)]);
}

#[test]
fn z39h24_check_passes_a_transport_failure_through() {
    let mut s = Session::open(MockTransport::new(vec![Step::transact_err(
        get_data(1, SETTINGS_MASK),
        TError::Timeout,
    )]));
    let r = s.check_settings_saved(&snapshot_of(&block()));
    assert_eq!(r, Err(Error::Transport(TError::Timeout)));
}

// ---------------------------------------------------------- AC-Z39H24.5

#[test]
fn z39h24_poll_gap_is_two_seconds() {
    assert_eq!(SETTINGS_SAVE_POLL_GAP, Duration::from_secs(2));
}

/// Session over `steps` on a fake clock starting at `T0_MS`.
const T0_MS: u64 = 1_790_000_000_000;

fn timed(steps: Vec<Step>) -> (Session<MockTransport, FakeClock>, FakeClock) {
    let clock = FakeClock::new(T0_MS);
    (
        Session::with_clock(MockTransport::new(steps), clock.clone()),
        clock,
    )
}

#[test]
fn z39h24_wait_checks_until_saved_at_least_2_s_apart() {
    let before = block();
    let stale = stale_a(&before, 2);
    let (mut s, clock) = timed(vec![
        read_step(1, &stale),
        read_step(2, &stale),
        read_step(3, &saved_3s()),
    ]);
    let mut seen = Vec::new();
    let mut at = Vec::new();
    let c2 = clock.clone();
    let r = s.wait_settings_saved(
        &snapshot_of(&before),
        Duration::from_secs(300),
        &mut |v: &SettingsSave| {
            seen.push(*v);
            at.push(c2.0.borrow().now_ms);
            true
        },
    );
    assert_eq!(r, Ok(SettingsSave::Saved(StartupLogo::Secs3)));
    assert_eq!(s.transport().remaining(), 0, "three checks");
    assert_only_settings_reads(s.transport().written());
    assert_eq!(
        seen,
        vec![
            SettingsSave::NeedsMenuSave,
            SettingsSave::NeedsMenuSave,
            SettingsSave::Saved(StartupLogo::Secs3)
        ]
    );
    for pair in at.windows(2) {
        assert!(pair[1] >= pair[0] + 2000, "checks {at:?} are 2 s apart");
    }
    assert!(clock.total_slept() >= Duration::from_secs(4));
}

#[test]
fn z39h24_wait_returns_calibration_changed_at_once() {
    let before = block();
    let (mut s, _clock) = timed(vec![
        read_step(1, &stale_a(&before, 2)),
        read_step(2, &calibration_moved(&saved_3s())),
        read_step(3, &saved_3s()),
    ]);
    let r = s.wait_settings_saved(&snapshot_of(&before), Duration::from_secs(300), &mut |_| {
        true
    });
    assert_eq!(r, Ok(SettingsSave::CalibrationChanged));
    assert_eq!(s.transport().remaining(), 1, "no check after it");
}

#[test]
fn z39h24_wait_gives_up_when_the_timeout_passes() {
    let before = block();
    let stale = stale_a(&before, 2);
    let steps = (1..=20u8).map(|id| read_step(id, &stale)).collect();
    let (mut s, clock) = timed(steps);
    let r = s.wait_settings_saved(&snapshot_of(&before), Duration::from_secs(5), &mut |_| true);
    assert_eq!(r, Ok(SettingsSave::NeedsMenuSave));
    let checks = s.transport().written().len();
    assert!(
        (3..=4).contains(&checks),
        "a 5 s wait at 2 s gaps makes 3 or 4 checks, made {checks}"
    );
    assert_only_settings_reads(s.transport().written());
    let slept = clock.total_slept();
    assert!(
        slept >= Duration::from_secs(4) && slept <= Duration::from_secs(7),
        "slept {slept:?}"
    );
}

#[test]
fn z39h24_wait_with_timeout_0_checks_exactly_once() {
    let before = block();
    let stale = stale_a(&before, 2);
    let (mut s, clock) = timed(vec![read_step(1, &stale), read_step(2, &stale)]);
    let r = s.wait_settings_saved(&snapshot_of(&before), Duration::ZERO, &mut |_| true);
    assert_eq!(r, Ok(SettingsSave::NeedsMenuSave));
    assert_eq!(s.transport().written(), &[get_data(1, SETTINGS_MASK)]);
    assert_eq!(clock.total_slept(), Duration::ZERO);
}

#[test]
fn z39h24_wait_stops_when_the_caller_says_so() {
    let before = block();
    let stale = stale_a(&before, 2);
    let (mut s, _clock) = timed(vec![read_step(1, &stale), read_step(2, &saved_3s())]);
    let mut calls = 0;
    let r = s.wait_settings_saved(&snapshot_of(&before), Duration::from_secs(300), &mut |_| {
        calls += 1;
        false
    });
    assert_eq!(r, Ok(SettingsSave::NeedsMenuSave));
    assert_eq!(calls, 1);
    assert_eq!(s.transport().written().len(), 1, "no check after the stop");
}

// ---------------------------------------------------------- AC-Z39H24.6

#[test]
fn z39h24_menu_save_steps_say_what_to_do_on_the_meter() {
    let t = MENU_SAVE_STEPS.to_lowercase();
    assert!(t.contains("settings"), "{MENU_SAVE_STEPS}");
    assert!(t.contains("startup"), "{MENU_SAVE_STEPS}");
    assert!(t.contains("save"), "{MENU_SAVE_STEPS}");
    assert!(t.contains("unplug"), "{MENU_SAVE_STEPS}");
}

#[test]
fn z39h24_is_settings_not_saved_is_true_only_for_a_settings_block_crc() {
    for what in ["SETTINGS block A", "SETTINGS block B"] {
        assert!(Error::Decode(DecodeError::Crc { what }).is_settings_not_saved());
    }
    assert!(
        !Error::Decode(DecodeError::Crc {
            what: "ADCNEW frame"
        })
        .is_settings_not_saved()
    );
    assert!(!Error::Transport(TError::Timeout).is_settings_not_saved());
    assert!(!Error::Protocol("x".into()).is_settings_not_saved());
}

#[test]
fn z39h24_settings_reread_error_says_the_logo_was_written_and_gives_the_steps() {
    let e = Error::LogoSettingsReread {
        reason: "transport: timeout".into(),
    };
    let t = e.to_string();
    assert!(t.to_lowercase().contains("logo"), "{t}");
    assert!(t.to_lowercase().contains("written"), "{t}");
    assert!(t.contains(MENU_SAVE_STEPS), "{t}");
    assert!(t.contains("timeout"), "{t}");
    assert!(!e.is_settings_not_saved());
}

#[test]
fn z39h24_duration_write_frames_keep_their_bytes() {
    // The 0x1C write of 2 (upload) and of 0 (Remove) is unchanged by the
    // rename to "startup duration".
    assert_eq!(
        logo_flag_request(1, 2),
        vec![
            0x48, 0x01, 0xC0, 0x00, 0x1C, 0x00, 0x00, 0x01, 0x02, 0x00, 0x00, 0x00
        ]
    );
    assert_eq!(logo_flag_request(1, 0)[8..], [0x00, 0x00, 0x00, 0x00]);
}
