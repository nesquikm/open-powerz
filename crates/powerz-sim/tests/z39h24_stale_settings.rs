//! FR Z39H24: the 0x1C write sets the startup duration and leaves the
//! SETTINGS block A check value stale until a save in the meter's menu.
//!
//! AC-Z39H24.1 the sim's 0x1C write and the stale CRC A; AC-Z39H24.2 the
//! menu save (by hand and automatic); AC-Z39H24.3 what an upload or Remove
//! reports; AC-Z39H24.4 the check against the sim; AC-Z39H24.5 the paced,
//! read-only wait; AC-Z39H24.6 upload and Remove refused while stale;
//! AC-Z39H24.7 a settings write refused while stale, and nothing ever
//! writes to repair the block.

mod common;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use common::*;
use powerz_protocol::crypto;
use powerz_protocol::decode::settings::{StartupLogo, WritableField};
use powerz_protocol::decode::{DecodeError, Settings};
use powerz_protocol::frame::Attribute;
use powerz_protocol::logo::{self, LogoBlob, LogoMode, Rgba};
use powerz_protocol::session::{Error, LogoWritten, SettingsSave};
use powerz_protocol::transport::{Error as TError, GuardedTransport, Transport};

// ---- raw frames ----

fn le(w: u32) -> [u8; 4] {
    w.to_le_bytes()
}

/// SET_DATA 0x1C with `value`.
fn duration_frame(id: u8, value: u32) -> Vec<u8> {
    let mut f = le(0x48 | (u32::from(id) << 8) | (3 << 22)).to_vec();
    f.extend(le(0x1C | (4 << 22)));
    f.extend(le(value));
    f
}

/// The 180-byte SETTINGS block the meter serves now.
fn served(d: &mut SimDevice, id: u8) -> Vec<u8> {
    let reply = d.transact(&get_data(id, Attribute::SETTINGS), T).unwrap();
    let secs = sections(&reply);
    assert_eq!(secs.len(), 1, "{reply:02x?}");
    assert_eq!(secs[0].1.len(), 180);
    secs[0].1.clone()
}

fn duration_code(block: &[u8]) -> u32 {
    (u32::from_le_bytes(block[0..4].try_into().unwrap()) >> 14) & 0b11
}

fn crc_a_stored(block: &[u8]) -> [u8; 4] {
    block[0x5C..0x60].try_into().unwrap()
}

fn crc_a_ok(block: &[u8]) -> bool {
    crc_a_stored(block) == crypto::crc32(&block[..0x5C]).to_le_bytes()
}

fn crc_b_ok(block: &[u8]) -> bool {
    block[0xB0..0xB4] == crypto::crc32(&block[0x60..0xB0]).to_le_bytes()
}

/// `a` and `b` agree on every byte but the duration bits of byte 1 and CRC A.
fn same_but_duration_and_crc_a(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).enumerate().all(|(i, (x, y))| match i {
            1 => (x & 0x3F) == (y & 0x3F),
            0x5C..=0x5F => true,
            _ => x == y,
        })
}

fn bulk_raw(f: impl FnOnce(SimDevice) -> SimDevice, clock: &SimClock) -> SimDevice {
    raw_connect(f(device(Persona::Bulk, Scenario::Idle, clock)))
}

fn write_duration(d: &mut SimDevice, id: u8, value: u32) {
    let reply = d.transact_logo_flag(&duration_frame(id, value), T).unwrap();
    assert_eq!(ty(&reply), 0x07, "FINISHED: {reply:02x?}");
}

// ---------------------------------------------------------- AC-Z39H24.1

#[test]
fn z39h24_upload_value_sets_3_s_and_keeps_the_old_crc_a() {
    let clock = clock();
    let mut d = bulk_raw(|d| d.with_startup_logo(3), &clock);
    let before = served(&mut d, 1);
    assert_eq!(duration_code(&before), 3);
    assert!(crc_a_ok(&before));
    assert!(!d.settings_crc_a_stale());

    write_duration(&mut d, 2, 2);
    assert_eq!(d.logo_flag(), 2);
    assert!(d.settings_crc_a_stale());
    let after = served(&mut d, 3);
    assert_eq!(duration_code(&after), 2, "code 2 = 3 s");
    assert_eq!(crc_a_stored(&after), crc_a_stored(&before), "CRC A kept");
    assert!(!crc_a_ok(&after), "so block A fails its check");
    assert!(crc_b_ok(&after), "block B is served as before");
    assert!(same_but_duration_and_crc_a(&before, &after));
    assert_eq!(
        Settings::decode(&after).unwrap().meter_settings(),
        Err(DecodeError::Crc {
            what: "SETTINGS block A"
        })
    );
}

#[test]
fn z39h24_remove_value_turns_the_startup_screen_off_and_keeps_the_old_crc_a() {
    let clock = clock();
    let mut d = bulk_raw(|d| d.with_startup_logo(3), &clock);
    let before = served(&mut d, 1);
    write_duration(&mut d, 2, 0);
    assert_eq!(d.logo_flag(), 0);
    let after = served(&mut d, 3);
    assert_eq!(duration_code(&after), 0, "code 0 = off");
    assert_eq!(crc_a_stored(&after), crc_a_stored(&before));
    assert!(!crc_a_ok(&after));
    assert!(Settings::decode(&after).unwrap().meter_settings().is_err());
}

#[test]
fn z39h24_a_second_write_while_stale_keeps_the_first_frozen_crc() {
    let clock = clock();
    let mut d = bulk_raw(|d| d.with_startup_logo(3), &clock);
    let before = served(&mut d, 1);
    write_duration(&mut d, 2, 2);
    write_duration(&mut d, 3, 0);
    let after = served(&mut d, 4);
    assert_eq!(duration_code(&after), 0);
    assert_eq!(crc_a_stored(&after), crc_a_stored(&before));
    assert!(!crc_a_ok(&after));
}

#[test]
fn z39h24_with_logo_flag_only_stores_the_value() {
    let clock = clock();
    let mut d = bulk_raw(|d| d.with_startup_logo(3).with_logo_flag(2), &clock);
    assert_eq!(d.logo_flag(), 2);
    assert!(!d.settings_crc_a_stale());
    let s = served(&mut d, 1);
    assert_eq!(duration_code(&s), 3);
    assert!(crc_a_ok(&s));
}

// ---------------------------------------------------------- AC-Z39H24.2

#[test]
fn z39h24_every_reply_stays_stale_until_the_menu_save() {
    let clock = clock();
    let mut d = bulk_raw(|d| d.with_startup_logo(3), &clock);
    write_duration(&mut d, 1, 2);
    for id in 2..6 {
        clock.advance(Duration::from_secs(30));
        assert!(!crc_a_ok(&served(&mut d, id)), "read {id} is still stale");
    }
    assert!(d.settings_crc_a_stale());
}

#[test]
fn z39h24_menu_save_sets_the_chosen_duration_and_repairs_block_a_only() {
    for (code, want) in [
        (0u8, StartupLogo::Off),
        (1, StartupLogo::Secs1),
        (2, StartupLogo::Secs3),
        (3, StartupLogo::Secs5),
    ] {
        let clock = clock();
        let mut d = bulk_raw(|d| d.with_startup_logo(3), &clock);
        write_duration(&mut d, 1, 2);
        let stale = served(&mut d, 2);
        d.menu_save(code);
        assert!(!d.settings_crc_a_stale(), "code {code}");
        let saved = served(&mut d, 3);
        assert_eq!(duration_code(&saved), u32::from(code));
        assert!(crc_a_ok(&saved), "code {code}: block A valid again");
        assert!(crc_b_ok(&saved));
        assert!(
            same_but_duration_and_crc_a(&stale, &saved),
            "code {code}: every other byte unchanged"
        );
        let m = Settings::decode(&saved).unwrap().meter_settings().unwrap();
        assert_eq!(m.startup_logo.value, want);
        // And it stays saved.
        assert!(crc_a_ok(&served(&mut d, 4)));
    }
}

#[test]
fn z39h24_auto_menu_save_happens_after_the_set_time() {
    let clock = clock();
    let mut d = bulk_raw(
        |d| {
            d.with_startup_logo(3)
                .with_auto_menu_save(Duration::from_secs(10), 1)
        },
        &clock,
    );
    // Nothing to save before a 0x1C write.
    clock.advance(Duration::from_secs(20));
    assert!(crc_a_ok(&served(&mut d, 1)));
    assert_eq!(duration_code(&served(&mut d, 2)), 3);

    write_duration(&mut d, 3, 2);
    clock.advance(Duration::from_secs(9));
    assert!(!crc_a_ok(&served(&mut d, 4)), "9 s after: still stale");
    clock.advance(Duration::from_secs(1));
    let saved = served(&mut d, 5);
    assert!(crc_a_ok(&saved), "10 s after: saved");
    assert_eq!(duration_code(&saved), 1);
    assert!(!d.settings_crc_a_stale());
}

// ---- a recording pipe whose meter stays reachable ----

#[derive(Debug, Clone)]
struct Sent {
    at_ms: u64,
    frame: Vec<u8>,
}

type Log = Rc<RefCell<Vec<Sent>>>;
type Dev = Rc<RefCell<SimDevice>>;

struct Rec {
    dev: Dev,
    clock: SimClock,
    log: Log,
}

impl Transport for Rec {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, TError> {
        self.log.borrow_mut().push(Sent {
            at_ms: self.clock.now_ms(),
            frame: request.to_vec(),
        });
        self.dev.borrow_mut().transact(request, timeout)
    }

    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, TError> {
        self.dev.borrow_mut().read(timeout)
    }
}

type BenchSession = powerz_protocol::session::Session<GuardedTransport<Rec>, SimClock>;

struct Bench {
    s: BenchSession,
    dev: Dev,
    log: Log,
    clock: SimClock,
    mark: usize,
}

impl Bench {
    fn sent(&self) -> Vec<Sent> {
        self.log.borrow()[self.mark..].to_vec()
    }

    fn start(&mut self) {
        self.mark = self.log.borrow().len();
    }
}

fn bench_over(dev: SimDevice, clock: &SimClock) -> Bench {
    let dev: Dev = Rc::new(RefCell::new(dev));
    let log: Log = Rc::default();
    let rec = Rec {
        dev: Rc::clone(&dev),
        clock: clock.clone(),
        log: Rc::clone(&log),
    };
    let s =
        powerz_protocol::session::Session::with_clock(GuardedTransport::new(rec), clock.clone());
    Bench {
        s,
        dev,
        log,
        clock: clock.clone(),
        mark: 0,
    }
}

/// Bulk KM003C at 5 s built by `f`: CONNECT, auth, SETTINGS and model read.
fn ready(f: impl FnOnce(SimDevice) -> SimDevice) -> Bench {
    let clock = clock();
    let dev = f(device(Persona::Bulk, Scenario::Idle, &clock).with_startup_logo(3));
    let mut b = bench_over(dev, &clock);
    b.s.connect().expect("CONNECT");
    assert_eq!(b.s.authenticate(1), Ok(1));
    b.s.read_settings().expect("SETTINGS read");
    b.s.device_info().expect("device info");
    b.start();
    b
}

fn frame_ty(f: &[u8]) -> u8 {
    f[0] & 0x7F
}

fn is_settings_read(f: &[u8]) -> bool {
    f.len() == 4 && f == get_data(f[1], Attribute::SETTINGS).as_slice()
}

fn is_duration_write(f: &[u8]) -> bool {
    frame_ty(f) == 0x48
        && f.len() == 12
        && u32::from_le_bytes(f[4..8].try_into().unwrap()) & 0x7FFF == 0x1C
}

fn settings_reads(sent: &[Sent]) -> Vec<u64> {
    sent.iter()
        .filter(|s| is_settings_read(&s.frame))
        .map(|s| s.at_ms)
        .collect()
}

fn image(w: u32, h: u32) -> Rgba {
    let mut px = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            px.extend([(x * 5) as u8, (y * 3) as u8, ((x + y) * 2) as u8, 255]);
        }
    }
    Rgba { w, h, px }
}

fn blob() -> LogoBlob {
    logo::encode(&image(48, 48), LogoMode::Opaque).unwrap()
}

fn upload(b: &mut Bench) -> Result<LogoWritten, Error> {
    b.s.logo_upload(&blob(), &mut |_| {})
}

fn remove(b: &mut Bench) -> Result<LogoWritten, Error> {
    b.s.logo_remove(&mut |_| {})
}

// ---------------------------------------------------------- AC-Z39H24.3

#[test]
fn z39h24_upload_reports_before_set_and_a_stale_fresh_read() {
    let mut b = ready(|d| d);
    let snap = b.s.read_settings().unwrap().calibration_snapshot();
    b.start();
    let w = upload(&mut b).expect("upload");
    assert_eq!(w.duration_before, StartupLogo::Secs5);
    assert_eq!(w.duration_set, StartupLogo::Secs3);
    assert_eq!(w.settings_after, SettingsSave::NeedsMenuSave);
    assert_eq!(w.calibration, snap);

    // One fresh SETTINGS read, after the duration write and the read-back.
    let sent = b.sent();
    let reads: Vec<usize> = (0..sent.len())
        .filter(|&i| is_settings_read(&sent[i].frame))
        .collect();
    assert_eq!(reads.len(), 1, "one SETTINGS read after the write");
    let write_at = sent.iter().position(|s| is_duration_write(&s.frame));
    assert!(write_at < Some(reads[0]));
    assert_eq!(reads[0], sent.len() - 1, "the read comes last");
}

#[test]
fn z39h24_upload_reports_the_duration_the_fresh_read_finds() {
    // The menu save lands before the re-read: it reports that read, not
    // the duration from before the write nor the one the write set.
    let mut b = ready(|d| d.with_auto_menu_save(Duration::ZERO, 1));
    let w = upload(&mut b).expect("upload");
    assert_eq!(w.duration_before, StartupLogo::Secs5);
    assert_eq!(w.duration_set, StartupLogo::Secs3);
    assert_eq!(w.settings_after, SettingsSave::Saved(StartupLogo::Secs1));
}

#[test]
fn z39h24_remove_reports_off_and_a_stale_fresh_read() {
    let mut b = ready(|d| d);
    let w = remove(&mut b).expect("remove");
    assert_eq!(w.duration_before, StartupLogo::Secs5);
    assert_eq!(w.duration_set, StartupLogo::Off);
    assert_eq!(w.settings_after, SettingsSave::NeedsMenuSave);
    assert_eq!(settings_reads(&b.sent()).len(), 1);

    let mut b = ready(|d| d.with_auto_menu_save(Duration::ZERO, 0));
    let w = remove(&mut b).expect("remove");
    assert_eq!(w.duration_before, StartupLogo::Secs5);
    assert_eq!(w.settings_after, SettingsSave::Saved(StartupLogo::Off));
}

// ---------------------------------------------------------- AC-Z39H24.4

#[test]
fn z39h24_check_against_the_sim_follows_the_menu_save() {
    let mut b = ready(|d| d);
    let w = upload(&mut b).expect("upload");
    b.start();
    assert_eq!(
        b.s.check_settings_saved(&w.calibration),
        Ok(SettingsSave::NeedsMenuSave)
    );
    assert_eq!(settings_reads(&b.sent()).len(), 1);
    assert_eq!(b.sent().len(), 1, "only the SETTINGS read");

    b.dev.borrow_mut().menu_save(2);
    b.start();
    assert_eq!(
        b.s.check_settings_saved(&w.calibration),
        Ok(SettingsSave::Saved(StartupLogo::Secs3))
    );
    assert_eq!(b.sent().len(), 1);
    assert!(is_settings_read(&b.sent()[0].frame));
}

// ---------------------------------------------------------- AC-Z39H24.5

#[test]
fn z39h24_wait_polls_read_only_at_2_s_until_the_menu_save() {
    let mut b = ready(|d| d.with_auto_menu_save(Duration::from_secs(7), 3));
    let w = upload(&mut b).expect("upload");
    assert_eq!(w.settings_after, SettingsSave::NeedsMenuSave);
    b.start();
    let t0 = b.clock.now_ms();
    let mut checks = Vec::new();
    let r =
        b.s.wait_settings_saved(&w.calibration, Duration::from_secs(300), &mut |c| {
            checks.push(*c);
            true
        });
    assert_eq!(r, Ok(SettingsSave::Saved(StartupLogo::Secs5)));
    let sent = b.sent();
    assert!(
        sent.iter().all(|s| is_settings_read(&s.frame)),
        "only SETTINGS reads: {:02x?}",
        sent.iter().map(|s| &s.frame).collect::<Vec<_>>()
    );
    let at = settings_reads(&sent);
    assert!(at.len() >= 3, "it waited for the save: {at:?}");
    assert_eq!(checks.len(), at.len(), "on_check sees every check");
    assert_eq!(
        checks.last(),
        Some(&SettingsSave::Saved(StartupLogo::Secs5))
    );
    for pair in at.windows(2) {
        assert!(pair[1] >= pair[0] + 2000, "checks 2 s apart: {at:?}");
    }
    assert!(b.clock.now_ms() - t0 < 300_000, "stopped at the save");
}

#[test]
fn z39h24_wait_times_out_without_a_menu_save_and_never_writes() {
    let mut b = ready(|d| d);
    let w = upload(&mut b).expect("upload");
    b.start();
    let t0 = b.clock.now_ms();
    let r =
        b.s.wait_settings_saved(&w.calibration, Duration::from_secs(10), &mut |_| true);
    assert_eq!(r, Ok(SettingsSave::NeedsMenuSave));
    let elapsed = b.clock.now_ms() - t0;
    assert!(
        (8_000..=12_000).contains(&elapsed),
        "a 10 s wait took {elapsed} ms"
    );
    let sent = b.sent();
    assert!(sent.iter().all(|s| is_settings_read(&s.frame)));
    assert!((5..=6).contains(&sent.len()), "{} checks", sent.len());
    // Nothing anywhere in the session wrote settings to repair the block.
    let all = b.log.borrow();
    assert!(all.iter().all(|s| frame_ty(&s.frame) != 0x0A));
    assert!(b.dev.borrow().settings_crc_a_stale());
}

#[test]
fn z39h24_wait_stops_when_on_check_returns_false() {
    let mut b = ready(|d| d.with_auto_menu_save(Duration::from_secs(60), 3));
    let w = upload(&mut b).expect("upload");
    b.start();
    let r =
        b.s.wait_settings_saved(&w.calibration, Duration::from_secs(300), &mut |_| false);
    assert_eq!(r, Ok(SettingsSave::NeedsMenuSave));
    assert_eq!(settings_reads(&b.sent()).len(), 1);
}

// ---------------------------------------------------------- AC-Z39H24.6

#[test]
fn z39h24_upload_and_remove_are_refused_while_block_a_is_stale() {
    let mut b = ready(|d| d);
    upload(&mut b).expect("first upload");
    assert!(b.dev.borrow().settings_crc_a_stale());
    for action in ["upload", "remove"] {
        b.start();
        let r = if action == "upload" {
            upload(&mut b)
        } else {
            remove(&mut b)
        };
        let e = r.expect_err(action);
        assert!(e.is_settings_not_saved(), "{action}: {e:?}");
        assert_eq!(
            e,
            Error::Decode(DecodeError::Crc {
                what: "SETTINGS block A"
            }),
            "{action}"
        );
        assert!(b.sent().is_empty(), "{action}: nothing is sent");
    }
    assert_eq!(b.dev.borrow().logo_flag(), 2, "no second 0x1C write");

    // After a menu save and a fresh read the next action goes through.
    b.dev.borrow_mut().menu_save(3);
    b.s.read_settings().expect("SETTINGS read");
    let w = remove(&mut b).expect("remove after the save");
    assert_eq!(w.duration_before, StartupLogo::Secs5);
    assert_eq!(w.duration_set, StartupLogo::Off);
}

#[test]
fn z39h24_a_session_read_of_a_stale_meter_refuses_the_logo_write() {
    // A fresh session that meets a meter left stale by an earlier one.
    let clock = clock();
    let d = device(Persona::Bulk, Scenario::Idle, &clock).with_startup_logo(3);
    let mut b = bench_over(d, &clock);
    b.s.connect().expect("CONNECT");
    // The earlier write, straight to the meter.
    write_duration(&mut b.dev.borrow_mut(), 0x30, 2);
    assert_eq!(b.s.authenticate(1), Ok(1));
    b.s.read_settings().expect("a stale block still reads");
    b.s.device_info().expect("device info");
    b.start();
    let e = upload(&mut b).expect_err("refused");
    assert!(e.is_settings_not_saved(), "{e:?}");
    assert!(b.sent().is_empty());
}

// ---------------------------------------------------------- AC-Z39H24.7

#[test]
fn z39h24_a_settings_write_while_stale_sends_no_0x0a_frame() {
    let clock = clock();
    let mut d = device(Persona::Cdc, Scenario::Idle, &clock).with_startup_logo(3);
    d.transact_logo_flag(&duration_frame(0x30, 2), T).unwrap();
    assert!(d.settings_crc_a_stale());
    let mut b = bench_over(d, &clock);
    for changes in [
        vec![(WritableField::Brightness, 80)],
        vec![(WritableField::StartupDuration, 3)],
    ] {
        let e =
            b.s.write_settings_checked(&changes)
                .expect_err("refused while stale");
        assert!(e.is_settings_not_saved(), "{e:?}");
        let e =
            b.s.write_settings(&changes)
                .expect_err("refused while stale");
        assert!(e.is_settings_not_saved(), "{e:?}");
    }
    let log = b.log.borrow();
    assert!(
        log.iter().all(|s| frame_ty(&s.frame) != 0x0A),
        "a settings write was sent"
    );
    assert!(
        b.dev.borrow().settings_crc_a_stale(),
        "the block stays stale"
    );
}
