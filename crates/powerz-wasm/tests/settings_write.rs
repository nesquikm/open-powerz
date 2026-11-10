//! FR W8SHX9: changing meter settings in the browser.
//!
//! - AC-W8SHX9.1: `SettingsWrite` asks for SETTINGS twice, then builds one
//!   type-0x0A write, then one more SETTINGS read; two reads that differ refuse
//!   the write (nothing more is built).
//! - AC-W8SHX9.2: the write frame is byte for byte the one the native
//!   `Session::write_settings_checked` sends over the CDC persona; only the six
//!   panel fields (plus the startup duration) can change; an unknown key or an
//!   out-of-range value is refused when the write is made, before any frame.
//! - AC-W8SHX9.3: the read-back check is the native one: a change that did not
//!   take names the field; undecoded bytes that moved come back as the native
//!   warnings.
//! - AC-W8SHX9.6: `cdc_frame_length` is the native CDC framing
//!   (`transport::cdc_frame::frame_complete`).
//! - `DemoMeter::cdc_twin` shares the demo meter's stored settings: a write
//!   there is what the bulk demo meter serves afterwards.
//!
//! The native side is a `Session` over the CDC twin of the desktop demo sim
//! (as the desktop worker opens the settings pipe); the binding is fed the very
//! replies the native session read and must build the very frames it sent.

mod common;

use std::collections::BTreeMap;

use common::*;
use powerz_protocol::decode::settings::{SETTINGS_WRITE_TYPE, WritableField};
use powerz_protocol::session::{Error as SessionError, Session, SettingsWritten};
use powerz_protocol::transport::cdc_frame::frame_complete;
use powerz_sim::{FaultPlan, SimClock};
use powerz_wasm::{
    BindError, DemoMeter, SettingsWrite, cdc_frame_length, connect_request, decode_settings,
    settings_request,
};

/// A native session over the CDC twin of the desktop demo sim with `faults`.
fn native_cdc_session(faults: FaultPlan) -> Session<Recorder, FixedClock> {
    let clock = SimClock::new(0);
    let bulk = demo_sim(&clock).with_faults(faults);
    let recorder = Recorder {
        sim: bulk.cdc_twin(),
        log: Vec::new(),
    };
    Session::with_clock(recorder, FixedClock)
}

fn changes(pairs: &[(&str, i32)]) -> BTreeMap<String, i32> {
    pairs.iter().map(|&(k, v)| (k.to_owned(), v)).collect()
}

/// Native write of `pairs` over the CDC twin: its result and every exchange.
fn native_write(
    faults: FaultPlan,
    pairs: &[(WritableField, i32)],
) -> (Result<SettingsWritten, SessionError>, Vec<Exchange>) {
    let mut s = native_cdc_session(faults);
    let result = s.write_settings_checked(pairs);
    let log = s.transport().log.clone();
    (result, log)
}

fn is_settings_read(e: &Exchange) -> bool {
    e.request == settings_request(id_of(e))
}

fn reply_or_none(e: &Exchange) -> Option<Vec<u8>> {
    let r = e.reply();
    (!r.is_empty()).then_some(r)
}

fn assert_err(err: &BindError, kind: &str, message: &str, hint: Option<&str>) {
    assert_eq!(err.kind, kind, "{err:?}");
    assert_eq!(err.message, message, "{err:?}");
    assert_eq!(err.hint.as_deref(), hint, "{err:?}");
}

// ---- AC-W8SHX9.1 / .2: the frames, byte for byte ----

#[test]
fn the_binding_sends_the_native_read_read_write_read_frames() {
    let (result, log) = native_write(FaultPlan::new(), &[(WritableField::Brightness, 80)]);
    let native = result.expect("the native write takes on the demo sim");
    assert_eq!(log.len(), 4, "read, read, write, read: {log:02x?}");
    assert!(is_settings_read(&log[0]) && is_settings_read(&log[1]) && is_settings_read(&log[3]));
    assert_eq!(log[2].request[0] & 0x7F, SETTINGS_WRITE_TYPE);

    let mut sw = SettingsWrite::from_changes(&changes(&[("brightness", 80)])).unwrap();
    for e in &log[..2] {
        assert_eq!(sw.read_request(id_of(e)), e.request);
        sw.take_read(&e.reply()).expect("a SETTINGS reply");
    }
    let write = sw
        .write_request(id_of(&log[2]))
        .expect("two matching reads");
    assert_eq!(write, log[2].request, "byte for byte the native write");
    sw.take_write_reply(reply_or_none(&log[2]))
        .expect("the write was taken");
    assert_eq!(sw.read_request(id_of(&log[3])), log[3].request);
    sw.take_read(&log[3].reply()).expect("the read-back");

    let view = sw.result().expect("the read-back matches");
    assert_eq!(json(&view.fields), json(&native.settings.rows()));
    assert_eq!(view.warnings, native.warnings());
    let brightness = view.fields.iter().find(|r| r.key == "brightness").unwrap();
    assert!(brightness.value.contains("80"), "{brightness:?}");
}

#[test]
fn every_writable_field_at_once_builds_the_native_frame() {
    let pairs = [
        (WritableField::Language, 0),
        (WritableField::Brightness, 33),
        (WritableField::DimTimeout, 2),
        (WritableField::Theme, 4),
        (WritableField::Rotation, 1),
        (WritableField::SampleRate, 1),
    ];
    let (result, log) = native_write(FaultPlan::new(), &pairs);
    let native = result.expect("the native write takes");
    let named: Vec<(&str, i32)> = pairs.iter().map(|&(f, v)| (f.name(), v)).collect();
    let mut sw = SettingsWrite::from_changes(&changes(&named)).unwrap();
    sw.take_read(&log[0].reply()).unwrap();
    sw.take_read(&log[1].reply()).unwrap();
    // Calibration and auto-rotate go out as read: the frame is the native one.
    assert_eq!(sw.write_request(id_of(&log[2])).unwrap(), log[2].request);
    sw.take_write_reply(reply_or_none(&log[2])).unwrap();
    sw.take_read(&log[3].reply()).unwrap();
    assert_eq!(
        json(&sw.result().unwrap().fields),
        json(&native.settings.rows())
    );
}

#[test]
fn two_reads_that_differ_refuse_the_write() {
    let (result, log) = native_write(
        FaultPlan::new().flaky_settings_read(),
        &[(WritableField::Brightness, 80)],
    );
    assert!(
        matches!(result, Err(SessionError::SettingsReadsDiffer)),
        "{result:?}"
    );
    assert_eq!(log.len(), 2, "two reads and nothing else");

    let mut sw = SettingsWrite::from_changes(&changes(&[("brightness", 80)])).unwrap();
    sw.take_read(&log[0].reply()).unwrap();
    let err = sw
        .take_read(&log[1].reply())
        .expect_err("the second read differs");
    assert_err(
        &err,
        "protocol",
        "Two reads of the meter's settings differ, so nothing was written.",
        Some("Check the cable, then apply again."),
    );
    let refused = sw
        .write_request(7)
        .expect_err("no write after reads that differ");
    assert_eq!(refused.kind, "invalid", "{refused:?}");
}

#[test]
fn the_write_waits_for_two_reads() {
    let (_, log) = native_write(FaultPlan::new(), &[(WritableField::Brightness, 80)]);
    let mut sw = SettingsWrite::from_changes(&changes(&[("brightness", 80)])).unwrap();
    assert_eq!(sw.write_request(1).unwrap_err().kind, "invalid");
    sw.take_read(&log[0].reply()).unwrap();
    assert_eq!(sw.write_request(2).unwrap_err().kind, "invalid");
    assert_eq!(sw.result().unwrap_err().kind, "invalid");
}

// ---- AC-W8SHX9.2: what may change ----

#[test]
fn bad_changes_are_refused_with_the_desktop_wording() {
    let cases: [(&[(&str, i32)], &str); 6] = [
        (&[], "No setting was changed."),
        (
            &[("voltage_cal", 1)],
            "The setting voltage_cal cannot be changed.",
        ),
        (
            &[("auto_rotate", 1)],
            "The setting auto_rotate cannot be changed.",
        ),
        (
            &[("nonsense", 1)],
            "The setting nonsense cannot be changed.",
        ),
        (
            &[("brightness", 0)],
            "brightness = 0 is out of range (1..=100).",
        ),
        (&[("theme", 7)], "theme = 7 is out of range (0..=6)."),
    ];
    for (pairs, message) in cases {
        let err = SettingsWrite::from_changes(&changes(pairs)).expect_err(message);
        assert_err(&err, "usage", message, None);
    }
}

#[test]
fn the_six_panel_fields_and_the_startup_duration_are_writable() {
    for key in [
        "language",
        "brightness",
        "dim_timeout",
        "theme",
        "rotation",
        "sample_rate",
        "startup_duration",
    ] {
        let value = if key == "brightness" { 50 } else { 1 };
        assert!(
            SettingsWrite::from_changes(&changes(&[(key, value)])).is_ok(),
            "{key}"
        );
    }
}

// ---- AC-W8SHX9.3: the read-back ----

#[test]
fn a_change_that_did_not_take_names_the_field() {
    let (result, log) = native_write(
        FaultPlan::new().ignore_settings_write(),
        &[(WritableField::Brightness, 80)],
    );
    let Err(SessionError::SettingsMismatch {
        field,
        expected,
        actual,
    }) = result
    else {
        panic!("the native write should fail the read-back: {result:?}");
    };
    let mut sw = SettingsWrite::from_changes(&changes(&[("brightness", 80)])).unwrap();
    sw.take_read(&log[0].reply()).unwrap();
    sw.take_read(&log[1].reply()).unwrap();
    assert_eq!(sw.write_request(id_of(&log[2])).unwrap(), log[2].request);
    sw.take_write_reply(reply_or_none(&log[2])).unwrap();
    sw.take_read(&log[3].reply()).unwrap();
    let err = sw.result().expect_err("the read-back lacks the change");
    assert_err(
        &err,
        "protocol",
        &format!(
            "The meter's settings do not match after the change: {field} reads {actual}, expected {expected}."
        ),
        Some("Read the settings again; nothing was retried."),
    );
    assert!(err.message.contains("brightness"), "{}", err.message);
}

#[test]
fn moved_undecoded_bytes_are_the_native_warnings() {
    let (result, log) = native_write(
        FaultPlan::new().stir_undecoded_settings(),
        &[(WritableField::Brightness, 80)],
    );
    let native = result.expect("undecoded bytes do not fail the write");
    assert!(!native.warnings().is_empty());
    let mut sw = SettingsWrite::from_changes(&changes(&[("brightness", 80)])).unwrap();
    sw.take_read(&log[0].reply()).unwrap();
    sw.take_read(&log[1].reply()).unwrap();
    sw.write_request(id_of(&log[2])).unwrap();
    sw.take_write_reply(reply_or_none(&log[2])).unwrap();
    sw.take_read(&log[3].reply()).unwrap();
    let view = sw.result().expect("a warning, not a failure");
    assert_eq!(view.warnings, native.warnings());
}

#[test]
fn write_replies_none_accept_and_finished_go_on_a_reject_fails() {
    let (_, log) = native_write(FaultPlan::new(), &[(WritableField::Brightness, 80)]);
    let ready = || {
        let mut sw = SettingsWrite::from_changes(&changes(&[("brightness", 80)])).unwrap();
        sw.take_read(&log[0].reply()).unwrap();
        sw.take_read(&log[1].reply()).unwrap();
        let id = id_of(&log[2]);
        sw.write_request(id).unwrap();
        (sw, id)
    };
    for reply in [None, Some(vec![0x05]), Some(vec![0x07])] {
        let (mut sw, id) = ready();
        let reply = reply.map(|ty| vec![ty[0], id, 0, 0]);
        assert!(sw.take_write_reply(reply.clone()).is_ok(), "{reply:02x?}");
        // The read-back decides after any of them.
        sw.take_read(&log[3].reply()).unwrap();
        assert!(sw.result().is_ok(), "{reply:02x?}");
    }
    let (mut sw, id) = ready();
    let err = sw
        .take_write_reply(Some(vec![0x06, id, 0, 0]))
        .expect_err("a REJECT fails the write");
    assert_err(
        &err,
        "protocol",
        "The meter refused the request.",
        Some("Updating the meter's firmware may help."),
    );
}

// ---- AC-W8SHX9.6: CDC framing ----

#[test]
fn cdc_frame_length_is_the_native_framing() {
    let (_, log) = native_write(FaultPlan::new(), &[(WritableField::Brightness, 80)]);
    let settings = log[0].reply();
    assert!(settings.len() > 8);
    let mut two = settings.clone();
    two.extend_from_slice(&[0x05, 0x09, 0x00, 0x00]);
    let cases: Vec<Vec<u8>> = vec![
        Vec::new(),
        vec![0x05, 0x01],
        vec![0x05, 0x01, 0x00, 0x00],
        settings[..8].to_vec(),
        settings[..settings.len() - 1].to_vec(),
        settings.clone(),
        two,
    ];
    for buf in &cases {
        assert_eq!(
            cdc_frame_length(buf),
            frame_complete(buf).map(|n| n as u32),
            "{} bytes",
            buf.len()
        );
    }
    assert_eq!(cdc_frame_length(&settings), Some(settings.len() as u32));
    assert_eq!(cdc_frame_length(&settings[..settings.len() - 1]), None);
}

// ---- the demo meter's CDC twin ----

#[test]
fn the_demo_cdc_twin_shares_the_settings_with_the_bulk_demo_meter() {
    let mut bulk = DemoMeter::new("load-5v-1a").expect("a known scenario");
    assert_eq!(
        bulk.transact(&connect_request(1)),
        vec![0x05, 0x01, 0x00, 0x00]
    );
    let mut cdc = bulk.cdc_twin();

    let mut sw = SettingsWrite::from_changes(&changes(&[("brightness", 42)])).unwrap();
    sw.take_read(&cdc.transact(&sw.read_request(1))).unwrap();
    sw.take_read(&cdc.transact(&sw.read_request(2))).unwrap();
    let write = sw.write_request(3).unwrap();
    let reply = cdc.transact(&write);
    sw.take_write_reply((!reply.is_empty()).then_some(reply))
        .unwrap();
    cdc.advance(10);
    sw.take_read(&cdc.transact(&sw.read_request(4))).unwrap();
    let written = sw.result().expect("the demo twin takes the write");

    bulk.advance(10);
    let served = decode_settings(&bulk.transact(&settings_request(2))).unwrap();
    assert_eq!(json(&served.fields), json(&written.fields));
    let brightness = served
        .fields
        .iter()
        .find(|r| r.key == "brightness")
        .unwrap();
    assert!(brightness.value.contains("42"), "{brightness:?}");
}
