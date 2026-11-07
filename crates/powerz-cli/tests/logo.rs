//! `powerz logo preview|read|write|remove` (FR YCRQRQ, AC .1–.7) run as the
//! real binary against the `powerz-sim` demo meter. No hardware is touched;
//! every image is drawn here.
//!
//! Test-only environment knobs the CLI honours for `--transport sim` (on
//! top of the ones in `e2e_sim.rs`):
//!
//! - `POWERZ_SIM_LOGO_FAULT=reject@K|no-reply@K|unplug@K`: the simulator's
//!   [`LogoFault`](powerz_sim::LogoFault) for logo chunk K (0-based).
//! - `POWERZ_SIM_MODEL=<model>`: the demo meter reports `<model>`.
//! - `POWERZ_SIM_STARTUP_LOGO=off|1|3|5`: the startup-screen duration in the
//!   demo meter's SETTINGS block (the demo meter's own is `off`).
//! - `POWERZ_SIM_LOGO_FILE=<path>`: the file's bytes are in the simulated
//!   logo flash at 0x300000 when the meter starts.
//! - `POWERZ_SIM_CTRL_C_AFTER_CHUNKS=K`: stand in for Ctrl-C once K logo
//!   chunks have been sent.
//! - `POWERZ_SIM_MENU_SAVE=<off|1|3|5>@<ms>`: the demo meter's user saves
//!   that startup duration in the meter's menu `<ms>` after a logo write
//!   left its settings unsaved (FR Z39H24). Without it nobody saves, so a
//!   logo write ends waiting for the save.
//!
//! The confirmation typed at the prompt is `write` for `logo write` and
//! `remove` for `logo remove`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use assert_cmd::Command;
use assert_cmd::assert::Assert;
use powerz_protocol::logo::{self, LogoBlob, LogoMode, Rgba};

/// A serial no output may show (set through `POWERZ_SIM_SERIAL`).
const SECRET_SERIAL: &str = "SIMSER7Q4Z";
/// The demo meter's synthetic UID, as `powerz info` prints it.
const SIM_UID_HEX: &str = "5a17c30d9e426bf108a43ce5";

const KNOBS: [&str; 11] = [
    "POWERZ_SIM_CLOCK",
    "POWERZ_SIM_FAULT",
    "POWERZ_SIM_CTRL_C_AFTER",
    "POWERZ_SIM_SERIAL",
    "POWERZ_SIM_LOGO_FAULT",
    "POWERZ_SIM_MODEL",
    "POWERZ_SIM_STARTUP_LOGO",
    "POWERZ_SIM_LOGO_FILE",
    "POWERZ_SIM_CTRL_C_AFTER_CHUNKS",
    "POWERZ_SIM_MODE",
    "POWERZ_SIM_MENU_SAVE",
];

fn powerz() -> Command {
    let mut c = Command::cargo_bin("powerz").expect("powerz binary is built");
    for k in KNOBS {
        c.env_remove(k);
    }
    c.env("POWERZ_SIM_SERIAL", SECRET_SERIAL);
    c.env("POWERZ_SIM_CLOCK", "fake");
    c.timeout(Duration::from_secs(60));
    c
}

/// `powerz --transport sim --trace ...`: the trace on stderr shows every
/// frame sent.
fn sim() -> Command {
    let mut c = powerz();
    c.args(["--transport", "sim", "--trace"]);
    c
}

/// `powerz --transport sim ...` without the trace.
fn sim_quiet() -> Command {
    let mut c = powerz();
    c.args(["--transport", "sim"]);
    c
}

fn stdout_of(a: &Assert) -> String {
    String::from_utf8_lossy(&a.get_output().stdout).into_owned()
}

fn stderr_of(a: &Assert) -> String {
    String::from_utf8_lossy(&a.get_output().stderr).into_owned()
}

fn code_of(a: &Assert) -> i32 {
    a.get_output().status.code().unwrap_or(-1)
}

fn fresh_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("powerz-logo-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A `w × h` RGBA gradient with a half-transparent right half.
fn gradient(w: u32, h: u32) -> image::RgbaImage {
    image::RgbaImage::from_fn(w, h, |x, y| {
        let a = if x >= w / 2 { 128 } else { 255 };
        image::Rgba([(x * 7) as u8, (y * 5) as u8, ((x + y) * 3) as u8, a])
    })
}

/// Write a `w × h` gradient PNG into `dir` and return its path.
fn png(dir: &Path, name: &str, w: u32, h: u32) -> PathBuf {
    let path = dir.join(name);
    gradient(w, h).save(&path).unwrap();
    path
}

/// What the library makes of `path` in `mode`.
fn encoded(path: &Path, mode: LogoMode) -> LogoBlob {
    let img = logo::load_file(path).unwrap();
    logo::encode(&img, mode).unwrap()
}

/// The pixels of a PNG file.
fn png_pixels(path: &Path) -> Rgba {
    let img = image::open(path).expect("a readable PNG").into_rgba8();
    Rgba {
        w: img.width(),
        h: img.height(),
        px: img.into_raw(),
    }
}

/// Lines and `\r`-separated progress segments of `text`.
fn segments(text: &str) -> Vec<&str> {
    text.split(['\n', '\r']).collect()
}

/// Trace lines of frames sent whose type byte is `ty` (e.g. "4a").
fn sent_frames<'a>(trace: &'a str, ty: &str) -> Vec<&'a str> {
    trace
        .lines()
        .filter(|l| l.starts_with("> ["))
        .filter(|l| {
            l.split_once("] ")
                .is_some_and(|(_, bytes)| bytes.split(' ').next() == Some(ty))
        })
        .collect()
}

/// No logo chunk (0x4A) and no SET_DATA (0x48) was sent.
fn assert_no_write(trace: &str) {
    assert!(
        sent_frames(trace, "4a").is_empty(),
        "a logo chunk was sent:\n{trace}"
    );
    assert!(
        sent_frames(trace, "48").is_empty(),
        "a SET_DATA was sent:\n{trace}"
    );
}

/// Neither the serial nor the UID shows anywhere in `a`'s output.
fn assert_private(a: &Assert) {
    let all = format!("{}{}", stdout_of(a), stderr_of(a));
    assert!(!all.contains(SECRET_SERIAL), "the serial leaked:\n{all}");
    assert!(
        !all.to_ascii_lowercase().contains(SIM_UID_HEX),
        "the UID leaked:\n{all}"
    );
}

/// The first line of `text` containing `key` (case-insensitive).
fn line_with<'a>(text: &'a str, key: &str) -> &'a str {
    text.lines()
        .find(|l| l.to_ascii_lowercase().contains(&key.to_ascii_lowercase()))
        .unwrap_or_else(|| panic!("no line with {key:?} in:\n{text}"))
}

fn contains_size(text: &str, w: u32, h: u32) -> bool {
    text.contains(&format!("{w}x{h}")) || text.contains(&format!("{w}×{h}"))
}

fn repo_file(rel: &str) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::fs::read_to_string(root.join(rel)).unwrap_or_else(|e| panic!("cannot read {rel}: {e}"))
}

// ------------------------------------------------------------ AC-YCRQRQ.1

#[test]
fn ycrqrq_1_preview_prints_size_mode_blob_padded_and_chunks() {
    let dir = fresh_dir("preview");
    let path = png(&dir, "wide.png", 300, 150);
    let p = encoded(&path, LogoMode::Opaque).preview();
    let a = sim()
        .args(["logo", "preview"])
        .arg(&path)
        .assert()
        .success();
    let out = stdout_of(&a);
    assert!(contains_size(&out, p.width, p.height), "final size: {out}");
    assert!(
        line_with(&out, "mode")
            .to_ascii_lowercase()
            .contains("opaque"),
        "{out}"
    );
    assert!(
        line_with(&out, "blob").contains(&p.blob_size.to_string()),
        "{out}"
    );
    assert!(
        line_with(&out, "padded").contains(&p.padded_size.to_string()),
        "{out}"
    );
    assert!(
        line_with(&out, "chunk").contains(&p.chunks.to_string()),
        "{out}"
    );
    assert!(
        !out.to_ascii_lowercase().contains("warn"),
        "no warning for {}x{}: {out}",
        p.width,
        p.height
    );
    assert_private(&a);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ycrqrq_1_preview_alpha_uses_the_alpha_mode() {
    let dir = fresh_dir("preview-alpha");
    let path = png(&dir, "big.png", 400, 400);
    let p = encoded(&path, LogoMode::Alpha).preview();
    assert_eq!((p.width, p.height), (200, 200), "alpha box");
    let a = sim()
        .args(["logo", "preview", "--alpha"])
        .arg(&path)
        .assert()
        .success();
    let out = stdout_of(&a);
    assert!(contains_size(&out, 200, 200), "{out}");
    assert!(
        line_with(&out, "mode")
            .to_ascii_lowercase()
            .contains("alpha"),
        "{out}"
    );
    assert!(
        line_with(&out, "blob").contains(&p.blob_size.to_string()),
        "{out}"
    );
    assert!(
        line_with(&out, "chunk").contains(&p.chunks.to_string()),
        "{out}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ycrqrq_1_preview_warns_when_a_side_is_below_48() {
    let dir = fresh_dir("preview-thin");
    let path = png(&dir, "thin.png", 400, 20);
    let p = encoded(&path, LogoMode::Opaque).preview();
    assert!(p.small_side_warning, "the test image must end up below 48");
    let a = sim()
        .args(["logo", "preview"])
        .arg(&path)
        .assert()
        .success();
    let all = format!("{}{}", stdout_of(&a), stderr_of(&a));
    let warning = line_with(&all, "warn");
    assert!(
        warning.contains("48"),
        "the warning names the 48-pixel floor: {all}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ycrqrq_1_preview_writes_the_decoded_preview_png() {
    let dir = fresh_dir("preview-out");
    let path = png(&dir, "in.png", 120, 90);
    let out_png = dir.join("out.png");
    let blob = encoded(&path, LogoMode::Opaque);
    sim()
        .args(["logo", "preview"])
        .arg(&path)
        .arg("-o")
        .arg(&out_png)
        .assert()
        .success();
    let expected = logo::decode(&blob.bytes).unwrap();
    let got = png_pixels(&out_png);
    assert_eq!((got.w, got.h), (expected.w, expected.h));
    assert!(got.px == expected.px, "the PNG holds what would be written");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ycrqrq_1_preview_reads_a_raw_bin_file() {
    let dir = fresh_dir("preview-bin");
    let src = png(&dir, "src.png", 64, 48);
    let blob = encoded(&src, LogoMode::Opaque);
    let bin = dir.join("logo.bin");
    std::fs::write(&bin, &blob.bytes).unwrap();
    let a = sim().args(["logo", "preview"]).arg(&bin).assert().success();
    let out = stdout_of(&a);
    assert!(contains_size(&out, 64, 48), "{out}");
    assert!(
        line_with(&out, "blob").contains(&blob.bytes.len().to_string()),
        "{out}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ycrqrq_1_preview_opens_no_device() {
    let dir = fresh_dir("preview-nodev");
    let path = png(&dir, "in.png", 64, 64);
    // On the simulator: not one frame is sent.
    let a = sim()
        .args(["logo", "preview"])
        .arg(&path)
        .assert()
        .success();
    let trace = stderr_of(&a);
    assert!(
        !trace
            .lines()
            .any(|l| l.starts_with("> [") || l.starts_with("< [")),
        "preview sent frames:\n{trace}"
    );
    // On real USB with a meter that does not exist: still fine, because no
    // meter is looked up or opened.
    powerz()
        .args(["--device", "no-such-meter-YCRQRQ", "logo", "preview"])
        .arg(&path)
        .assert()
        .success();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ycrqrq_1_preview_output_snapshot() {
    let dir = fresh_dir("preview-snap");
    png(&dir, "fixed.png", 100, 60);
    let a = sim_quiet()
        .current_dir(&dir)
        .args(["logo", "preview", "fixed.png"])
        .assert()
        .success();
    insta::assert_snapshot!("logo_preview_fixed_100x60", stdout_of(&a));
    let _ = std::fs::remove_dir_all(&dir);
}

// ------------------------------------------------------------ AC-YCRQRQ.2

#[test]
fn ycrqrq_2_read_reports_an_empty_logo_area_and_sends_no_write() {
    let a = sim().args(["logo", "read"]).assert().success();
    let out = stdout_of(&a);
    assert!(out.to_ascii_lowercase().contains("empty"), "{out}");
    assert_no_write(&stderr_of(&a));
    assert_private(&a);
}

#[test]
fn ycrqrq_2_read_reports_size_and_mode_of_a_stored_logo() {
    let dir = fresh_dir("read-logo");
    let src = png(&dir, "src.png", 64, 48);
    let blob = encoded(&src, LogoMode::Alpha);
    let flash = dir.join("flash.bin");
    std::fs::write(&flash, blob.padded()).unwrap();
    let a = sim()
        .env("POWERZ_SIM_LOGO_FILE", &flash)
        .args(["logo", "read"])
        .assert()
        .success();
    let out = stdout_of(&a);
    assert!(contains_size(&out, 64, 48), "{out}");
    assert!(out.to_ascii_lowercase().contains("alpha"), "{out}");
    assert!(!out.to_ascii_lowercase().contains("empty"), "{out}");
    assert_no_write(&stderr_of(&a));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ycrqrq_2_read_reports_unrecognised_data() {
    let dir = fresh_dir("read-junk");
    let flash = dir.join("junk.bin");
    std::fs::write(&flash, [0x5Au8; 256]).unwrap();
    let a = sim()
        .env("POWERZ_SIM_LOGO_FILE", &flash)
        .args(["logo", "read"])
        .assert()
        .success();
    let out = stdout_of(&a);
    assert!(out.to_ascii_lowercase().contains("unrecognised"), "{out}");
    assert_no_write(&stderr_of(&a));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ycrqrq_2_read_with_output_writes_the_whole_logo_as_png() {
    let dir = fresh_dir("read-out");
    let src = png(&dir, "src.png", 120, 90);
    let blob = encoded(&src, LogoMode::Opaque);
    let flash = dir.join("flash.bin");
    std::fs::write(&flash, blob.padded()).unwrap();
    let out_png = dir.join("read.png");
    let a = sim()
        .env("POWERZ_SIM_LOGO_FILE", &flash)
        .args(["logo", "read", "-o"])
        .arg(&out_png)
        .assert()
        .success();
    let expected = logo::decode(&blob.bytes).unwrap();
    let got = png_pixels(&out_png);
    assert_eq!((got.w, got.h), (expected.w, expected.h));
    assert!(got.px == expected.px, "the PNG holds the logo read");
    // The whole padded logo was read: more than the 16-byte header read.
    let trace = stderr_of(&a);
    assert!(
        trace.lines().filter(|l| l.starts_with("> [")).count() > blob.padded().len() / 256,
        "every 256-byte block of the logo is read:\n{trace}"
    );
    assert_no_write(&trace);
    let _ = std::fs::remove_dir_all(&dir);
}

// ------------------------------------------------------------ AC-YCRQRQ.3

#[test]
fn ycrqrq_3_write_without_i_understand_is_refused_and_sends_nothing() {
    let dir = fresh_dir("write-noack");
    let path = png(&dir, "in.png", 64, 48);
    for extra in [&[][..], &["--yes"][..]] {
        let a = sim()
            .args(["logo", "write"])
            .arg(&path)
            .args(extra)
            .write_stdin("write\n")
            .assert()
            .failure();
        assert_eq!(code_of(&a), 2, "usage error, args {extra:?}");
        let err = stderr_of(&a);
        assert!(err.contains("--i-understand"), "{err}");
        assert_no_write(&err);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ycrqrq_3_write_with_a_wrong_confirmation_sends_nothing() {
    let dir = fresh_dir("write-wrong");
    let path = png(&dir, "in.png", 64, 48);
    let a = sim()
        .args(["logo", "write", "--i-understand"])
        .arg(&path)
        .write_stdin("nope\n")
        .assert()
        .failure();
    assert_eq!(code_of(&a), 2);
    assert!(
        stdout_of(&a).contains("write"),
        "the prompt names the word to type"
    );
    assert_no_write(&stderr_of(&a));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ycrqrq_3_write_with_the_typed_confirmation_uploads_and_verifies() {
    let dir = fresh_dir("write-typed");
    let path = png(&dir, "in.png", 64, 48);
    let p = encoded(&path, LogoMode::Opaque).preview();
    let a = sim()
        .env("POWERZ_SIM_MENU_SAVE", "3@0")
        .args(["logo", "write", "--i-understand"])
        .arg(&path)
        .write_stdin("write\n")
        .assert()
        .success();
    let trace = stderr_of(&a);
    assert_eq!(
        sent_frames(&trace, "4a").len(),
        p.chunks,
        "one 0x4A per chunk"
    );
    assert_eq!(sent_frames(&trace, "48").len(), 1, "one flag write");
    assert!(stdout_of(&a).to_ascii_lowercase().contains("verified"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ycrqrq_3_write_prints_progress_on_stderr_and_the_verified_result() {
    let dir = fresh_dir("write-yes");
    let path = png(&dir, "in.png", 64, 48);
    let p = encoded(&path, LogoMode::Opaque).preview();
    let n = p.chunks;
    let a = sim_quiet()
        .env("POWERZ_SIM_MENU_SAVE", "3@0")
        .args(["logo", "write", "--i-understand", "--yes"])
        .arg(&path)
        .assert()
        .success();
    let err = stderr_of(&a);
    for k in 1..=n {
        assert!(
            err.contains(&format!("{k}/{n}")),
            "chunk {k}/{n} progress:\n{err}"
        );
    }
    let segs = segments(&err);
    assert!(
        segs.iter()
            .any(|s| s.to_ascii_lowercase().contains("chunk") && s.contains(&format!("{n}/{n}"))),
        "chunk progress on stderr:\n{err}"
    );
    assert!(
        segs.iter()
            .any(|s| s.to_ascii_lowercase().contains("read") && s.contains(&format!("{n}/{n}"))),
        "read-back progress on stderr:\n{err}"
    );
    let out = stdout_of(&a);
    assert!(
        !out.contains(&format!("1/{n}")),
        "progress stays off stdout:\n{out}"
    );
    assert!(out.to_ascii_lowercase().contains("verified"), "{out}");
    assert!(
        contains_size(&out, p.width, p.height),
        "the result names the logo: {out}"
    );
    assert_private(&a);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ycrqrq_3_write_with_the_startup_screen_off_shows_the_change_and_no_hint() {
    // FR Z39H24: the old "turn it on" hint is gone; the upload itself sets
    // 3 s, and the tool says so before the write.
    let dir = fresh_dir("write-off");
    let path = png(&dir, "in.png", 64, 48);
    let a = sim_quiet()
        .env("POWERZ_SIM_MENU_SAVE", "3@0")
        .args(["logo", "write", "--i-understand", "--yes"])
        .arg(&path)
        .assert()
        .success();
    let out = stdout_of(&a);
    let all = format!("{out}{}", stderr_of(&a));
    assert!(
        out.contains("Startup: now off; the upload sets it to 3 s"),
        "{all}"
    );
    assert!(!all.contains("startup_duration"), "no hint: {all}");
    assert!(!all.to_ascii_lowercase().contains("hint"), "no hint: {all}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ycrqrq_3_write_with_the_startup_screen_on_shows_the_duration_and_no_hint() {
    let dir = fresh_dir("write-3s");
    let path = png(&dir, "in.png", 64, 48);
    let a = sim_quiet()
        .env("POWERZ_SIM_STARTUP_LOGO", "3")
        .env("POWERZ_SIM_MENU_SAVE", "3@0")
        .args(["logo", "write", "--i-understand", "--yes", "--alpha"])
        .arg(&path)
        .assert()
        .success();
    let out = stdout_of(&a);
    let all = format!("{out}{}", stderr_of(&a));
    assert!(out.lines().any(|l| l.trim() == "Startup: 3 s"), "{out}");
    assert!(
        !all.contains("startup_duration"),
        "no hint when it shows: {all}"
    );
    assert!(out.to_ascii_lowercase().contains("verified"), "{out}");
    let _ = std::fs::remove_dir_all(&dir);
}

// ------------------------------------------------------------ AC-YCRQRQ.4

#[test]
fn ycrqrq_4_remove_without_i_understand_is_refused_and_sends_nothing() {
    for extra in [&[][..], &["--yes"][..]] {
        let a = sim()
            .args(["logo", "remove"])
            .args(extra)
            .write_stdin("remove\n")
            .assert()
            .failure();
        assert_eq!(code_of(&a), 2, "usage error, args {extra:?}");
        let err = stderr_of(&a);
        assert!(err.contains("--i-understand"), "{err}");
        assert_no_write(&err);
    }
}

#[test]
fn ycrqrq_4_remove_with_a_wrong_confirmation_sends_nothing() {
    let a = sim()
        .args(["logo", "remove", "--i-understand"])
        .write_stdin("write\n")
        .assert()
        .failure();
    assert_eq!(code_of(&a), 2);
    assert!(
        stdout_of(&a).contains("remove"),
        "the prompt names the word to type"
    );
    assert_no_write(&stderr_of(&a));
}

#[test]
fn ycrqrq_4_remove_with_the_typed_confirmation_prints_the_verified_result() {
    let a = sim()
        .env("POWERZ_SIM_MENU_SAVE", "off@0")
        .args(["logo", "remove", "--i-understand"])
        .write_stdin("remove\n")
        .assert()
        .success();
    let trace = stderr_of(&a);
    assert_eq!(sent_frames(&trace, "4a").len(), 1, "one clearing chunk");
    assert_eq!(sent_frames(&trace, "48").len(), 1, "one flag write");
    let out = stdout_of(&a);
    assert!(out.to_ascii_lowercase().contains("verified"), "{out}");
    assert!(out.to_ascii_lowercase().contains("removed"), "{out}");
    assert_private(&a);
}

#[test]
fn ycrqrq_4_remove_with_yes_skips_only_the_prompt() {
    let a = sim_quiet()
        .env("POWERZ_SIM_MENU_SAVE", "off@0")
        .args(["logo", "remove", "--i-understand", "--yes"])
        .assert()
        .success();
    let out = stdout_of(&a);
    assert!(out.to_ascii_lowercase().contains("verified"), "{out}");
    assert!(out.lines().any(|l| l.trim() == "Startup: off"), "{out}");
}

// ------------------------------------------------------------ AC-YCRQRQ.5

#[test]
fn ycrqrq_5_ctrl_c_in_the_chunk_phase_finishes_the_write_first() {
    let dir = fresh_dir("ctrl-c");
    let path = png(&dir, "in.png", 64, 48);
    let p = encoded(&path, LogoMode::Opaque).preview();
    assert!(p.chunks > 3);
    let a = sim()
        .env("POWERZ_SIM_CTRL_C_AFTER_CHUNKS", "2")
        .env("POWERZ_SIM_MENU_SAVE", "3@0")
        .args(["logo", "write", "--i-understand", "--yes"])
        .arg(&path)
        .assert();
    let err = stderr_of(&a);
    assert_eq!(
        sent_frames(&err, "4a").len(),
        p.chunks,
        "every chunk is still sent after Ctrl-C:\n{err}"
    );
    assert_eq!(
        sent_frames(&err, "48").len(),
        1,
        "the flag is still written"
    );
    let notes: Vec<&str> = err
        .lines()
        .filter(|l| l.to_ascii_lowercase().contains("finish"))
        .collect();
    assert_eq!(
        notes.len(),
        1,
        "one note that the write finishes first:\n{err}"
    );
    assert!(
        stdout_of(&a).to_ascii_lowercase().contains("verified"),
        "the write ends verified, then the tool exits: {}",
        stdout_of(&a)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ------------------------------------------------------------ AC-YCRQRQ.6

#[test]
fn ycrqrq_6_a_rejected_chunk_names_the_step_and_index() {
    let dir = fresh_dir("fail-reject");
    let path = png(&dir, "in.png", 64, 48);
    let a = sim()
        .env("POWERZ_SIM_LOGO_FAULT", "reject@3")
        .args(["logo", "write", "--i-understand", "--yes"])
        .arg(&path)
        .assert()
        .failure();
    assert_ne!(code_of(&a), 0);
    let err = stderr_of(&a);
    let msg = err
        .lines()
        .find(|l| l.starts_with("powerz:"))
        .unwrap_or_else(|| panic!("an error line:\n{err}"));
    assert!(msg.contains("chunk 3"), "names chunk 3: {msg}");
    assert_eq!(
        sent_frames(&err, "4a").len(),
        4,
        "the write stops at chunk 3"
    );
    assert!(
        sent_frames(&err, "48").is_empty(),
        "no flag after a failed chunk"
    );
    assert!(!stdout_of(&a).to_ascii_lowercase().contains("verified"));
    assert_private(&a);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ycrqrq_6_a_chunk_without_reply_or_an_unplug_names_the_chunk() {
    let dir = fresh_dir("fail-noreply");
    let path = png(&dir, "in.png", 64, 48);
    for (fault, chunk) in [("no-reply@2", 2), ("unplug@5", 6)] {
        let a = sim_quiet()
            .env("POWERZ_SIM_LOGO_FAULT", fault)
            .args(["logo", "write", "--i-understand", "--yes"])
            .arg(&path)
            .assert()
            .failure();
        assert_ne!(code_of(&a), 0, "{fault}");
        let err = stderr_of(&a);
        assert!(
            err.contains(&format!("chunk {chunk}")),
            "{fault} names chunk {chunk}: {err}"
        );
        assert_private(&a);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ycrqrq_6_a_failed_remove_names_its_step() {
    let a = sim_quiet()
        .env("POWERZ_SIM_LOGO_FAULT", "reject@0")
        .args(["logo", "remove", "--i-understand", "--yes"])
        .assert()
        .failure();
    assert_ne!(code_of(&a), 0);
    let err = stderr_of(&a);
    assert!(err.contains("clear"), "names the clear step: {err}");
    assert!(err.contains("chunk 0"), "names the chunk index: {err}");
    assert!(!stdout_of(&a).to_ascii_lowercase().contains("verified"));
    assert_private(&a);
}

#[test]
fn ycrqrq_6_errors_on_every_command_keep_serial_and_uid_out() {
    let dir = fresh_dir("fail-private");
    // A meter that stops answering mid-read.
    let a = sim_quiet()
        .env("POWERZ_SIM_FAULT", "timeout")
        .args(["logo", "read"])
        .assert()
        .failure();
    assert_ne!(code_of(&a), 0);
    assert_private(&a);
    // A missing image file.
    let a = sim_quiet()
        .args(["logo", "write", "--i-understand", "--yes"])
        .arg(dir.join("missing.png"))
        .assert()
        .failure();
    assert_ne!(code_of(&a), 0);
    assert!(stderr_of(&a).contains("missing.png"), "{}", stderr_of(&a));
    assert_private(&a);
    let _ = std::fs::remove_dir_all(&dir);
}

// ------------------------------------------------------------ AC-YCRQRQ.7

#[test]
fn ycrqrq_7_a_non_km003c_meter_is_refused_before_any_write() {
    let dir = fresh_dir("km002c");
    let path = png(&dir, "in.png", 64, 48);
    let a = sim()
        .env("POWERZ_SIM_MODEL", "KM002C")
        .args(["logo", "write", "--i-understand", "--yes"])
        .arg(&path)
        .assert()
        .failure();
    assert_ne!(code_of(&a), 0);
    let err = stderr_of(&a);
    let msg = err
        .lines()
        .find(|l| l.starts_with("powerz:"))
        .unwrap_or_else(|| panic!("an error line:\n{err}"));
    assert!(msg.contains("KM002C") && msg.contains("KM003C"), "{msg}");
    assert_no_write(&err);
    let a = sim()
        .env("POWERZ_SIM_MODEL", "KM002C")
        .args(["logo", "remove", "--i-understand", "--yes"])
        .assert()
        .failure();
    assert_ne!(code_of(&a), 0);
    assert_no_write(&stderr_of(&a));
    assert_private(&a);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ycrqrq_7_a_non_km003c_meter_is_refused_before_the_prompt() {
    let dir = fresh_dir("km002c-prompt");
    let path = png(&dir, "in.png", 64, 48);
    let a = sim()
        .env("POWERZ_SIM_MODEL", "KM002C")
        .args(["logo", "write", "--i-understand"])
        .arg(&path)
        .write_stdin("write\n")
        .assert()
        .failure();
    assert!(
        !stdout_of(&a).to_lowercase().contains("type"),
        "no prompt before the model refusal:\n{}",
        stdout_of(&a)
    );
    assert_no_write(&stderr_of(&a));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ycrqrq_3_without_i_understand_no_meter_is_opened() {
    let dir = fresh_dir("no-open");
    let path = png(&dir, "in.png", 64, 48);
    for args in [
        vec!["--device", "no-such-meter-YCRQRQ", "logo", "write"],
        vec!["--device", "no-such-meter-YCRQRQ", "logo", "remove"],
    ] {
        let mut cmd = sim();
        cmd.args(&args);
        if args[3] == "write" {
            cmd.arg(&path);
        }
        let a = cmd.assert().failure();
        assert_eq!(code_of(&a), 2, "usage refusal, not a device error");
        assert!(
            stderr_of(&a).contains("--i-understand"),
            "{}",
            stderr_of(&a)
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ycrqrq_7_help_lists_the_four_commands_and_their_flags() {
    let a = powerz().args(["logo", "--help"]).assert().success();
    let out = stdout_of(&a);
    for sub in ["preview", "read", "write", "remove"] {
        assert!(out.contains(sub), "`logo --help` lists {sub}: {out}");
    }
    let a = powerz()
        .args(["logo", "write", "--help"])
        .assert()
        .success();
    let out = stdout_of(&a);
    for flag in ["--alpha", "--i-understand", "--yes"] {
        assert!(out.contains(flag), "`logo write --help` has {flag}: {out}");
    }
    let a = powerz()
        .args(["logo", "preview", "--help"])
        .assert()
        .success();
    let out = stdout_of(&a);
    assert!(out.contains("--alpha") && out.contains("-o"), "{out}");
}

#[test]
fn ycrqrq_7_readme_and_cli_docs_describe_the_logo_commands() {
    for doc in ["README.md", "docs/reference/cli.md"] {
        let text = repo_file(doc);
        for cmd in ["logo preview", "logo read", "logo write", "logo remove"] {
            assert!(text.contains(cmd), "{doc} describes `{cmd}`");
        }
    }
    let cli = repo_file("docs/reference/cli.md");
    for word in ["--i-understand", "--alpha", "KM003C"] {
        let logo_part: String = cli
            .lines()
            .filter(|l| l.contains("logo "))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            logo_part.contains(word),
            "the logo rows of cli.md mention {word}"
        );
    }
}

// ------------------------------------------------------------ AC-YCRQRQ.8
//
// `powerz logo read --dump ADDR LEN` prints a hex dump: each line starts
// with an 8-hex-digit address (an optional `0x` prefix and trailing `:` are
// fine) followed by the bytes as 2-digit hex tokens; anything after the
// first non-byte token (an ASCII column, say) is ignored.
//
// The read pacing (>= 50 ms between reads) is not observable through the
// CLI: the trace carries no timestamps and the fake clock is internal. The
// session tests for `logo_read_back` cover it.

/// Start of the logo read window.
const WINDOW: u32 = 0x9830_0000;
/// One past the end of the logo read window.
const WINDOW_END: u32 = 0x9832_0000;

/// The bytes a hex dump shows, keyed by address.
fn dumped_bytes(out: &str) -> std::collections::BTreeMap<u32, u8> {
    let mut map = std::collections::BTreeMap::new();
    for line in out.lines() {
        let mut tokens = line.split_whitespace();
        let Some(first) = tokens.next() else { continue };
        let a = first.trim_end_matches(':');
        let a = a
            .strip_prefix("0x")
            .or_else(|| a.strip_prefix("0X"))
            .unwrap_or(a);
        if a.len() != 8 {
            continue;
        }
        let Ok(addr) = u32::from_str_radix(a, 16) else {
            continue;
        };
        for (i, t) in tokens.enumerate() {
            if t.len() != 2 {
                break;
            }
            let Ok(b) = u8::from_str_radix(t, 16) else {
                break;
            };
            map.insert(addr + i as u32, b);
        }
    }
    map
}

/// `(addr, len)` of every memory read the meter confirmed, from the
/// plaintext confirmations (`c4 id 01 01 <addr le> <len le> ...`) in the
/// trace.
fn memory_reads(trace: &str) -> Vec<(u32, u32)> {
    trace
        .lines()
        .filter(|l| l.starts_with("< ["))
        .filter_map(|l| l.split_once("] ").map(|(_, b)| b))
        .filter_map(|bytes| {
            let b: Vec<u8> = bytes
                .split(' ')
                .filter_map(|t| u8::from_str_radix(t, 16).ok())
                .collect();
            (b.len() >= 12 && b[0] == 0xC4).then(|| {
                (
                    u32::from_le_bytes([b[4], b[5], b[6], b[7]]),
                    u32::from_le_bytes([b[8], b[9], b[10], b[11]]),
                )
            })
        })
        .collect()
}

/// Memory reads inside the logo window.
fn window_reads(trace: &str) -> Vec<(u32, u32)> {
    memory_reads(trace)
        .into_iter()
        .filter(|&(a, _)| (WINDOW..WINDOW_END).contains(&a))
        .collect()
}

/// A seed file of `n` distinct-looking bytes.
fn seed_pattern(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i * 7 + 3) as u8 ^ (i >> 8) as u8).collect()
}

#[test]
fn ycrqrq_8_dump_of_an_empty_logo_area_shows_ff_bytes_with_addresses() {
    let a = sim()
        .args(["logo", "read", "--dump", "0x98300000", "16"])
        .assert()
        .success();
    let out = stdout_of(&a);
    assert!(
        out.to_ascii_lowercase().contains("98300000"),
        "the dump shows the address: {out}"
    );
    let bytes = dumped_bytes(&out);
    let want: Vec<u32> = (WINDOW..WINDOW + 16).collect();
    assert_eq!(
        bytes.keys().copied().collect::<Vec<_>>(),
        want,
        "exactly the 16 bytes from 0x98300000 are dumped: {out}"
    );
    assert!(bytes.values().all(|&b| b == 0xFF), "erased flash: {out}");
    let trace = stderr_of(&a);
    assert_eq!(window_reads(&trace), vec![(WINDOW, 16)], "{trace}");
    assert_no_write(&trace);
    assert_private(&a);
}

#[test]
fn ycrqrq_8_dump_of_600_bytes_shows_the_seeded_bytes_in_256_byte_reads() {
    let dir = fresh_dir("dump-600");
    let seed = seed_pattern(1024);
    let flash = dir.join("flash.bin");
    std::fs::write(&flash, &seed).unwrap();
    let a = sim()
        .env("POWERZ_SIM_LOGO_FILE", &flash)
        .args(["logo", "read", "--dump", "0x98300000", "600"])
        .assert()
        .success();
    let out = stdout_of(&a);
    let bytes = dumped_bytes(&out);
    let want: std::collections::BTreeMap<u32, u8> = seed[..600]
        .iter()
        .enumerate()
        .map(|(i, &b)| (WINDOW + i as u32, b))
        .collect();
    assert!(bytes == want, "the dump holds the 600 seeded bytes:\n{out}");
    let trace = stderr_of(&a);
    let reads = window_reads(&trace);
    assert_eq!(reads.len(), 3, "600 bytes take 3 reads: {reads:x?}");
    assert!(
        reads.iter().all(|&(_, len)| len > 0 && len <= 256),
        "every read is at most 256 bytes: {reads:x?}"
    );
    let mut next = WINDOW;
    for &(addr, len) in &reads {
        assert_eq!(addr, next, "the reads are contiguous: {reads:x?}");
        next = addr + len;
    }
    assert_eq!(next, WINDOW + 600, "the reads cover 600 bytes: {reads:x?}");
    assert_eq!(
        sent_frames(&trace, "44").len(),
        memory_reads(&trace).len(),
        "every memory-read request was confirmed"
    );
    assert_no_write(&trace);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ycrqrq_8_dump_address_accepts_hex_and_decimal() {
    let dir = fresh_dir("dump-dec");
    let seed = seed_pattern(64);
    let flash = dir.join("flash.bin");
    std::fs::write(&flash, &seed).unwrap();
    let addr = WINDOW + 0x10;
    for arg in [format!("{addr:#x}"), addr.to_string()] {
        let a = sim()
            .env("POWERZ_SIM_LOGO_FILE", &flash)
            .args(["logo", "read", "--dump", &arg, "16"])
            .assert()
            .success();
        let out = stdout_of(&a);
        let bytes = dumped_bytes(&out);
        let want: std::collections::BTreeMap<u32, u8> = seed[0x10..0x20]
            .iter()
            .enumerate()
            .map(|(i, &b)| (addr + i as u32, b))
            .collect();
        assert!(bytes == want, "ADDR {arg}: 16 bytes from {addr:#x}:\n{out}");
        let trace = stderr_of(&a);
        assert_eq!(window_reads(&trace), vec![(addr, 16)], "ADDR {arg}");
        assert_no_write(&trace);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ycrqrq_8_dump_ending_exactly_at_the_window_end_is_allowed() {
    let a = sim()
        .args(["logo", "read", "--dump", "0x9831FFF0", "16"])
        .assert()
        .success();
    let out = stdout_of(&a);
    let bytes = dumped_bytes(&out);
    assert_eq!(bytes.len(), 16, "{out}");
    assert_eq!(bytes.keys().next().copied(), Some(WINDOW_END - 16), "{out}");
    let trace = stderr_of(&a);
    assert_eq!(window_reads(&trace), vec![(WINDOW_END - 16, 16)]);
    assert_no_write(&trace);
}

#[test]
fn ycrqrq_8_dump_outside_the_window_is_refused_before_anything_is_sent() {
    let cases: [(&str, &str, &str); 5] = [
        ("below the window", "0x982FFFF0", "16"),
        ("ends past the window", "0x9831FFF0", "32"),
        ("starts at the window end", "0x98320000", "16"),
        ("length zero", "0x98300000", "0"),
        ("address overflow", "0xFFFFFFF0", "0x20"),
    ];
    for (what, addr, len) in cases {
        let a = sim()
            .args(["logo", "read", "--dump", addr, len])
            .assert()
            .failure();
        assert_eq!(code_of(&a), 2, "{what}: usage error");
        let err = stderr_of(&a);
        assert!(
            err.to_ascii_lowercase().contains("98300000")
                || err.to_ascii_lowercase().contains("window"),
            "{what}: the error names the allowed window:\n{err}"
        );
        assert!(
            sent_frames(&err, "44").is_empty(),
            "{what}: no memory read was sent:\n{err}"
        );
        assert!(
            !err.lines().any(|l| l.starts_with("> [")),
            "{what}: nothing at all was sent:\n{err}"
        );
        assert_no_write(&err);
        assert!(stdout_of(&a).trim().is_empty(), "{what}: no dump printed");
    }
}

#[test]
fn ycrqrq_8_cli_docs_describe_dump() {
    let cli = repo_file("docs/reference/cli.md");
    let logo_part: String = cli
        .lines()
        .filter(|l| l.contains("logo "))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        logo_part.contains("--dump"),
        "the logo rows of cli.md describe `logo read --dump`"
    );
    let a = powerz().args(["logo", "read", "--help"]).assert().success();
    assert!(
        stdout_of(&a).contains("--dump"),
        "`logo read --help` has --dump"
    );
}

// ------------------------------------------------------------ FR Z39H24
//
// The 0x1C write sets the startup duration (3 s for an upload, off for a
// Remove) and leaves the meter's settings unsaved until a save in its menu.

use powerz_protocol::decode::settings::MENU_SAVE_STEPS;

/// Trace lines of SETTINGS reads sent (`0c id 10 00`).
fn settings_reads(trace: &str) -> Vec<&str> {
    sent_frames(trace, "0c")
        .into_iter()
        .filter(|l| {
            l.split_once("] ").is_some_and(|(_, b)| {
                let t: Vec<&str> = b.split(' ').collect();
                t.len() == 4 && t[2] == "10" && t[3] == "00"
            })
        })
        .collect()
}

/// SETTINGS reads sent after the startup-duration (0x48) write.
fn settings_reads_after_the_write(trace: &str) -> usize {
    let sent: Vec<&str> = trace.lines().filter(|l| l.starts_with("> [")).collect();
    let Some(at) = sent.iter().position(|l| {
        l.split_once("] ")
            .is_some_and(|(_, b)| b.split(' ').next() == Some("48"))
    }) else {
        panic!("no 0x48 write in:\n{trace}");
    };
    let reads = settings_reads(trace);
    sent[at..].iter().filter(|l| reads.contains(l)).count()
}

fn no_settings_write(trace: &str) {
    assert!(
        sent_frames(trace, "0a").is_empty(),
        "a settings write (0x0A) was sent:\n{trace}"
    );
}

fn stdout_line_index(out: &str, needle: &str) -> usize {
    out.lines()
        .position(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("no {needle:?} in:\n{out}"))
}

// ------------------------------------------------------------ AC-Z39H24.8

#[test]
fn z39h24_8_write_prints_the_duration_change_before_the_confirmation() {
    let dir = fresh_dir("z39-8-write");
    let path = png(&dir, "in.png", 64, 48);
    // A wrong confirmation: the change was printed, nothing was written.
    let a = sim()
        .env("POWERZ_SIM_STARTUP_LOGO", "5")
        .args(["logo", "write", "--i-understand"])
        .arg(&path)
        .write_stdin("nope\n")
        .assert()
        .failure();
    assert_eq!(code_of(&a), 2);
    let out = stdout_of(&a);
    assert!(
        out.contains("Startup: now 5 s; the upload sets it to 3 s"),
        "{out}"
    );
    assert_no_write(&stderr_of(&a));

    // With --yes it is printed too, ahead of the result.
    let a = sim_quiet()
        .env("POWERZ_SIM_STARTUP_LOGO", "1")
        .env("POWERZ_SIM_MENU_SAVE", "1@0")
        .args(["logo", "write", "--i-understand", "--yes"])
        .arg(&path)
        .assert()
        .success();
    let out = stdout_of(&a);
    let before = stdout_line_index(&out, "Startup: now 1 s; the upload sets it to 3 s");
    let logo = stdout_line_index(&out, "written and verified");
    assert!(before < logo, "{out}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn z39h24_8_remove_prints_the_duration_change_before_the_confirmation() {
    let a = sim()
        .env("POWERZ_SIM_STARTUP_LOGO", "5")
        .args(["logo", "remove", "--i-understand"])
        .write_stdin("nope\n")
        .assert()
        .failure();
    assert_eq!(code_of(&a), 2);
    let out = stdout_of(&a);
    assert!(
        out.contains("Startup: now 5 s; the remove turns the startup screen off"),
        "{out}"
    );
    assert_no_write(&stderr_of(&a));

    let a = sim_quiet()
        .env("POWERZ_SIM_STARTUP_LOGO", "3")
        .env("POWERZ_SIM_MENU_SAVE", "off@0")
        .args(["logo", "remove", "--i-understand", "--yes"])
        .assert()
        .success();
    let out = stdout_of(&a);
    let before = stdout_line_index(
        &out,
        "Startup: now 3 s; the remove turns the startup screen off",
    );
    let logo = stdout_line_index(&out, "removed and verified");
    assert!(before < logo, "{out}");
}

// ------------------------------------------------------------ AC-Z39H24.9

#[test]
fn z39h24_9_write_prints_the_duration_from_a_fresh_read() {
    let dir = fresh_dir("z39-9-fresh");
    let path = png(&dir, "in.png", 64, 48);
    // Saved in the menu at 1 s before the re-read: neither the 5 s from
    // before nor the 3 s the write set.
    let a = sim()
        .env("POWERZ_SIM_STARTUP_LOGO", "5")
        .env("POWERZ_SIM_MENU_SAVE", "1@0")
        .args(["logo", "write", "--i-understand", "--yes"])
        .arg(&path)
        .assert()
        .success();
    let out = stdout_of(&a);
    assert!(out.lines().any(|l| l.trim() == "Startup: 1 s"), "{out}");
    assert!(!out.lines().any(|l| l.trim() == "Startup: 5 s"), "{out}");
    assert!(!out.lines().any(|l| l.trim() == "Startup: 3 s"), "{out}");
    let err = stderr_of(&a);
    assert!(!err.contains("Waiting"), "no wait when saved:\n{err}");
    assert!(settings_reads_after_the_write(&err) >= 1, "{err}");
    no_settings_write(&err);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn z39h24_9_write_shows_the_steps_and_waits_for_the_menu_save() {
    let dir = fresh_dir("z39-9-wait");
    let path = png(&dir, "in.png", 64, 48);
    let a = sim()
        .env("POWERZ_SIM_STARTUP_LOGO", "5")
        .env("POWERZ_SIM_MENU_SAVE", "5@10000")
        .args(["logo", "write", "--i-understand", "--yes"])
        .arg(&path)
        .assert()
        .success();
    let err = stderr_of(&a);
    assert!(
        err.contains("The meter changed its startup duration but left its settings unsaved."),
        "{err}"
    );
    assert!(err.contains(MENU_SAVE_STEPS), "{err}");
    assert!(err.contains("Waiting up to 300 s"), "{err}");
    assert!(!err.contains("Pick Disable"), "that is for Remove:\n{err}");
    let out = stdout_of(&a);
    assert!(out.to_ascii_lowercase().contains("verified"), "{out}");
    assert!(out.lines().any(|l| l.trim() == "Startup: 5 s"), "{out}");
    // A 10 s wait at 2 s gaps: several read-only checks, one 0x1C write.
    let reads = settings_reads_after_the_write(&err);
    assert!((4..=10).contains(&reads), "{reads} SETTINGS reads:\n{err}");
    assert_eq!(sent_frames(&err, "48").len(), 1, "one 0x1C write");
    no_settings_write(&err);
    assert_private(&a);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn z39h24_9_write_exits_6_when_the_wait_times_out() {
    let dir = fresh_dir("z39-9-timeout");
    let path = png(&dir, "in.png", 64, 48);
    let a = sim()
        .env("POWERZ_SIM_MENU_SAVE", "5@60000")
        .args([
            "logo",
            "write",
            "--i-understand",
            "--yes",
            "--menu-save-timeout",
            "4",
        ])
        .arg(&path)
        .assert()
        .failure();
    assert_eq!(code_of(&a), 6);
    let err = stderr_of(&a);
    assert!(err.contains("Waiting up to 4 s"), "{err}");
    let msg = err
        .lines()
        .find(|l| l.starts_with("powerz:"))
        .unwrap_or_else(|| panic!("an error line:\n{err}"));
    assert!(msg.contains(MENU_SAVE_STEPS), "{msg}");
    assert!(stdout_of(&a).to_ascii_lowercase().contains("verified"));
    assert!(
        !stdout_of(&a).lines().any(|l| l.trim() == "Startup: 5 s"),
        "the save came after the timeout"
    );
    no_settings_write(&err);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn z39h24_9_the_default_wait_is_300_s() {
    let dir = fresh_dir("z39-9-default");
    let path = png(&dir, "in.png", 64, 48);
    // Nobody saves: the whole default wait runs on the fake clock.
    let a = sim()
        .args(["logo", "write", "--i-understand", "--yes"])
        .arg(&path)
        .assert()
        .failure();
    assert_eq!(code_of(&a), 6);
    let err = stderr_of(&a);
    assert!(err.contains("Waiting up to 300 s"), "{err}");
    let reads = settings_reads_after_the_write(&err);
    assert!(
        (140..=160).contains(&reads),
        "300 s at 2 s gaps, {reads} reads"
    );
    no_settings_write(&err);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn z39h24_9_timeout_0_does_not_wait() {
    let dir = fresh_dir("z39-9-zero");
    let path = png(&dir, "in.png", 64, 48);
    let a = sim()
        .env("POWERZ_SIM_MENU_SAVE", "5@0")
        .env("POWERZ_SIM_STARTUP_LOGO", "5")
        .args(["logo", "remove", "--i-understand", "--yes"])
        .assert()
        .success();
    assert!(stdout_of(&a).lines().any(|l| l.trim() == "Startup: 5 s"));
    let a = sim()
        .args([
            "logo",
            "write",
            "--i-understand",
            "--yes",
            "--menu-save-timeout",
            "0",
        ])
        .arg(&path)
        .assert()
        .failure();
    assert_eq!(code_of(&a), 6);
    let err = stderr_of(&a);
    assert!(err.contains(MENU_SAVE_STEPS), "{err}");
    let reads = settings_reads_after_the_write(&err);
    assert!((1..=2).contains(&reads), "no wait, {reads} reads:\n{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn z39h24_9_remove_says_to_pick_disable_and_waits() {
    let a = sim()
        .env("POWERZ_SIM_STARTUP_LOGO", "5")
        .env("POWERZ_SIM_MENU_SAVE", "off@6000")
        .args(["logo", "remove", "--i-understand", "--yes"])
        .assert()
        .success();
    let err = stderr_of(&a);
    assert!(err.contains(MENU_SAVE_STEPS), "{err}");
    assert!(
        err.contains("Pick Disable to keep the startup screen off."),
        "{err}"
    );
    assert!(err.contains("Waiting up to 300 s"), "{err}");
    let out = stdout_of(&a);
    assert!(out.contains("removed and verified"), "{out}");
    assert!(out.lines().any(|l| l.trim() == "Startup: off"), "{out}");
    no_settings_write(&err);

    // From 5 s, so the remove really moves the duration: writing off over an
    // already-off duration changes no byte and leaves the check value valid.
    let a = sim()
        .env("POWERZ_SIM_STARTUP_LOGO", "5")
        .args([
            "logo",
            "remove",
            "--i-understand",
            "--yes",
            "--menu-save-timeout",
            "2",
        ])
        .assert()
        .failure();
    assert_eq!(code_of(&a), 6);
    assert!(stderr_of(&a).contains("Pick Disable"), "{}", stderr_of(&a));
}

#[test]
fn z39h24_9_ctrl_c_during_the_wait_exits_6() {
    let dir = fresh_dir("z39-9-ctrlc");
    let path = png(&dir, "in.png", 64, 48);
    // Ctrl-C (pressed during the chunks) is still set when the wait starts;
    // the save would only come at 60 s.
    let a = sim()
        .env("POWERZ_SIM_CTRL_C_AFTER_CHUNKS", "2")
        .env("POWERZ_SIM_MENU_SAVE", "3@60000")
        .args(["logo", "write", "--i-understand", "--yes"])
        .arg(&path)
        .assert()
        .failure();
    assert_eq!(code_of(&a), 6);
    let err = stderr_of(&a);
    assert!(err.contains(MENU_SAVE_STEPS), "{err}");
    assert!(
        settings_reads_after_the_write(&err) <= 3,
        "the wait stopped at once:\n{err}"
    );
    assert!(stdout_of(&a).to_ascii_lowercase().contains("verified"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn z39h24_9_help_shows_the_menu_save_timeout() {
    for sub in ["write", "remove"] {
        let a = powerz().args(["logo", sub, "--help"]).assert().success();
        let out = stdout_of(&a);
        assert!(out.contains("--menu-save-timeout"), "logo {sub}: {out}");
        assert!(out.contains("300"), "logo {sub} names the default: {out}");
    }
}

// ------------------------------------------------------------ AC-Z39H24.11

#[test]
fn z39h24_11_cli_docs_describe_the_menu_save_and_exit_code_6() {
    let cli = repo_file("docs/reference/cli.md");
    assert!(cli.contains("--menu-save-timeout"), "cli.md: the option");
    let exit = cli
        .split("## Exit codes")
        .nth(1)
        .expect("an Exit codes section");
    let exit = exit.split("\n## ").next().unwrap_or(exit);
    assert!(
        exit.contains('6'),
        "the Exit codes section lists 6:\n{exit}"
    );
    assert!(
        exit.to_lowercase().contains("menu"),
        "exit 6 is about the menu save:\n{exit}"
    );
    let logo_part: String = cli
        .lines()
        .filter(|l| l.contains("logo "))
        .collect::<Vec<_>>()
        .join("\n")
        .to_lowercase();
    assert!(logo_part.contains("startup duration"), "{logo_part}");
    assert!(logo_part.contains("3 s"), "upload sets 3 s");
    assert!(
        !logo_part.contains("settings set startup_duration=3"),
        "the old hint is gone"
    );
    assert!(
        cli.contains("POWERZ_SIM_MENU_SAVE"),
        "the test knob is listed"
    );
}

// ================================================= FR 70X5YX: built-in logos
//
// `logo preview` and `logo write` take `--builtin NAME` (surf or
// surf-wordmark) in place of the IMG file; exactly one of the two is
// required. A built-in logo is always encoded in the transparent (alpha)
// mode, so `--alpha` changes nothing for it.

use powerz_protocol::logo::builtin::BuiltinLogo;

/// The repository's brand meter logo PNG of built-in `name`.
fn brand_png(name: &str) -> PathBuf {
    let file = match name {
        "surf" => "meter-logo.png",
        "surf-wordmark" => "meter-logo-wordmark.png",
        other => panic!("no brand PNG for {other}"),
    };
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/brand")
        .join(file)
}

/// The blob built-in `b` encodes to.
fn builtin_blob(b: BuiltinLogo) -> LogoBlob {
    b.encode().expect("the built-in logo encodes")
}

/// The logo bytes carried by the 0x4A chunk frames on the trace, in order:
/// each frame is a 4-byte header, a 16-byte extended part, then 256 bytes.
fn sent_logo_bytes(trace: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for line in sent_frames(trace, "4a") {
        let (_, hex) = line.split_once("] ").expect("a frame line");
        let bytes: Vec<u8> = hex
            .split_whitespace()
            .map(|h| u8::from_str_radix(h, 16).expect("hex byte"))
            .collect();
        assert_eq!(bytes.len(), 20 + 256, "a whole chunk frame: {line}");
        out.extend_from_slice(&bytes[20..]);
    }
    out
}

/// No frame at all went out or came back.
fn assert_no_frames(trace: &str) {
    assert!(
        !trace
            .lines()
            .any(|l| l.starts_with("> [") || l.starts_with("< [")),
        "frames were exchanged:\n{trace}"
    );
}

// ------------------------------------------------------------ AC-70X5YX.2

#[test]
fn _70x5yx_2_preview_of_a_builtin_prints_the_same_report_as_its_file_in_alpha() {
    for name in ["surf", "surf-wordmark"] {
        let file = sim_quiet()
            .args(["logo", "preview", "--alpha"])
            .arg(brand_png(name))
            .assert()
            .success();
        let file_out = stdout_of(&file);
        let a = sim_quiet()
            .args(["logo", "preview", "--builtin", name])
            .assert()
            .success();
        let out = stdout_of(&a);
        for line in file_out.lines().filter(|l| !l.trim().is_empty()) {
            assert!(
                out.lines().any(|l| l == line),
                "{name}: the file report line {line:?} is missing:\n{out}"
            );
        }
        assert!(contains_size(&out, 200, 200), "{name}: {out}");
        assert!(
            line_with(&out, "mode")
                .to_ascii_lowercase()
                .contains("alpha"),
            "{name}: transparent mode without --alpha: {out}"
        );
        assert!(line_with(&out, "blob").contains("120016"), "{out}");
        assert!(line_with(&out, "padded").contains("120064"), "{out}");
        assert!(line_with(&out, "chunk").contains("469"), "{out}");
    }
}

#[test]
fn _70x5yx_2_preview_of_a_builtin_writes_the_preview_png() {
    let dir = fresh_dir("70x5yx-preview-out");
    for (name, b) in [
        ("surf", BuiltinLogo::Surf),
        ("surf-wordmark", BuiltinLogo::SurfWordmark),
    ] {
        let out_png = dir.join(format!("{name}.png"));
        sim_quiet()
            .args(["logo", "preview", "--builtin", name, "-o"])
            .arg(&out_png)
            .assert()
            .success();
        let expected = logo::decode(&builtin_blob(b).bytes).unwrap();
        let got = png_pixels(&out_png);
        assert_eq!((got.w, got.h), (200, 200), "{name}");
        assert!(
            got.px == expected.px,
            "{name}: the PNG holds what would be written"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn _70x5yx_2_preview_of_a_builtin_opens_no_meter() {
    let a = sim()
        .args(["logo", "preview", "--builtin", "surf"])
        .assert()
        .success();
    assert_no_frames(&stderr_of(&a));
    powerz()
        .args([
            "--device",
            "no-such-meter-70X5YX",
            "logo",
            "preview",
            "--builtin",
            "surf-wordmark",
        ])
        .assert()
        .success();
}

// ------------------------------------------------------------ AC-70X5YX.3

#[test]
fn _70x5yx_3_write_of_a_builtin_without_i_understand_is_refused_and_opens_no_meter() {
    for extra in [&[][..], &["--yes"][..]] {
        let a = sim()
            .args(["logo", "write", "--builtin", "surf"])
            .args(extra)
            .write_stdin("write\n")
            .assert()
            .failure();
        assert_eq!(code_of(&a), 2, "usage error, args {extra:?}");
        let err = stderr_of(&a);
        assert!(err.contains("--i-understand"), "{err}");
        assert_no_frames(&err);
    }
    let a = powerz()
        .args([
            "--device",
            "no-such-meter-70X5YX",
            "logo",
            "write",
            "--builtin",
            "surf",
        ])
        .assert()
        .failure();
    assert_eq!(code_of(&a), 2, "refused before the meter is looked up");
}

#[test]
fn _70x5yx_3_write_of_a_builtin_with_a_wrong_confirmation_sends_nothing() {
    let a = sim()
        .args(["logo", "write", "--i-understand", "--builtin", "surf"])
        .write_stdin("nope\n")
        .assert()
        .failure();
    assert_eq!(code_of(&a), 2);
    let out = stdout_of(&a);
    assert!(out.contains("write"), "the prompt names the word: {out}");
    assert!(
        out.contains("Startup: now off; the upload sets it to 3 s"),
        "the duration change before the prompt: {out}"
    );
    assert_no_write(&stderr_of(&a));
}

#[test]
fn _70x5yx_3_write_of_a_builtin_sends_its_encoding_and_verifies() {
    for (name, b) in [
        ("surf", BuiltinLogo::Surf),
        ("surf-wordmark", BuiltinLogo::SurfWordmark),
    ] {
        let blob = builtin_blob(b);
        let a = sim()
            .env("POWERZ_SIM_MENU_SAVE", "3@0")
            .args(["logo", "write", "--i-understand", "--builtin", name])
            .write_stdin("write\n")
            .assert()
            .success();
        let trace = stderr_of(&a);
        assert_eq!(sent_frames(&trace, "4a").len(), 469, "{name}: 469 chunks");
        assert_eq!(sent_frames(&trace, "48").len(), 1, "{name}: one 0x1C write");
        assert!(
            sent_logo_bytes(&trace) == blob.padded(),
            "{name}: the blob sent is the built-in logo's encoding"
        );
        let out = stdout_of(&a);
        assert!(out.contains("written and verified"), "{name}: {out}");
        assert!(contains_size(&out, 200, 200), "{name}: {out}");
        assert!(
            out.lines().any(|l| l.trim() == "Startup: 3 s"),
            "{name}: the fresh settings read: {out}"
        );
        assert_private(&a);
    }
}

#[test]
fn _70x5yx_3_write_of_a_builtin_shows_chunk_and_read_back_progress() {
    let a = sim_quiet()
        .env("POWERZ_SIM_MENU_SAVE", "3@0")
        .args([
            "logo",
            "write",
            "--i-understand",
            "--yes",
            "--builtin",
            "surf",
        ])
        .assert()
        .success();
    let err = stderr_of(&a);
    let segs = segments(&err);
    for k in [1, 234, 469] {
        assert!(err.contains(&format!("{k}/469")), "{k}/469:\n{err}");
    }
    assert!(
        segs.iter()
            .any(|s| s.to_ascii_lowercase().contains("chunk") && s.contains("469/469")),
        "chunk progress:\n{err}"
    );
    assert!(
        segs.iter()
            .any(|s| s.to_ascii_lowercase().contains("read") && s.contains("469/469")),
        "read-back progress:\n{err}"
    );
    let out = stdout_of(&a);
    assert!(!out.contains("1/469"), "progress stays off stdout:\n{out}");
    assert!(
        out.contains("Startup: now off; the upload sets it to 3 s"),
        "also with --yes: {out}"
    );
    assert!(out.contains("written and verified"), "{out}");
}

#[test]
fn _70x5yx_3_write_of_a_builtin_waits_for_the_menu_save() {
    let a = sim()
        .env("POWERZ_SIM_STARTUP_LOGO", "5")
        .env("POWERZ_SIM_MENU_SAVE", "5@40000") // after the ~23 s of sim time a 469-chunk read-back takes
        .args([
            "logo",
            "write",
            "--i-understand",
            "--yes",
            "--builtin",
            "surf-wordmark",
        ])
        .assert()
        .success();
    let err = stderr_of(&a);
    assert!(
        err.contains("The meter changed its startup duration but left its settings unsaved."),
        "{err}"
    );
    assert!(err.contains(MENU_SAVE_STEPS), "{err}");
    assert!(err.contains("Waiting up to 300 s"), "{err}");
    let out = stdout_of(&a);
    assert!(
        out.contains("Startup: now 5 s; the upload sets it to 3 s"),
        "{out}"
    );
    assert!(out.contains("written and verified"), "{out}");
    assert!(out.lines().any(|l| l.trim() == "Startup: 5 s"), "{out}");
}

#[test]
fn _70x5yx_3_write_of_a_builtin_exits_6_when_the_menu_save_wait_times_out() {
    let a = sim()
        .env("POWERZ_SIM_MENU_SAVE", "5@60000")
        .args([
            "logo",
            "write",
            "--i-understand",
            "--yes",
            "--builtin",
            "surf",
            "--menu-save-timeout",
            "4",
        ])
        .assert()
        .failure();
    assert_eq!(code_of(&a), 6, "{}", stderr_of(&a));
}

// ------------------------------------------------------------ AC-70X5YX.4

#[test]
fn _70x5yx_4_a_builtin_and_a_file_together_is_a_usage_error() {
    let file = brand_png("surf");
    let a = sim()
        .args(["logo", "preview", "--builtin", "surf"])
        .arg(&file)
        .assert()
        .failure();
    assert_eq!(code_of(&a), 2, "preview: {}", stderr_of(&a));
    assert_no_frames(&stderr_of(&a));

    let a = sim()
        .args([
            "logo",
            "write",
            "--i-understand",
            "--yes",
            "--builtin",
            "surf",
        ])
        .arg(&file)
        .assert()
        .failure();
    assert_eq!(code_of(&a), 2, "write: {}", stderr_of(&a));
    assert_no_frames(&stderr_of(&a));

    // Before any meter is looked up, too.
    let a = powerz()
        .args([
            "--device",
            "no-such-meter-70X5YX",
            "logo",
            "write",
            "--i-understand",
            "--yes",
            "--builtin",
            "surf",
        ])
        .arg(&file)
        .assert()
        .failure();
    assert_eq!(code_of(&a), 2, "{}", stderr_of(&a));
}

#[test]
fn _70x5yx_4_neither_a_builtin_nor_a_file_is_a_usage_error() {
    let a = sim().args(["logo", "preview"]).assert().failure();
    assert_eq!(code_of(&a), 2, "preview: {}", stderr_of(&a));
    assert_no_frames(&stderr_of(&a));

    let a = sim()
        .args(["logo", "write", "--i-understand", "--yes"])
        .assert()
        .failure();
    assert_eq!(code_of(&a), 2, "write: {}", stderr_of(&a));
    assert_no_frames(&stderr_of(&a));

    let a = powerz()
        .args([
            "--device",
            "no-such-meter-70X5YX",
            "logo",
            "write",
            "--i-understand",
            "--yes",
        ])
        .assert()
        .failure();
    assert_eq!(code_of(&a), 2, "{}", stderr_of(&a));
}

#[test]
fn _70x5yx_4_an_unknown_builtin_is_a_usage_error_listing_the_names() {
    for args in [
        &["logo", "preview", "--builtin", "nope"][..],
        &[
            "logo",
            "write",
            "--i-understand",
            "--yes",
            "--builtin",
            "nope",
        ][..],
    ] {
        let a = sim().args(args).assert().failure();
        assert_eq!(code_of(&a), 2, "{args:?}: {}", stderr_of(&a));
        let err = stderr_of(&a);
        assert!(
            err.contains("surf-wordmark"),
            "{args:?} lists surf-wordmark:\n{err}"
        );
        assert!(
            err.replace("surf-wordmark", "").contains("surf"),
            "{args:?} lists surf:\n{err}"
        );
        assert_no_frames(&err);
    }
}

#[test]
fn _70x5yx_4_the_alpha_flag_changes_nothing_for_a_builtin() {
    for name in ["surf", "surf-wordmark"] {
        let plain = sim_quiet()
            .args(["logo", "preview", "--builtin", name])
            .assert()
            .success();
        let alpha = sim_quiet()
            .args(["logo", "preview", "--builtin", name, "--alpha"])
            .assert()
            .success();
        assert_eq!(stdout_of(&plain), stdout_of(&alpha), "{name}");
        assert!(
            line_with(&stdout_of(&plain), "mode")
                .to_ascii_lowercase()
                .contains("alpha"),
            "{name}: never opaque"
        );
    }
    let write = |alpha: bool| {
        let mut c = sim();
        c.env("POWERZ_SIM_MENU_SAVE", "3@0").args([
            "logo",
            "write",
            "--i-understand",
            "--yes",
            "--builtin",
            "surf",
        ]);
        if alpha {
            c.arg("--alpha");
        }
        let a = c.assert().success();
        sent_logo_bytes(&stderr_of(&a))
    };
    let (plain, alpha) = (write(false), write(true));
    assert!(plain == alpha, "--alpha changed the blob sent");
    assert!(
        plain == builtin_blob(BuiltinLogo::Surf).padded(),
        "always the transparent encoding"
    );
}

#[test]
fn _70x5yx_4_help_shows_the_builtin_option() {
    for cmd in ["preview", "write"] {
        let a = powerz().args(["logo", cmd, "--help"]).assert().success();
        let out = stdout_of(&a);
        assert!(out.contains("--builtin"), "logo {cmd} --help:\n{out}");
        assert!(out.contains("surf-wordmark"), "logo {cmd} --help:\n{out}");
    }
}

// ------------------------------------------------------------ AC-70X5YX.8

#[test]
fn _70x5yx_8_cli_reference_describes_the_builtin_option() {
    let cli = repo_file("docs/reference/cli.md");
    for cmd in ["`logo preview`", "`logo write`"] {
        let row = cli
            .lines()
            .find(|l| l.starts_with(&format!("| {cmd} |")))
            .unwrap_or_else(|| panic!("no {cmd} row"));
        assert!(row.contains("--builtin"), "{cmd} row: {row}");
        assert!(row.contains("surf-wordmark"), "{cmd} row: {row}");
        assert!(
            row.replace("surf-wordmark", "").contains("surf"),
            "{cmd} row: {row}"
        );
    }
    let section = cli
        .split("## Startup logo")
        .nth(1)
        .expect("a Startup logo section");
    let section = section.split("\n## ").next().unwrap_or(section);
    assert!(
        section.contains("--builtin"),
        "the Startup logo section names --builtin:\n{section}"
    );
    assert!(
        section.to_lowercase().contains("transparent"),
        "it says built-ins use the transparent mode:\n{section}"
    );
}

#[test]
fn _70x5yx_8_readme_startup_logo_section_mentions_the_builtin_logos() {
    let readme = repo_file("README.md");
    let section = readme
        .split("### Startup logo")
        .nth(1)
        .expect("a Startup logo section in README.md");
    let section = section.split("\n#").next().unwrap_or(section);
    assert!(
        section.contains("--builtin") || section.to_lowercase().contains("built-in"),
        "README's Startup logo section names the built-in logos:\n{section}"
    );
    assert!(
        section.to_lowercase().contains("open-powerz logo"),
        "and the desktop's open-powerz logo choice:\n{section}"
    );
}

#[test]
fn _70x5yx_8_desktop_backend_reference_describes_the_builtin_argument() {
    let doc = repo_file("docs/reference/desktop-backend.md");
    for cmd in ["logo_preview", "logo_upload"] {
        let lines: Vec<&str> = doc.lines().filter(|l| l.contains(cmd)).collect();
        assert!(!lines.is_empty(), "{cmd} is documented");
        assert!(
            lines.iter().any(|l| l.contains("builtin")),
            "{cmd} documents the builtin argument:\n{}",
            lines.join("\n")
        );
    }
    assert!(doc.contains("surf-wordmark"), "the built-in names");
}
