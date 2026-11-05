//! Startup logo commands for the desktop app (FR MEHQC0): preview a picture
//! as the exact blob that would be written, and upload, Remove or read the
//! meter's logo on the device worker, with progress batched for the UI.

use std::path::Path;
use std::time::Instant;

use powerz_protocol::decode::settings::StartupLogo;
use powerz_protocol::logo::{self, LogoBlob, LogoMode};
use powerz_protocol::session::{Clock, LogoProgress, LogoStatus, LogoStep, LogoWritten, Session};
use powerz_protocol::transport::Transport;
use powerz_protocol::transport::guard::GuardedTransport;
use serde::Serialize;

use crate::ui_error::UiError;
use crate::worker::Worker;

/// Least gap between two progress messages to the UI: at most 20 a second.
pub const PROGRESS_GAP_MS: u64 = 50;

/// What the preview shows: the encoded logo's numbers and its pixels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LogoPreviewView {
    /// Width in pixels of the logo as written.
    pub width: u32,
    /// Height in pixels of the logo as written.
    pub height: u32,
    /// `"opaque"` or `"alpha"`.
    pub mode: String,
    /// Unpadded blob size in bytes.
    pub blob_size: usize,
    /// Padded (written) size in bytes.
    pub padded_size: usize,
    /// Number of 256-byte chunks written.
    pub chunks: usize,
    /// Either side is below 48.
    pub small_side_warning: bool,
    /// The blob decoded back to RGBA8888 (`width * height * 4` bytes), base64.
    pub rgba: String,
}

/// One progress message: `done` of `total` for `step` (`"chunks"` or `"read_back"`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LogoProgressView {
    pub step: String,
    pub done: usize,
    pub total: usize,
}

/// What an upload or Remove left on the meter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LogoWrittenView {
    /// The startup-screen duration as the settings panel words it (`off`, `1 s`, …).
    pub startup_duration: String,
    /// The startup screen is off, so the logo will not show.
    pub startup_screen_off: bool,
}

/// What the meter's logo area holds now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LogoReadView {
    /// `"empty"`, `"logo"` or `"unrecognised"`.
    pub status: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub mode: Option<String>,
    /// The stored logo as RGBA8888, base64, when a full read was asked for.
    pub image: Option<String>,
}

/// Lets through at most one progress message per [`PROGRESS_GAP_MS`].
#[derive(Debug, Default)]
pub struct ProgressGate {
    /// When the last message went through, ms.
    last_ms: Option<u64>,
}

impl ProgressGate {
    /// A gate that lets the first message through.
    pub fn new() -> ProgressGate {
        ProgressGate { last_ms: None }
    }

    /// The message to send for `progress` at `now_ms`, if one is due.
    pub fn offer(&mut self, progress: LogoProgress, now_ms: u64) -> Option<LogoProgressView> {
        if self
            .last_ms
            .is_some_and(|last| now_ms.saturating_sub(last) < PROGRESS_GAP_MS)
        {
            return None;
        }
        self.last_ms = Some(now_ms);
        Some(LogoProgressView {
            step: match progress.step {
                LogoStep::Chunks => "chunks",
                LogoStep::ReadBack => "read_back",
            }
            .to_owned(),
            done: progress.done,
            total: progress.total,
        })
    }
}

/// A progress callback that passes `send` what a [`ProgressGate`] lets
/// through, timed on the wall clock.
pub fn gated(send: &mut (dyn FnMut(LogoProgressView) + Send)) -> impl FnMut(LogoProgress) + '_ {
    let started = Instant::now();
    let mut gate = ProgressGate::new();
    move |progress| {
        let now_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        if let Some(view) = gate.offer(progress, now_ms) {
            send(view);
        }
    }
}

/// The blob a logo file encodes to: PNG, JPEG, BMP or a raw `.bin` logo file.
pub fn blob_from_file(path: &Path, alpha: bool) -> Result<LogoBlob, UiError> {
    let name = path.display();
    let img = logo::load_file(path).map_err(|e| UiError::usage(format!("{name}: {e}")))?;
    let mode = if alpha {
        LogoMode::Alpha
    } else {
        LogoMode::Opaque
    };
    logo::encode(&img, mode).map_err(|e| UiError::usage(format!("{name}: {e}")))
}

/// The preview of the blob [`blob_from_file`] makes.
pub fn preview_file(path: &Path, alpha: bool) -> Result<LogoPreviewView, UiError> {
    let blob = blob_from_file(path, alpha)?;
    let p = blob.preview();
    let pixels = logo::decode(&blob.bytes)
        .map_err(|e| UiError::usage(format!("{}: {e}", path.display())))?;
    Ok(LogoPreviewView {
        width: p.width,
        height: p.height,
        mode: mode_name(p.mode).to_owned(),
        blob_size: p.blob_size,
        padded_size: p.padded_size,
        chunks: p.chunks,
        small_side_warning: p.small_side_warning,
        rgba: base64(&pixels.px),
    })
}

/// The mode as the UI names it.
fn mode_name(mode: LogoMode) -> &'static str {
    match mode {
        LogoMode::Opaque => "opaque",
        LogoMode::Alpha => "alpha",
    }
}

/// Standard base64 with padding.
fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let digit = |v: u32| char::from(A[(v & 63) as usize]);
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for group in bytes.chunks(3) {
        let b = [
            group.first().copied().unwrap_or(0),
            group.get(1).copied().unwrap_or(0),
            group.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(digit(n >> 18));
        out.push(digit(n >> 12));
        out.push(if group.len() > 1 { digit(n >> 6) } else { '=' });
        out.push(if group.len() > 2 { digit(n) } else { '=' });
    }
    out
}

impl From<LogoWritten> for LogoWrittenView {
    fn from(w: LogoWritten) -> Self {
        LogoWrittenView {
            startup_duration: match w.startup_duration {
                StartupLogo::Off => "off",
                StartupLogo::Secs1 => "1 s",
                StartupLogo::Secs3 => "3 s",
                StartupLogo::Secs5 => "5 s",
            }
            .to_owned(),
            startup_screen_off: w.startup_screen_off,
        }
    }
}

/// Authenticate (level 1) unless the session already is: every logo
/// command reads memory.
fn ensure_auth<T: Transport, C: Clock>(
    session: &mut Session<GuardedTransport<T>, C>,
) -> Result<(), powerz_protocol::session::Error> {
    if session.auth_level() < 1 {
        session.authenticate(1)?;
    }
    Ok(())
}

/// Authenticate and read the device model, the logo write's preconditions
/// besides the SETTINGS read, which the view must have done (the user saw
/// the settings the hint words).
fn prepare_write<T: Transport, C: Clock>(
    session: &mut Session<GuardedTransport<T>, C>,
) -> Result<(), powerz_protocol::session::Error> {
    ensure_auth(session)?;
    session.device_info()?;
    Ok(())
}

impl<T: Transport, C: Clock + Clone> Worker<T, C> {
    /// Upload `blob` (chunks, flag, full read-back).
    pub fn logo_upload(
        &mut self,
        blob: &LogoBlob,
        progress: &mut dyn FnMut(LogoProgress),
    ) -> Result<LogoWrittenView, UiError> {
        self.with_queue_session(|session| {
            prepare_write(session)?;
            session.logo_upload(blob, progress)
        })
        .map(LogoWrittenView::from)
    }

    /// Remove the logo (the built-in one shows again).
    pub fn logo_remove(
        &mut self,
        progress: &mut dyn FnMut(LogoProgress),
    ) -> Result<LogoWrittenView, UiError> {
        self.with_queue_session(|session| {
            prepare_write(session)?;
            session.logo_remove(progress)
        })
        .map(LogoWrittenView::from)
    }

    /// What the logo area holds; with `full`, the stored image too.
    pub fn logo_read(&mut self, full: bool) -> Result<LogoReadView, UiError> {
        self.with_queue_session(|session| {
            ensure_auth(session)?;
            let view = match session.logo_read()? {
                LogoStatus::Empty => empty_read("empty"),
                LogoStatus::Unrecognised => empty_read("unrecognised"),
                LogoStatus::Logo {
                    width,
                    height,
                    mode,
                } => LogoReadView {
                    status: "logo".to_owned(),
                    width: Some(width),
                    height: Some(height),
                    mode: Some(mode_name(mode).to_owned()),
                    image: if full {
                        Some(base64(&session.logo_read_full()?.px))
                    } else {
                        None
                    },
                },
            };
            Ok(view)
        })
    }
}

/// A read with no logo in it.
fn empty_read(status: &str) -> LogoReadView {
    LogoReadView {
        status: status.to_owned(),
        width: None,
        height: None,
        mode: None,
        image: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_error::ErrorKind;
    use crate::worker::{BackendEvent, WorkerConfig};
    use powerz_protocol::decode::Rate;
    use powerz_protocol::logo::{self, LogoMode, Rgba};
    use powerz_protocol::session::LogoStep;
    use powerz_sim::{LogoFault, Persona, Scenario, SimClock, SimDevice};
    use std::path::PathBuf;

    // ---- helpers ----

    /// A fresh scratch folder for this test.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "open-powerz-logo-{}-{name}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
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

    fn write_bmp(dir: &Path, name: &str, w: u32, h: u32) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, bmp(w, h)).expect("write bmp");
        path
    }

    fn rgba(w: u32, h: u32) -> Rgba {
        let mut px = Vec::new();
        for y in 0..h {
            for x in 0..w {
                px.extend([(x * 5) as u8, (y * 3) as u8, ((x + y) * 2) as u8, 255]);
            }
        }
        Rgba { w, h, px }
    }

    fn b64_decode(s: &str) -> Vec<u8> {
        const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = Vec::new();
        let mut buf = 0u32;
        let mut bits = 0;
        for c in s.bytes().filter(|&c| c != b'=') {
            let v = A.iter().position(|&a| a == c).expect("base64 digit") as u32;
            buf = (buf << 6) | v;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((buf >> bits) as u8);
            }
        }
        out
    }

    fn worker_on(device: SimDevice, clock: SimClock) -> Worker<SimDevice, SimClock> {
        let config = WorkerConfig::new(Rate::Sps50, 1);
        Worker::start(device, clock, config, |_event: BackendEvent| {})
            .unwrap_or_else(|e| panic!("the worker did not start: {e:?}"))
    }

    fn sim(clock: &SimClock) -> SimDevice {
        SimDevice::with_clock(Persona::Bulk, Scenario::Load5v1a, clock.clone())
    }

    /// A worker on the demo simulator that has read SETTINGS.
    fn ready(device: impl FnOnce(SimDevice) -> SimDevice) -> Worker<SimDevice, SimClock> {
        let clock = SimClock::new(1_790_000_000_000);
        let mut worker = worker_on(device(sim(&clock)), clock);
        worker.read_settings().expect("settings read");
        worker
    }

    fn blob_100x60() -> LogoBlob {
        logo::encode(&rgba(100, 60), LogoMode::Opaque).expect("encodes")
    }

    // ---- AC-MEHQC0.1: preview at the real pixel size, with its numbers ----

    #[test]
    fn mehqc0_preview_of_an_image_reports_size_blob_and_chunks() {
        let dir = scratch("preview");
        let path = write_bmp(&dir, "logo.bmp", 100, 60);
        let p = preview_file(&path, false).expect("previews");
        assert_eq!((p.width, p.height), (100, 60));
        assert_eq!(p.mode, "opaque");
        assert_eq!(p.blob_size, 16 + 100 * 60 * 2);
        assert_eq!(p.padded_size, 12_032);
        assert_eq!(p.chunks, 47);
        assert!(!p.small_side_warning);
    }

    #[test]
    fn mehqc0_preview_in_alpha_mode_uses_three_bytes_a_pixel_and_the_200_box() {
        let dir = scratch("alpha");
        let path = write_bmp(&dir, "big.bmp", 300, 300);
        let p = preview_file(&path, true).expect("previews");
        assert_eq!((p.width, p.height), (200, 200));
        assert_eq!(p.mode, "alpha");
        assert_eq!(p.blob_size, 120_016);
        assert_eq!(p.chunks, 469);
    }

    #[test]
    fn mehqc0_preview_warns_when_a_side_is_below_48() {
        let dir = scratch("small");
        let path = write_bmp(&dir, "wide.bmp", 400, 40);
        let p = preview_file(&path, false).expect("previews");
        assert_eq!((p.width, p.height), (240, 24));
        assert!(p.small_side_warning, "{p:?}");
    }

    #[test]
    fn mehqc0_preview_pixels_are_the_blob_that_would_be_written() {
        let dir = scratch("pixels");
        let path = write_bmp(&dir, "logo.bmp", 100, 60);
        let blob = blob_from_file(&path, false).expect("encodes");
        let p = preview_file(&path, false).expect("previews");
        let px = b64_decode(&p.rgba);
        assert_eq!(px.len(), 100 * 60 * 4);
        assert_eq!(px, logo::decode(&blob.bytes).expect("decodes").px);
    }

    #[test]
    fn mehqc0_preview_opens_a_raw_logo_file() {
        let dir = scratch("raw");
        let blob = logo::encode(&rgba(64, 48), LogoMode::Alpha).expect("encodes");
        let path = dir.join("logo.bin");
        std::fs::write(&path, &blob.bytes).expect("write bin");
        let p = preview_file(&path, true).expect("previews");
        assert_eq!((p.width, p.height), (64, 48));
        assert_eq!(p.mode, "alpha");
        assert_eq!(p.blob_size, blob.bytes.len());
    }

    #[test]
    fn mehqc0_a_file_that_is_no_image_is_a_usage_error() {
        let dir = scratch("junk");
        let path = dir.join("junk.png");
        std::fs::write(&path, b"not an image").expect("write");
        let err = preview_file(&path, false).expect_err("refused");
        assert_eq!(err.kind, ErrorKind::Usage);
        assert!(!err.message.is_empty() && err.message != "not implemented");
        let err = preview_file(&dir.join("missing.png"), false).expect_err("refused");
        assert_eq!(err.kind, ErrorKind::Usage);
        assert_ne!(err.message, "not implemented");
    }

    #[test]
    fn mehqc0_preview_serializes_for_the_ui() {
        let dir = scratch("json");
        let path = write_bmp(&dir, "logo.bmp", 100, 60);
        let p = preview_file(&path, false).expect("previews");
        let json = serde_json::to_value(&p).expect("json");
        for key in [
            "width",
            "height",
            "mode",
            "blob_size",
            "padded_size",
            "chunks",
            "small_side_warning",
            "rgba",
        ] {
            assert!(json.get(key).is_some(), "{key} missing in {json}");
        }
        assert_eq!(json["chunks"], 47);
    }

    // ---- AC-MEHQC0.3 / .4: upload and Remove on the device worker ----

    #[test]
    fn mehqc0_upload_on_the_worker_reports_chunks_then_read_back_and_verifies() {
        let mut worker = ready(|d| d);
        let blob = blob_100x60();
        let mut seen = Vec::new();
        let written = worker
            .logo_upload(&blob, &mut |p| seen.push(p))
            .expect("the upload verifies");
        let chunks: Vec<_> = seen.iter().filter(|p| p.step == LogoStep::Chunks).collect();
        assert_eq!(chunks.len(), 47);
        assert_eq!(chunks.last().map(|p| (p.done, p.total)), Some((47, 47)));
        let first_read = seen
            .iter()
            .position(|p| p.step == LogoStep::ReadBack)
            .expect("read-back progress");
        assert!(
            seen[..first_read]
                .iter()
                .all(|p| p.step == LogoStep::Chunks)
        );
        assert_eq!(written.startup_duration, "off");
        assert!(written.startup_screen_off);

        let read = worker.logo_read(false).expect("reads");
        assert_eq!(read.status, "logo");
        assert_eq!((read.width, read.height), (Some(100), Some(60)));
        assert_eq!(read.mode.as_deref(), Some("opaque"));
        assert_eq!(read.image, None);
    }

    #[test]
    fn mehqc0_upload_reports_the_startup_duration_when_it_is_on() {
        let mut worker = ready(|d| d.with_startup_logo(2));
        let written = worker
            .logo_upload(&blob_100x60(), &mut |_| {})
            .expect("verifies");
        assert_eq!(written.startup_duration, "3 s");
        assert!(!written.startup_screen_off);
    }

    #[test]
    fn mehqc0_a_failure_at_chunk_k_names_the_chunk_and_stops_there() {
        let mut worker = ready(|d| d.with_logo_fault(LogoFault::RejectAt(7)));
        let mut seen = Vec::new();
        let err = worker
            .logo_upload(&blob_100x60(), &mut |p| seen.push(p))
            .expect_err("chunk 7 fails");
        assert!(err.message.contains("chunk 7"), "{err:?}");
        assert!(
            seen.iter()
                .all(|p| p.step == LogoStep::Chunks && p.done <= 7),
            "{seen:?}"
        );
        // Nothing past the failed chunk: no read-back ran (every progress
        // step above is a chunk). Chunks 0..6 did land, so a Read would
        // honestly show their header; the flag is never sent on this path
        // (covered by the session's sim tests).
    }

    #[test]
    fn mehqc0_remove_brings_back_the_empty_area() {
        let mut worker = ready(|d| d);
        worker
            .logo_upload(&blob_100x60(), &mut |_| {})
            .expect("verifies");
        let mut seen = Vec::new();
        worker.logo_remove(&mut |p| seen.push(p)).expect("removes");
        assert!(
            seen.iter().any(|p| p.step == LogoStep::ReadBack),
            "{seen:?}"
        );
        let read = worker.logo_read(false).expect("reads");
        assert_eq!(read.status, "empty");
        assert_eq!((read.width, read.height, read.mode), (None, None, None));
    }

    // ---- AC-MEHQC0.6: refusals before anything is sent ----

    #[test]
    fn mehqc0_a_km002c_is_refused_naming_the_km003c() {
        let mut worker = ready(|d| d.with_model("KM002C"));
        let err = worker
            .logo_upload(&blob_100x60(), &mut |_| {})
            .expect_err("refused");
        assert!(err.message.contains("KM003C"), "{err:?}");
        let err = worker.logo_remove(&mut |_| {}).expect_err("refused");
        assert!(err.message.contains("KM003C"), "{err:?}");
        assert_eq!(worker.logo_read(false).expect("reads").status, "empty");
    }

    #[test]
    fn mehqc0_an_upload_before_the_settings_read_is_refused() {
        let clock = SimClock::new(1_790_000_000_000);
        let mut worker = worker_on(sim(&clock), clock);
        let mut seen = 0;
        let err = worker
            .logo_upload(&blob_100x60(), &mut |_| seen += 1)
            .expect_err("refused");
        assert!(err.message.to_lowercase().contains("settings"), "{err:?}");
        assert_eq!(seen, 0);
        assert_eq!(worker.logo_read(false).expect("reads").status, "empty");
    }

    // ---- AC-MEHQC0.7: Read shows what the meter holds ----

    #[test]
    fn mehqc0_read_of_a_fresh_meter_is_empty() {
        let mut worker = ready(|d| d);
        let read = worker.logo_read(false).expect("reads");
        assert_eq!(read.status, "empty");
    }

    #[test]
    fn mehqc0_read_of_junk_is_unrecognised() {
        let mut worker = ready(|d| d.with_logo_bytes(&[0x12; 64]));
        assert_eq!(
            worker.logo_read(false).expect("reads").status,
            "unrecognised"
        );
    }

    #[test]
    fn mehqc0_full_read_returns_the_stored_logo() {
        let mut worker = ready(|d| d);
        let blob = blob_100x60();
        worker.logo_upload(&blob, &mut |_| {}).expect("verifies");
        let read = worker.logo_read(true).expect("reads");
        let px = b64_decode(read.image.as_deref().expect("image"));
        assert_eq!(px, logo::decode(&blob.bytes).expect("decodes").px);
    }

    #[test]
    fn mehqc0_read_serializes_with_a_status() {
        let mut worker = ready(|d| d);
        let json = serde_json::to_value(worker.logo_read(false).expect("reads")).expect("json");
        assert_eq!(json["status"], "empty");
        assert!(json.get("image").is_some());
    }

    // ---- AC-MEHQC0.8: progress batched to at most 20 messages a second ----

    fn chunk(done: usize, total: usize) -> LogoProgress {
        LogoProgress {
            step: LogoStep::Chunks,
            done,
            total,
        }
    }

    #[test]
    fn mehqc0_the_gate_sends_at_most_one_message_per_50_ms() {
        let mut gate = ProgressGate::new();
        let mut sent = Vec::new();
        // 469 chunks one per ms, then 59 read-back reads one per ms.
        for k in 1..=469 {
            if let Some(m) = gate.offer(chunk(k, 469), k as u64) {
                sent.push((k as u64, m));
            }
        }
        for k in 1..=59 {
            let p = LogoProgress {
                step: LogoStep::ReadBack,
                done: k,
                total: 59,
            };
            if let Some(m) = gate.offer(p, 469 + k as u64) {
                sent.push((469 + k as u64, m));
            }
        }
        assert!(sent.len() >= 10, "too few messages: {}", sent.len());
        for pair in sent.windows(2) {
            assert!(
                pair[1].0 - pair[0].0 >= PROGRESS_GAP_MS,
                "{:?} then {:?}",
                pair[0],
                pair[1]
            );
        }
        for second in 0..2u64 {
            let n = sent
                .iter()
                .filter(|(t, _)| (second * 1000..(second + 1) * 1000).contains(t))
                .count();
            assert!(n <= 20, "{n} messages in second {second}");
        }
        assert_eq!(
            sent.first().map(|(_, m)| (m.step.as_str(), m.done)),
            Some(("chunks", 1))
        );
        assert!(sent.iter().any(|(_, m)| m.step == "read_back"));
    }

    #[test]
    fn mehqc0_the_gate_lets_slow_progress_through_whole() {
        let mut gate = ProgressGate::new();
        let sent: Vec<_> = (1..=10)
            .filter_map(|k| gate.offer(chunk(k, 10), k as u64 * 100))
            .collect();
        assert_eq!(sent.len(), 10);
        assert_eq!(
            sent.last(),
            Some(&LogoProgressView {
                step: "chunks".into(),
                done: 10,
                total: 10
            })
        );
    }

    #[test]
    fn mehqc0_progress_serializes_with_step_done_and_total() {
        let mut gate = ProgressGate::new();
        let m = gate.offer(chunk(1, 47), 0).expect("first is sent");
        assert_eq!(
            serde_json::to_value(&m).expect("json"),
            serde_json::json!({"step": "chunks", "done": 1, "total": 47})
        );
    }

    #[test]
    fn mehqc0_the_logo_commands_are_registered() {
        let source = include_str!("lib.rs");
        let handlers = source
            .split("generate_handler![")
            .nth(1)
            .and_then(|rest| rest.split(']').next())
            .expect("the handler list");
        for name in [
            "logo_choose",
            "logo_preview",
            "logo_read",
            "logo_upload",
            "logo_remove",
        ] {
            assert!(
                handlers.split(',').any(|h| h.trim() == name),
                "{name} is not registered"
            );
        }
    }
}
