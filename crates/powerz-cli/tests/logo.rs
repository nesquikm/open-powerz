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

const KNOBS: [&str; 10] = [
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
fn ycrqrq_3_write_with_the_startup_screen_off_adds_a_hint() {
    let dir = fresh_dir("write-off");
    let path = png(&dir, "in.png", 64, 48);
    let a = sim_quiet()
        .args(["logo", "write", "--i-understand", "--yes"])
        .arg(&path)
        .assert()
        .success();
    let all = format!("{}{}", stdout_of(&a), stderr_of(&a));
    assert!(
        line_with(&stdout_of(&a), "startup")
            .to_ascii_lowercase()
            .contains("off"),
        "the startup-screen duration: {all}"
    );
    assert!(
        all.contains("startup_duration"),
        "the hint says how to turn the startup screen on: {all}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ycrqrq_3_write_with_the_startup_screen_on_shows_the_duration_and_no_hint() {
    let dir = fresh_dir("write-3s");
    let path = png(&dir, "in.png", 64, 48);
    let a = sim_quiet()
        .env("POWERZ_SIM_STARTUP_LOGO", "3")
        .args(["logo", "write", "--i-understand", "--yes", "--alpha"])
        .arg(&path)
        .assert()
        .success();
    let out = stdout_of(&a);
    let all = format!("{out}{}", stderr_of(&a));
    let line = line_with(&out, "startup");
    assert!(line.contains("3 s") || line.contains("3s"), "{out}");
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
        .args(["logo", "remove", "--i-understand", "--yes"])
        .assert()
        .success();
    let out = stdout_of(&a);
    assert!(out.to_ascii_lowercase().contains("verified"), "{out}");
    assert!(
        line_with(&out, "startup")
            .to_ascii_lowercase()
            .contains("off"),
        "{out}"
    );
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
