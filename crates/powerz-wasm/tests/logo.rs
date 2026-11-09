//! FR W8SHXB: the startup logo in the browser, against the native session.
//!
//! - AC-W8SHXB.1: `logo_preview` (picture bytes, `.bin` by flag) and
//!   `logo_preview_builtin` are the desktop app's `LogoPreviewView` for the
//!   same input (load, encode, preview numbers, pixels decoded back, base64).
//! - AC-W8SHXB.2: `LogoWrite::preconditions` refuses before any frame exists
//!   when the session is not authenticated, SETTINGS was not read, the model
//!   was not read or is not a KM003C, in the desktop LogoRefused wording.
//! - AC-W8SHXB.3: an upload's chunk frames, its one startup-duration write
//!   (value 2) and its read-back reads are byte for byte the native
//!   `Session::logo_upload`'s; Remove is one 16-byte 0xFF chunk, value 0 and a
//!   16-byte read-back. The read-back is compared byte for byte; a failed
//!   chunk stops the write, nothing is retried; requests outside the logo
//!   paths are refused.
//! - AC-W8SHXB.5: `LogoReader` reads (head, or head plus the whole logo) are
//!   the native `logo_read` / `logo_read_full` reads inside the logo window,
//!   with the desktop `LogoReadView`.
//! - AC-W8SHXB.6: `take_settings` is the desktop `LogoWrittenView` from one
//!   SETTINGS read; `settings_save_check` is the desktop
//!   `LogoSettingsCheckView` (saved / needs_menu_save / calibration_changed).
//!
//! The native side is a `powerz_protocol::session::Session` over the demo sim
//! (which stores logo chunks and serves the logo window), recorded.

mod common;

use std::time::Duration;

use common::*;
use powerz_protocol::decode::settings::{CalibrationSnapshot, MENU_SAVE_STEPS, StartupLogo};
use powerz_protocol::frame::{self, Attribute};
use powerz_protocol::logo::builtin::BuiltinLogo;
use powerz_protocol::logo::{self, LogoBlob, LogoMode};
use powerz_protocol::session::{
    Error as SessionError, LogoStatus, LogoWritten, Session, SettingsSave, logo_flag_request,
};
use powerz_protocol::transport::guard::check_logo_flag_write;
use powerz_sim::{LogoFault, SimClock, SimDevice};
use powerz_wasm::{
    BindError, LogoReader, LogoWrite, decode_memory_read, logo_preview, logo_preview_builtin,
    settings_save_check,
};
use serde_json::{Value, json};

const WINDOW: u32 = 0x9830_0000;
const WINDOW_END: u32 = 0x9832_0000;
const REFUSED_HINT: &str = "Replug the meter so the app reads it again, then retry.";
const CHUNK_HINT: &str =
    "The startup duration was not changed, so the meter keeps its old screen. Try again.";
const UPLOAD_HINT: &str = "Try the upload again; nothing was retried.";
const REMOVE_HINT: &str = "Try Remove again; nothing was retried.";
const REREAD: &str = "The logo was written, but the settings could not be read again.";

type Native = Session<Recorder, FixedClock>;

// ---- helpers ----

/// Which session steps ran before the logo action.
#[derive(Clone, Copy)]
struct Steps {
    auth: bool,
    settings: bool,
    info: bool,
}

const ALL: Steps = Steps {
    auth: true,
    settings: true,
    info: true,
};

/// A connected native session over the demo sim built by `f`, with `steps`.
fn native_with(f: impl FnOnce(SimDevice) -> SimDevice, steps: Steps) -> Native {
    let clock = SimClock::new(0);
    let recorder = Recorder {
        sim: f(demo_sim(&clock)),
        log: Vec::new(),
    };
    let mut s = Session::with_clock(recorder, FixedClock);
    s.connect().expect("CONNECT");
    if steps.auth {
        assert_eq!(s.authenticate(1), Ok(1), "level-1 auth");
    }
    if steps.settings {
        s.read_settings().expect("SETTINGS read");
    }
    if steps.info {
        s.device_info().expect("device info");
    }
    s
}

fn native() -> Native {
    native_with(|d| d, ALL)
}

fn is_settings_read(e: &Exchange) -> bool {
    e.request.first().map(|b| b & 0x7F) == Some(0x0C)
        && frame::get_data_mask(&e.request).map(|a| a.bits()) == Some(Attribute::SETTINGS.bits())
}

/// The last SETTINGS reply the session read.
fn last_settings_reply(s: &Native) -> Vec<u8> {
    s.transport()
        .log
        .iter()
        .rev()
        .find(|e| is_settings_read(e))
        .expect("a SETTINGS read")
        .reply()
}

/// The last SETTINGS reply the session read before log position `from`: the
/// read the native write's precondition used.
fn settings_reply_before(s: &Native, from: usize) -> Vec<u8> {
    s.transport().log[..from]
        .iter()
        .rev()
        .find(|e| is_settings_read(e))
        .expect("a SETTINGS read before the write")
        .reply()
}

/// Exchanges since log position `from` of request type `ty`.
fn since(s: &Native, from: usize, ty: u8) -> Vec<Exchange> {
    s.transport().log[from..]
        .iter()
        .filter(|e| e.request.first().map(|b| b & 0x7F) == Some(ty))
        .cloned()
        .collect()
}

fn settings_since(s: &Native, from: usize) -> Vec<Exchange> {
    s.transport().log[from..]
        .iter()
        .filter(|e| is_settings_read(e))
        .cloned()
        .collect()
}

fn mark(s: &Native) -> usize {
    s.transport().log.len()
}

fn control(ty: u8, id: u8) -> Vec<u8> {
    vec![ty, id, 0x00, 0x00]
}

fn assert_err(err: &BindError, kind: &str, message: &str, hint: Option<&str>) {
    assert_eq!(err.kind, kind, "{err:?}");
    assert_eq!(err.message, message, "{err:?}");
    assert_eq!(err.hint.as_deref(), hint, "{err:?}");
}

/// Standard base64 with padding.
fn b64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for g in bytes.chunks(3) {
        let b = [
            g[0],
            g.get(1).copied().unwrap_or(0),
            g.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(char::from(A[(n >> 18) as usize & 63]));
        out.push(char::from(A[(n >> 12) as usize & 63]));
        out.push(if g.len() > 1 {
            char::from(A[(n >> 6) as usize & 63])
        } else {
            '='
        });
        out.push(if g.len() > 2 {
            char::from(A[n as usize & 63])
        } else {
            '='
        });
    }
    out
}

/// A 24-bit BMP of `w × h` with a gradient, written by hand.
fn bmp(w: u32, h: u32) -> Vec<u8> {
    let row = (w * 3).div_ceil(4) * 4;
    let data = row * h;
    let mut f = Vec::new();
    f.extend(b"BM");
    f.extend((54 + data).to_le_bytes());
    f.extend(0u32.to_le_bytes());
    f.extend(54u32.to_le_bytes());
    f.extend(40u32.to_le_bytes());
    f.extend((w as i32).to_le_bytes());
    f.extend((h as i32).to_le_bytes());
    f.extend(1u16.to_le_bytes());
    f.extend(24u16.to_le_bytes());
    f.extend(0u32.to_le_bytes());
    f.extend(data.to_le_bytes());
    f.extend(2835u32.to_le_bytes());
    f.extend(2835u32.to_le_bytes());
    f.extend(0u32.to_le_bytes());
    f.extend(0u32.to_le_bytes());
    for y in 0..h {
        let mut line = Vec::new();
        for x in 0..w {
            line.extend([(x * 2) as u8, (y * 4) as u8, ((x + y) * 3) as u8]);
        }
        line.resize(row as usize, 0);
        f.extend(line);
    }
    f
}

fn mode_of(alpha: bool) -> LogoMode {
    if alpha {
        LogoMode::Alpha
    } else {
        LogoMode::Opaque
    }
}

fn mode_name(mode: LogoMode) -> &'static str {
    match mode {
        LogoMode::Opaque => "opaque",
        LogoMode::Alpha => "alpha",
    }
}

/// The blob the desktop app's `blob_from_file` makes of `bytes`.
fn desktop_blob(bytes: &[u8], is_bin: bool, alpha: bool) -> LogoBlob {
    let img = if is_bin {
        logo::decode(bytes).expect("a logo file")
    } else {
        logo::load_image_bytes(bytes).expect("a picture")
    };
    logo::encode(&img, mode_of(alpha)).expect("encodes")
}

/// The desktop app's `LogoPreviewView` of `blob`, as JSON.
fn desktop_preview(blob: &LogoBlob) -> Value {
    let p = blob.preview();
    let pixels = logo::decode(&blob.bytes).expect("decodes back");
    json!({
        "width": p.width,
        "height": p.height,
        "mode": mode_name(p.mode),
        "blob_size": p.blob_size,
        "padded_size": p.padded_size,
        "chunks": p.chunks,
        "small_side_warning": p.small_side_warning,
        "rgba": b64(&pixels.px),
    })
}

fn state(save: SettingsSave) -> (&'static str, Value) {
    match save {
        SettingsSave::Saved(d) => ("saved", json!(d.label())),
        SettingsSave::NeedsMenuSave => ("needs_menu_save", Value::Null),
        SettingsSave::CalibrationChanged => ("calibration_changed", Value::Null),
    }
}

/// The desktop app's `LogoWrittenView` of `w`, as JSON.
fn desktop_written(w: &LogoWritten) -> Value {
    let (settings, startup_duration) = state(w.settings_after);
    json!({
        "duration_before": w.duration_before.label(),
        "duration_set": w.duration_set.label(),
        "settings": settings,
        "startup_duration": startup_duration,
    })
}

/// Run the binding's `wr` over the native exchanges since `from` (a whole,
/// successful write): every frame must equal the native one; returns the
/// written view.
fn replay(s: &Native, from: usize, wr: &mut LogoWrite) -> Value {
    let chunks = since(s, from, 0x4A);
    assert_eq!(wr.chunk_count() as usize, chunks.len(), "chunk count");
    for (k, ex) in chunks.iter().enumerate() {
        let k32 = k as u32;
        assert_eq!(
            wr.chunk_request(id_of(ex), k32).expect("chunk frame"),
            ex.request,
            "chunk {k}, byte for byte"
        );
        wr.take_chunk_reply(k32, &ex.reply()).expect("FINISHED");
    }
    let flags = since(s, from, 0x48);
    assert_eq!(flags.len(), 1, "one startup-duration write");
    let flag = &flags[0];
    assert_eq!(
        wr.flag_request(id_of(flag)).expect("flag frame"),
        flag.request
    );
    wr.take_flag_reply(&flag.reply()).expect("flag reply");

    let reads = since(s, from, 0x44);
    let plan = wr.read_plan();
    assert_eq!(plan.len(), reads.len(), "read-back reads");
    for (c, ex) in plan.iter().zip(&reads) {
        assert_eq!(
            wr.read_request(id_of(ex), c.addr, c.len)
                .expect("read frame"),
            ex.request,
            "read at {:#x}",
            c.addr
        );
        let data = decode_memory_read(&ex.reply(), c.addr, c.len).expect("read data");
        wr.take_read(c.addr, &data).expect("read-back matches");
    }
    assert!(wr.read_plan().is_empty(), "every read taken");

    let settings = settings_since(s, from);
    assert_eq!(settings.len(), 1, "one SETTINGS read after the write");
    let ex = &settings[0];
    assert_eq!(
        wr.settings_request(id_of(ex)).expect("settings frame"),
        ex.request
    );
    json(&wr.take_settings(&ex.reply()).expect("written view"))
}

fn ready(wr: &mut LogoWrite, s: &Native) {
    wr.preconditions(
        s.auth_level(),
        Some(&last_settings_reply(s)),
        Some("KM003C"),
    )
    .expect("preconditions hold");
}

/// `ready` with the SETTINGS reply the native write at log position `from`
/// started from (the one read before it, not the post-write check).
fn ready_before(wr: &mut LogoWrite, s: &Native, from: usize) {
    wr.preconditions(
        s.auth_level(),
        Some(&settings_reply_before(s, from)),
        Some("KM003C"),
    )
    .expect("preconditions hold");
}

// ---- AC-W8SHXB.1: previews ----

#[test]
fn a_picture_previews_as_on_the_desktop() {
    for (w, h) in [(100u32, 60u32), (300, 300), (40, 20)] {
        let file = bmp(w, h);
        for alpha in [false, true] {
            let view = logo_preview(&file, false, alpha).expect("previews");
            assert_eq!(
                json(&view),
                desktop_preview(&desktop_blob(&file, false, alpha)),
                "{w}x{h} alpha={alpha}"
            );
        }
    }
}

#[test]
fn a_100x60_picture_has_the_desktop_numbers() {
    let view = json(&logo_preview(&bmp(100, 60), false, false).expect("previews"));
    assert_eq!(view["width"], 100);
    assert_eq!(view["height"], 60);
    assert_eq!(view["mode"], "opaque");
    assert_eq!(view["blob_size"], 16 + 100 * 60 * 2);
    assert_eq!(view["padded_size"], 12_032);
    assert_eq!(view["chunks"], 47);
    assert_eq!(view["small_side_warning"], false);
}

#[test]
fn a_png_previews_as_on_the_desktop() {
    // A built-in logo's PNG, given as a picture file.
    let png = BuiltinLogo::Surf.png();
    for alpha in [false, true] {
        assert_eq!(
            json(&logo_preview(png, false, alpha).expect("previews")),
            desktop_preview(&desktop_blob(png, false, alpha))
        );
    }
}

#[test]
fn a_bin_file_previews_as_on_the_desktop() {
    let raw = desktop_blob(&bmp(64, 48), false, true).bytes;
    for alpha in [false, true] {
        assert_eq!(
            json(&logo_preview(&raw, true, alpha).expect("previews")),
            desktop_preview(&desktop_blob(&raw, true, alpha))
        );
    }
    // The same bytes as a picture are no picture.
    let err = logo_preview(&raw, false, false).unwrap_err();
    assert_eq!(err.kind, "usage", "{err:?}");
}

#[test]
fn a_file_that_is_no_picture_is_a_usage_error() {
    for (bytes, is_bin) in [
        (&b"not a picture"[..], false),
        (&b""[..], false),
        (&b"not a logo"[..], true),
    ] {
        let err = logo_preview(bytes, is_bin, false).unwrap_err();
        assert_eq!(err.kind, "usage", "{err:?}");
        assert!(!err.message.is_empty());
    }
}

#[test]
fn the_built_in_logos_preview_as_on_the_desktop() {
    for b in BuiltinLogo::ALL {
        let view = logo_preview_builtin(b.name()).expect("previews");
        let want = desktop_preview(&b.encode().expect("encodes"));
        assert_eq!(json(&view), want, "{}", b.name());
        assert_eq!(want["mode"], "alpha");
    }
    let err = logo_preview_builtin("nope").unwrap_err();
    assert_eq!(err.kind, "usage", "{err:?}");
    assert_eq!(
        err.message,
        "nope".parse::<BuiltinLogo>().unwrap_err().to_string()
    );
}

// ---- AC-W8SHXB.2: refused before any write ----

/// The desktop wording of a native refusal.
fn refusal_text(e: &SessionError) -> String {
    match e {
        SessionError::LogoRefused(r) => format!("The logo was not written: {r}."),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

fn assert_nothing_written(s: &Native, from: usize) {
    assert!(since(s, from, 0x4A).is_empty(), "no chunk");
    assert!(since(s, from, 0x48).is_empty(), "no SET_DATA");
}

fn assert_refuses_every_frame(wr: &mut LogoWrite) {
    assert!(wr.chunk_request(1, 0).is_err(), "no chunk frame");
    assert!(wr.flag_request(2).is_err(), "no flag frame");
    assert!(wr.read_plan().is_empty(), "no read-back");
}

fn writes() -> Vec<(&'static str, LogoWrite)> {
    vec![
        (
            "upload",
            LogoWrite::upload(&bmp(100, 60), false, false).expect("a picture"),
        ),
        (
            "builtin",
            LogoWrite::upload_builtin("surf").expect("a built-in"),
        ),
        ("remove", LogoWrite::remove()),
    ]
}

#[test]
fn an_unauthenticated_session_is_refused_with_the_desktop_reason() {
    let steps = Steps { auth: false, ..ALL };
    let mut s = native_with(|d| d, steps);
    let from = mark(&s);
    let native = s.logo_remove(&mut |_| {}).unwrap_err();
    assert_nothing_written(&s, from);
    let want = refusal_text(&native);
    assert_eq!(
        want,
        "The logo was not written: the session has not authenticated."
    );
    let reply = last_settings_reply(&s);
    for (_, mut wr) in writes() {
        let err = wr
            .preconditions(0, Some(&reply), Some("KM003C"))
            .unwrap_err();
        assert_err(&err, "usage", &want, Some(REFUSED_HINT));
        assert_refuses_every_frame(&mut wr);
    }
}

#[test]
fn a_session_without_a_settings_read_is_refused_with_the_desktop_reason() {
    let steps = Steps {
        settings: false,
        ..ALL
    };
    let mut s = native_with(|d| d, steps);
    let from = mark(&s);
    let blob = desktop_blob(&bmp(100, 60), false, false);
    let native = s.logo_upload(&blob, &mut |_| {}).unwrap_err();
    assert_nothing_written(&s, from);
    let want = refusal_text(&native);
    assert_eq!(
        want,
        "The logo was not written: SETTINGS was not read in this session."
    );
    for (_, mut wr) in writes() {
        let err = wr.preconditions(1, None, Some("KM003C")).unwrap_err();
        assert_err(&err, "usage", &want, Some(REFUSED_HINT));
        assert_refuses_every_frame(&mut wr);
    }
}

#[test]
fn a_session_without_the_model_is_refused_with_the_desktop_reason() {
    let steps = Steps { info: false, ..ALL };
    let mut s = native_with(|d| d, steps);
    let from = mark(&s);
    let native = s.logo_remove(&mut |_| {}).unwrap_err();
    assert_nothing_written(&s, from);
    let want = refusal_text(&native);
    assert_eq!(
        want,
        "The logo was not written: the device model was not read in this session."
    );
    let reply = last_settings_reply(&s);
    for (_, mut wr) in writes() {
        let err = wr.preconditions(1, Some(&reply), None).unwrap_err();
        assert_err(&err, "usage", &want, Some(REFUSED_HINT));
        assert_refuses_every_frame(&mut wr);
    }
}

#[test]
fn a_meter_that_is_not_a_km003c_is_refused_with_the_desktop_reason() {
    let mut s = native_with(|d| d.with_model("KM002C"), ALL);
    let from = mark(&s);
    let blob = desktop_blob(&bmp(100, 60), false, false);
    let native = s.logo_upload(&blob, &mut |_| {}).unwrap_err();
    assert_nothing_written(&s, from);
    let want = refusal_text(&native);
    assert_eq!(
        want,
        "The logo was not written: the meter is a KM002C, logo upload needs a KM003C."
    );
    let reply = last_settings_reply(&s);
    for (_, mut wr) in writes() {
        let err = wr
            .preconditions(1, Some(&reply), Some("KM002C"))
            .unwrap_err();
        assert_err(&err, "usage", &want, Some(REFUSED_HINT));
        assert_refuses_every_frame(&mut wr);
    }
}

#[test]
fn the_checks_run_in_the_desktop_order() {
    let s = native();
    let reply = last_settings_reply(&s);
    let text = |e: BindError| e.message;
    let mut wr = LogoWrite::remove();
    // Authentication first, then SETTINGS, then the model.
    assert!(
        text(wr.preconditions(0, None, None).unwrap_err()).contains("authenticated"),
        "auth first"
    );
    assert!(
        text(wr.preconditions(1, None, None).unwrap_err()).contains("SETTINGS"),
        "settings second"
    );
    assert!(
        text(
            wr.preconditions(1, Some(&reply), Some("KM001C"))
                .unwrap_err()
        )
        .contains("KM001C")
    );
    // The model matches as the native check does: trimmed, any case.
    let mut ok = LogoWrite::remove();
    ok.preconditions(1, Some(&reply), Some(" km003c "))
        .expect("a KM003C in other case");
    assert_eq!(ok.chunk_count(), 1);
}

#[test]
fn no_frame_exists_before_the_preconditions() {
    for (what, mut wr) in writes() {
        assert!(wr.chunk_request(1, 0).is_err(), "{what}");
        assert!(wr.flag_request(1).is_err(), "{what}");
        assert!(wr.settings_request(1).is_err(), "{what}");
    }
}

#[test]
fn a_bad_source_is_a_usage_error() {
    for err in [
        LogoWrite::upload(b"not a picture", false, false).unwrap_err(),
        LogoWrite::upload(b"not a logo", true, false).unwrap_err(),
        LogoWrite::upload_builtin("nope").unwrap_err(),
    ] {
        assert_eq!(err.kind, "usage", "{err:?}");
        assert!(!err.message.is_empty());
    }
}

// ---- AC-W8SHXB.3 / .6: the upload against the native one ----

fn upload_parity(file: &[u8], is_bin: bool, alpha: bool) {
    let mut s = native();
    let blob = desktop_blob(file, is_bin, alpha);
    let from = mark(&s);
    let written = s.logo_upload(&blob, &mut |_| {}).expect("native upload");
    let mut wr = LogoWrite::upload(file, is_bin, alpha).expect("a picture");
    ready_before(&mut wr, &s, from);
    assert_eq!(wr.chunk_count() as usize, blob.padded().len() / 256);
    let view = replay(&s, from, &mut wr);
    assert_eq!(view, desktop_written(&written));
    assert_eq!(view["duration_set"], "3 s");
}

#[test]
fn a_picture_upload_sends_the_native_frames() {
    upload_parity(&bmp(100, 60), false, false);
}

#[test]
fn a_transparent_picture_upload_sends_the_native_frames() {
    upload_parity(&bmp(64, 48), false, true);
}

#[test]
fn a_bin_upload_sends_the_native_frames() {
    let raw = desktop_blob(&bmp(48, 48), false, false).bytes;
    upload_parity(&raw, true, false);
}

#[test]
fn a_built_in_upload_sends_the_native_frames() {
    for b in BuiltinLogo::ALL {
        let mut s = native();
        let blob = b.encode().expect("encodes");
        let from = mark(&s);
        let written = s.logo_upload(&blob, &mut |_| {}).expect("native upload");
        let mut wr = LogoWrite::upload_builtin(b.name()).expect("a built-in");
        ready_before(&mut wr, &s, from);
        assert_eq!(wr.chunk_count() as usize, blob.padded().len() / 256);
        assert_eq!(replay(&s, from, &mut wr), desktop_written(&written));
    }
}

#[test]
fn the_flag_write_is_value_2_for_an_upload_and_0_for_remove() {
    let s = native();
    let mut up = LogoWrite::upload(&bmp(48, 48), false, false).expect("a picture");
    ready(&mut up, &s);
    for k in 0..up.chunk_count() {
        up.chunk_request(10, k).expect("chunk");
        up.take_chunk_reply(k, &control(0x07, 10))
            .expect("FINISHED");
    }
    let f = up.flag_request(11).expect("flag");
    assert_eq!(f, logo_flag_request(11, 2));
    assert_eq!(check_logo_flag_write(&f), Ok(()));

    let mut rm = LogoWrite::remove();
    ready(&mut rm, &s);
    rm.chunk_request(12, 0).expect("chunk");
    rm.take_chunk_reply(0, &control(0x07, 12))
        .expect("FINISHED");
    let f = rm.flag_request(13).expect("flag");
    assert_eq!(f, logo_flag_request(13, 0));
    assert_eq!(check_logo_flag_write(&f), Ok(()));
}

#[test]
fn a_remove_sends_the_native_frames() {
    let mut s = native_with(
        |d| d.with_logo_bytes(&desktop_blob(&bmp(64, 48), false, false).padded()),
        ALL,
    );
    let from = mark(&s);
    let written = s.logo_remove(&mut |_| {}).expect("native remove");
    let mut wr = LogoWrite::remove();
    ready_before(&mut wr, &s, from);
    assert_eq!(wr.chunk_count(), 1);
    let chunks = since(&s, from, 0x4A);
    assert_eq!(chunks.len(), 1);
    let reads = since(&s, from, 0x44);
    assert_eq!(reads.len(), 1, "one 16-byte read-back");
    let view = replay(&s, from, &mut wr);
    assert_eq!(view, desktop_written(&written));
    assert_eq!(view["duration_set"], "off");
}

#[test]
fn the_read_back_plan_covers_the_padded_blob_from_the_window_start() {
    let s = native();
    let file = bmp(100, 60);
    let padded = desktop_blob(&file, false, false).padded().len() as u32;
    let mut wr = LogoWrite::upload(&file, false, false).expect("a picture");
    ready(&mut wr, &s);
    assert!(wr.read_plan().is_empty(), "no read-back before the writes");
    for k in 0..wr.chunk_count() {
        wr.chunk_request(1, k).expect("chunk");
        wr.take_chunk_reply(k, &control(0x07, 1)).expect("FINISHED");
    }
    assert!(wr.read_plan().is_empty(), "no read-back before the flag");
    wr.flag_request(2).expect("flag");
    wr.take_flag_reply(&control(0x05, 2))
        .expect("ACCEPT counts");
    let plan = wr.read_plan();
    let mut at = WINDOW;
    for c in &plan {
        assert_eq!(c.addr, at);
        assert!(c.len > 0 && c.len <= 256, "{c:?}");
        at += c.len;
    }
    assert_eq!(at, WINDOW + padded);
    assert!(at <= WINDOW_END);
}

// ---- AC-W8SHXB.3: failures stop the write, nothing is retried ----

#[test]
fn a_rejected_chunk_stops_the_upload_with_the_desktop_wording() {
    let mut s = native_with(|d| d.with_logo_fault(LogoFault::RejectAt(2)), ALL);
    let file = bmp(100, 60);
    let from = mark(&s);
    let native = s
        .logo_upload(&desktop_blob(&file, false, false), &mut |_| {})
        .unwrap_err();
    assert!(
        matches!(native, SessionError::LogoChunk { index: 2, .. }),
        "{native:?}"
    );
    let chunks = since(&s, from, 0x4A);
    assert_eq!(chunks.len(), 3, "no retry");
    let mut wr = LogoWrite::upload(&file, false, false).expect("a picture");
    ready(&mut wr, &s);
    for (k, ex) in chunks.iter().enumerate() {
        let k32 = k as u32;
        assert_eq!(wr.chunk_request(id_of(ex), k32).expect("chunk"), ex.request);
        let r = wr.take_chunk_reply(k32, &ex.reply());
        if k < 2 {
            r.expect("FINISHED");
        } else {
            assert_err(
                &r.unwrap_err(),
                "protocol",
                "Writing the logo stopped at chunk 2: REJECT.",
                Some(CHUNK_HINT),
            );
        }
    }
    // Stopped: no further chunk, no flag, no read-back.
    assert!(wr.chunk_request(9, 3).is_err());
    assert!(wr.flag_request(9).is_err());
    assert!(wr.read_plan().is_empty());
}

#[test]
fn a_chunk_answered_with_anything_but_finished_stops_the_upload() {
    let s = native();
    for reply in [control(0x05, 3), control(0x01, 3), vec![], vec![0x41]] {
        let mut wr = LogoWrite::upload(&bmp(48, 48), false, false).expect("a picture");
        ready(&mut wr, &s);
        wr.chunk_request(3, 0).expect("chunk");
        let err = wr.take_chunk_reply(0, &reply).unwrap_err();
        assert_eq!(err.kind, "protocol", "{reply:02x?}: {err:?}");
        assert!(
            err.message
                .starts_with("Writing the logo stopped at chunk 0: "),
            "{err:?}"
        );
        assert_eq!(err.hint.as_deref(), Some(CHUNK_HINT));
        assert!(wr.flag_request(4).is_err());
    }
}

#[test]
fn a_rejected_remove_chunk_names_the_clear_step() {
    let mut s = native_with(|d| d.with_logo_fault(LogoFault::RejectAt(0)), ALL);
    let from = mark(&s);
    let native = s.logo_remove(&mut |_| {}).unwrap_err();
    assert!(
        matches!(native, SessionError::LogoRemove { .. }),
        "{native:?}"
    );
    let ex = since(&s, from, 0x4A).pop().expect("the chunk");
    let mut wr = LogoWrite::remove();
    ready(&mut wr, &s);
    assert_eq!(wr.chunk_request(id_of(&ex), 0).expect("chunk"), ex.request);
    let err = wr.take_chunk_reply(0, &ex.reply()).unwrap_err();
    assert_err(
        &err,
        "protocol",
        "Removing the logo failed at the clear step: REJECT.",
        Some(REMOVE_HINT),
    );
    assert!(wr.flag_request(1).is_err());
}

#[test]
fn a_rejected_flag_write_fails_with_the_desktop_wording() {
    let s = native();
    let mut up = LogoWrite::upload(&bmp(48, 48), false, false).expect("a picture");
    ready(&mut up, &s);
    for k in 0..up.chunk_count() {
        up.chunk_request(1, k).expect("chunk");
        up.take_chunk_reply(k, &control(0x07, 1)).expect("FINISHED");
    }
    up.flag_request(2).expect("flag");
    let err = up.take_flag_reply(&control(0x06, 2)).unwrap_err();
    assert_err(
        &err,
        "protocol",
        "The logo was written but its flag write failed: REJECT.",
        Some(UPLOAD_HINT),
    );
    assert!(
        up.read_plan().is_empty(),
        "no read-back after a failed flag"
    );

    let mut rm = LogoWrite::remove();
    ready(&mut rm, &s);
    rm.chunk_request(1, 0).expect("chunk");
    rm.take_chunk_reply(0, &control(0x07, 1)).expect("FINISHED");
    rm.flag_request(2).expect("flag");
    let err = rm.take_flag_reply(&control(0x06, 2)).unwrap_err();
    assert_err(
        &err,
        "protocol",
        "Removing the logo failed at the startup duration step: REJECT.",
        Some(REMOVE_HINT),
    );
}

/// An upload of `file` driven up to its read-back on FINISHED / ACCEPT.
fn at_read_back(s: &Native, mut wr: LogoWrite) -> LogoWrite {
    ready(&mut wr, s);
    for k in 0..wr.chunk_count() {
        wr.chunk_request(1, k).expect("chunk");
        wr.take_chunk_reply(k, &control(0x07, 1)).expect("FINISHED");
    }
    wr.flag_request(2).expect("flag");
    wr.take_flag_reply(&control(0x07, 2)).expect("FINISHED");
    wr
}

#[test]
fn a_read_back_difference_names_the_offset_in_the_desktop_wording() {
    let s = native();
    let file = bmp(100, 60);
    let padded = desktop_blob(&file, false, false).padded();
    let mut wr = at_read_back(
        &s,
        LogoWrite::upload(&file, false, false).expect("a picture"),
    );
    let plan = wr.read_plan();
    let mut outcome = Ok(());
    for c in &plan {
        let off = (c.addr - WINDOW) as usize;
        let mut data = padded[off..off + c.len as usize].to_vec();
        if off == 256 {
            data[5] ^= 0x01;
        }
        outcome = outcome.and(wr.take_read(c.addr, &data));
    }
    assert_err(
        &outcome.unwrap_err(),
        "protocol",
        "The logo read back from the meter differs from the one written, at offset 0x105.",
        Some(UPLOAD_HINT),
    );
    assert!(wr.settings_request(3).is_err(), "no SETTINGS read after it");
}

#[test]
fn a_remove_read_back_that_is_not_erased_names_the_read_back_step() {
    let s = native();
    let mut wr = at_read_back(&s, LogoWrite::remove());
    let plan = wr.read_plan();
    assert_eq!(plan.len(), 1);
    assert_eq!((plan[0].addr, plan[0].len), (WINDOW, 16));
    let mut data = vec![0xFF; 16];
    data[0] = 0x00;
    let err = wr.take_read(WINDOW, &data).unwrap_err();
    assert_err(
        &err,
        "protocol",
        &format!(
            "Removing the logo failed at the read-back step: expected 16 bytes of 0xFF, read {data:02X?}."
        ),
        Some(REMOVE_HINT),
    );
}

#[test]
fn reads_outside_the_logo_window_are_refused() {
    let s = native();
    let mut wr = at_read_back(&s, LogoWrite::remove());
    for (addr, len) in [
        (WINDOW_END, 16),
        (WINDOW - 16, 16),
        (WINDOW_END - 16, 32),
        (WINDOW, 0),
        (WINDOW, 257),
        (0x0000_0420, 64),
        (0x0030_0000, 16),
    ] {
        let err = wr.read_request(1, addr, len).unwrap_err();
        assert_eq!(err.kind, "forbidden", "{addr:#x}+{len}: {err:?}");
        let mut r = LogoReader::new(true);
        let err = r.read_request(1, addr, len).unwrap_err();
        assert_eq!(err.kind, "forbidden", "reader {addr:#x}+{len}: {err:?}");
    }
}

#[test]
fn chunk_indices_past_the_blob_are_refused() {
    let s = native();
    let mut wr = LogoWrite::upload(&bmp(48, 48), false, false).expect("a picture");
    ready(&mut wr, &s);
    let n = wr.chunk_count();
    assert!(wr.chunk_request(1, n).is_err());
    assert!(wr.chunk_request(1, n + 100).is_err());
    let mut rm = LogoWrite::remove();
    ready(&mut rm, &s);
    assert!(rm.chunk_request(1, 1).is_err());
}

// ---- AC-W8SHXB.6: the settings check ----

#[test]
fn a_write_whose_settings_wait_for_a_menu_save_keeps_its_snapshot() {
    // A meter showing the logo for 1 s: Remove writes 0, and block A keeps
    // its old CRC until a menu save (the sim does what the KM003C does).
    let mut s = native_with(|d| d.with_startup_logo(1), ALL);
    let from = mark(&s);
    let written = s.logo_remove(&mut |_| {}).expect("native remove");
    assert_eq!(written.settings_after, SettingsSave::NeedsMenuSave);
    let mut wr = LogoWrite::remove();
    ready_before(&mut wr, &s, from);
    let view = replay(&s, from, &mut wr);
    assert_eq!(view["settings"], "needs_menu_save");
    assert_eq!(view["startup_duration"], Value::Null);
    assert_eq!(wr.pending_snapshot(), Some(written.calibration.0.to_vec()));
}

#[test]
fn a_write_the_meter_saved_reports_the_duration_read() {
    let mut s = native_with(|d| d.with_auto_menu_save(Duration::ZERO, 1), ALL);
    let file = bmp(64, 48);
    let from = mark(&s);
    let written = s
        .logo_upload(&desktop_blob(&file, false, false), &mut |_| {})
        .expect("native upload");
    assert_eq!(
        written.settings_after,
        SettingsSave::Saved(StartupLogo::Secs1)
    );
    let mut wr = LogoWrite::upload(&file, false, false).expect("a picture");
    ready_before(&mut wr, &s, from);
    let view = replay(&s, from, &mut wr);
    assert_eq!(view["settings"], "saved");
    assert_eq!(view["startup_duration"], "1 s");
    assert_eq!(view, desktop_written(&written));
    assert_eq!(wr.pending_snapshot(), None);
}

#[test]
fn a_failed_settings_re_read_is_the_desktop_reread_error() {
    let s = native();
    let mut wr = at_read_back(&s, LogoWrite::remove());
    wr.take_read(WINDOW, &[0xFF; 16]).expect("erased");
    wr.settings_request(4).expect("settings frame");
    let err = wr.take_settings(&control(0x06, 4)).unwrap_err();
    assert_err(&err, "protocol", REREAD, Some(MENU_SAVE_STEPS));
}

fn check_json(save: SettingsSave) -> Value {
    let (settings, startup_duration) = state(save);
    json!({ "settings": settings, "startup_duration": startup_duration })
}

#[test]
fn the_settings_check_is_the_native_check() {
    // Waiting for a menu save: a meter showing the logo for 1 s, then Remove
    // (value 0) leaves block A's CRC stale.
    let mut s = native_with(|d| d.with_startup_logo(1), ALL);
    let w = s.logo_remove(&mut |_| {}).expect("native remove");
    let native = s.check_settings_saved(&w.calibration).expect("check");
    assert_eq!(native, SettingsSave::NeedsMenuSave);
    let reply = last_settings_reply(&s);
    let view = settings_save_check(&w.calibration.0, &reply).expect("check");
    assert_eq!(json(&view), check_json(native));

    // Saved, and calibration changed, on a meter that saved.
    let mut s = native_with(|d| d.with_auto_menu_save(Duration::ZERO, 2), ALL);
    let w = s.logo_remove(&mut |_| {}).expect("native remove");
    let saved = s.check_settings_saved(&w.calibration).expect("check");
    assert!(matches!(saved, SettingsSave::Saved(_)), "{saved:?}");
    let reply = last_settings_reply(&s);
    assert_eq!(
        json(&settings_save_check(&w.calibration.0, &reply).expect("check")),
        check_json(saved)
    );
    let mut other = w.calibration.0;
    other[3] ^= 0x10;
    let changed = s
        .check_settings_saved(&CalibrationSnapshot(other))
        .expect("check");
    assert_eq!(changed, SettingsSave::CalibrationChanged);
    let reply = last_settings_reply(&s);
    assert_eq!(
        json(&settings_save_check(&other, &reply).expect("check")),
        json!({ "settings": "calibration_changed", "startup_duration": null })
    );
}

#[test]
fn the_settings_check_refuses_a_bad_snapshot_or_reply() {
    let s = native();
    let reply = last_settings_reply(&s);
    assert!(settings_save_check(&[0u8; 12], &reply).is_err());
    assert!(settings_save_check(&[0u8; 64], &control(0x06, 1)).is_err());
}

// ---- AC-W8SHXB.5: reading the current logo ----

/// The binding's reader over the native reads since `from`: the head read,
/// then (for `full`, when the head is a logo) the whole padded logo.
fn read_parity(s: &Native, from: usize, full: bool) -> Value {
    let reads = since(s, from, 0x44);
    let mut r = LogoReader::new(full);
    assert!(r.result().is_err(), "no result before the reads");
    let head = &reads[0];
    let plan = r.read_plan();
    assert_eq!(plan.len(), 1, "one head read");
    assert_eq!((plan[0].addr, plan[0].len), (WINDOW, 16));
    assert_eq!(
        r.read_request(id_of(head), WINDOW, 16).expect("head frame"),
        head.request
    );
    let data = decode_memory_read(&head.reply(), WINDOW, 16).expect("head");
    r.take_read(WINDOW, &data).expect("head taken");
    // The native full read reads the head once more, then the whole logo.
    let rest: Vec<Exchange> = if full {
        reads[2.min(reads.len())..].to_vec()
    } else {
        Vec::new()
    };
    let plan = r.read_plan();
    assert_eq!(plan.len(), rest.len(), "the full read");
    for (c, ex) in plan.iter().zip(&rest) {
        assert!(c.addr >= WINDOW && c.addr + c.len <= WINDOW_END && c.len <= 256);
        assert_eq!(
            r.read_request(id_of(ex), c.addr, c.len).expect("frame"),
            ex.request
        );
        let data = decode_memory_read(&ex.reply(), c.addr, c.len).expect("data");
        r.take_read(c.addr, &data).expect("taken");
    }
    assert!(r.read_plan().is_empty());
    json(&r.result().expect("read view"))
}

#[test]
fn an_erased_logo_area_reads_as_empty() {
    for full in [false, true] {
        let mut s = native();
        let from = mark(&s);
        assert_eq!(s.logo_read(), Ok(LogoStatus::Empty));
        assert_eq!(
            read_parity(&s, from, full),
            json!({ "status": "empty", "width": null, "height": null, "mode": null, "image": null })
        );
        let mut r = LogoReader::new(full);
        r.take_read(WINDOW, &[0xFF; 16]).expect("head");
        assert!(r.read_plan().is_empty(), "no full read of an empty area");
        assert_eq!(json(&r.result().expect("view"))["status"], "empty");
    }
}

#[test]
fn an_unknown_header_reads_as_unrecognised() {
    let mut s = native_with(|d| d.with_logo_bytes(&[0x12; 16]), ALL);
    let from = mark(&s);
    assert_eq!(s.logo_read(), Ok(LogoStatus::Unrecognised));
    assert_eq!(
        read_parity(&s, from, false),
        json!({ "status": "unrecognised", "width": null, "height": null, "mode": null, "image": null })
    );
}

#[test]
fn a_stored_logo_reads_its_size_and_mode() {
    let blob = desktop_blob(&bmp(100, 60), false, true);
    let mut s = native_with(|d| d.with_logo_bytes(&blob.padded()), ALL);
    let from = mark(&s);
    assert_eq!(
        s.logo_read(),
        Ok(LogoStatus::Logo {
            width: 100,
            height: 60,
            mode: LogoMode::Alpha
        })
    );
    assert_eq!(
        read_parity(&s, from, false),
        json!({ "status": "logo", "width": 100, "height": 60, "mode": "alpha", "image": null })
    );
}

#[test]
fn a_full_read_is_the_native_full_read() {
    let blob = desktop_blob(&bmp(100, 60), false, false);
    let mut s = native_with(|d| d.with_logo_bytes(&blob.padded()), ALL);
    let from = mark(&s);
    assert!(matches!(s.logo_read(), Ok(LogoStatus::Logo { .. })));
    let rgba = s.logo_read_full().expect("native full read");
    let view = read_parity(&s, from, true);
    assert_eq!(
        view,
        json!({
            "status": "logo",
            "width": 100,
            "height": 60,
            "mode": "opaque",
            "image": b64(&rgba.px),
        })
    );
    assert_eq!(
        since(&s, from, 0x44).len() - 2,
        blob.padded().len().div_ceil(256),
        "the full read is the padded logo in 256-byte reads"
    );
}
