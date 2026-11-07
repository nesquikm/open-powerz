//! Startup logo commands for the desktop app (FR MEHQC0): preview a picture
//! as the exact blob that would be written, and upload, Remove or read the
//! meter's logo on the device worker, with progress batched for the UI.

use std::path::Path;
use std::time::Instant;

use powerz_protocol::decode::settings::StartupLogo;
use powerz_protocol::logo::builtin::BuiltinLogo;
use powerz_protocol::logo::{self, LogoBlob, LogoMode};
use powerz_protocol::session::{
    Clock, LogoProgress, LogoStatus, LogoStep, LogoWritten, Session, SettingsSave,
};
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

/// What an upload or Remove left on the meter (FR QG6RNY).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LogoWrittenView {
    /// The startup-screen duration before the write, as the settings panel words it
    /// (`off`, `1 s`, …).
    pub duration_before: String,
    /// The startup-screen duration the write set.
    pub duration_set: String,
    /// The fresh settings read after the write: `"saved"`, `"needs_menu_save"` or
    /// `"calibration_changed"`.
    pub settings: String,
    /// The fresh read's duration when `settings` is `"saved"`, else null.
    pub startup_duration: Option<String>,
}

/// One read-only check of the settings after a logo write (FR QG6RNY).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LogoSettingsCheckView {
    /// `"saved"`, `"needs_menu_save"` or `"calibration_changed"`.
    pub settings: String,
    /// The duration read when `settings` is `"saved"`, else null.
    pub startup_duration: Option<String>,
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

/// The blob a logo source encodes to: exactly one of a file `path` (opaque, or
/// transparent with `alpha`) and a `builtin` logo name (always transparent,
/// FR 70X5YX).
pub fn blob_from_source(
    path: Option<&str>,
    builtin: Option<&str>,
    alpha: bool,
) -> Result<LogoBlob, UiError> {
    match (path, builtin) {
        (Some(path), None) => blob_from_file(Path::new(path), alpha),
        (None, Some(name)) => blob_from_builtin(name),
        _ => Err(one_source_error()),
    }
}

/// The preview of the blob [`blob_from_source`] makes.
pub fn preview_source(
    path: Option<&str>,
    builtin: Option<&str>,
    alpha: bool,
) -> Result<LogoPreviewView, UiError> {
    match (path, builtin) {
        (Some(path), None) => preview_file(Path::new(path), alpha),
        (None, Some(name)) => preview_blob(&blob_from_builtin(name)?)
            .map_err(|e| UiError::usage(format!("{name}: {e}"))),
        _ => Err(one_source_error()),
    }
}

/// The blob of built-in logo `name` (always transparent), or a usage error
/// listing the names.
fn blob_from_builtin(name: &str) -> Result<LogoBlob, UiError> {
    let b = name
        .parse::<BuiltinLogo>()
        .map_err(|e| UiError::usage(e.to_string()))?;
    b.encode().map_err(|e| UiError::usage(format!("{b}: {e}")))
}

/// The error for both or neither of a file and a built-in logo.
fn one_source_error() -> UiError {
    UiError::usage("give exactly one of a logo file and a built-in logo")
}

/// The preview view of `blob`, its pixels decoded back.
fn preview_blob(blob: &LogoBlob) -> Result<LogoPreviewView, logo::LogoError> {
    let p = blob.preview();
    let pixels = logo::decode(&blob.bytes)?;
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

/// A duration as the settings panel words it.
fn duration_name(d: StartupLogo) -> String {
    d.label().to_owned()
}

/// The settings state as the UI names it, and the duration read when saved.
fn settings_state(save: SettingsSave) -> (String, Option<String>) {
    match save {
        SettingsSave::Saved(d) => ("saved".to_owned(), Some(duration_name(d))),
        SettingsSave::NeedsMenuSave => ("needs_menu_save".to_owned(), None),
        SettingsSave::CalibrationChanged => ("calibration_changed".to_owned(), None),
    }
}

impl From<LogoWritten> for LogoWrittenView {
    fn from(w: LogoWritten) -> Self {
        let (settings, startup_duration) = settings_state(w.settings_after);
        LogoWrittenView {
            duration_before: duration_name(w.duration_before),
            duration_set: duration_name(w.duration_set),
            settings,
            startup_duration,
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
    /// Upload `blob` (chunks, startup-duration write, full read-back).
    pub fn logo_upload(
        &mut self,
        blob: &LogoBlob,
        progress: &mut dyn FnMut(LogoProgress),
    ) -> Result<LogoWrittenView, UiError> {
        let written = self.with_queue_session(|session| {
            prepare_write(session)?;
            session.logo_upload(blob, progress)
        })?;
        Ok(self.keep_pending(written))
    }

    /// Remove the logo (the built-in one shows again).
    pub fn logo_remove(
        &mut self,
        progress: &mut dyn FnMut(LogoProgress),
    ) -> Result<LogoWrittenView, UiError> {
        let written = self.with_queue_session(|session| {
            prepare_write(session)?;
            session.logo_remove(progress)
        })?;
        Ok(self.keep_pending(written))
    }

    /// One read-only check of the settings against the last logo write's
    /// calibration snapshot (FR QG6RNY).
    /// `saved` and `calibration_changed` end the wait.
    pub fn logo_settings_check(&mut self) -> Result<LogoSettingsCheckView, UiError> {
        let Some(snapshot) = self.logo_pending else {
            return Err(UiError::usage(
                "No logo write is waiting for a settings save.",
            ));
        };
        let save = self.with_queue_session(|session| session.check_settings_saved(&snapshot))?;
        if save != SettingsSave::NeedsMenuSave {
            self.logo_pending = None;
        }
        let (settings, startup_duration) = settings_state(save);
        Ok(LogoSettingsCheckView {
            settings,
            startup_duration,
        })
    }

    /// Keep the write's calibration snapshot while its settings wait for a
    /// menu save, for [`Worker::logo_settings_check`].
    fn keep_pending(&mut self, written: LogoWritten) -> LogoWrittenView {
        self.logo_pending = match written.settings_after {
            SettingsSave::NeedsMenuSave => Some(written.calibration),
            _ => None,
        };
        LogoWrittenView::from(written)
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
        assert_eq!(written.duration_before, "off");
        assert_eq!(written.duration_set, "3 s");

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
        assert_eq!(written.duration_before, "3 s");
        assert_eq!(written.duration_set, "3 s");
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
        // honestly show their header; the startup-duration write is never sent on this path
        // (covered by the session's sim tests).
    }

    #[test]
    fn mehqc0_remove_brings_back_the_empty_area() {
        // The upload leaves the settings unsaved until a menu save on the
        // meter (FR Z39H24); the sim does it at once, so the Remove is allowed.
        let mut worker = ready(|d| d.with_auto_menu_save(std::time::Duration::ZERO, 2));
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

    // ---- FR QG6RNY: the write result, the settings check, the demo menu save ----

    /// A worker on a sim with `clock`, its settings read, and a twin that
    /// shares the sim's stored settings (for a menu save from the test).
    fn ready_with_twin(
        clock: &SimClock,
        device: impl FnOnce(SimDevice) -> SimDevice,
    ) -> (Worker<SimDevice, SimClock>, SimDevice) {
        let dev = device(sim(clock));
        let twin = dev.cdc_twin();
        let mut worker = worker_on(dev, clock.clone());
        worker.read_settings().expect("settings read");
        (worker, twin)
    }

    fn json<T: Serialize>(v: &T) -> serde_json::Value {
        serde_json::to_value(v).expect("json")
    }

    // AC-QG6RNY.1 (backend side): the result says the duration before and the one set.
    #[test]
    fn qg6rny_upload_reports_before_set_and_a_needed_menu_save() {
        let mut worker = ready(|d| d);
        let written = worker
            .logo_upload(&blob_100x60(), &mut |_| {})
            .expect("verifies");
        assert_eq!(written.duration_before, "off");
        assert_eq!(written.duration_set, "3 s");
        assert_eq!(written.startup_duration, None);
        let v = json(&written);
        assert_eq!(v["settings"], "needs_menu_save");
        assert_eq!(v["startup_duration"], serde_json::Value::Null);
        let mut keys: Vec<String> = v.as_object().expect("object").keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "duration_before",
                "duration_set",
                "settings",
                "startup_duration"
            ]
        );
    }

    #[test]
    fn qg6rny_remove_reports_the_startup_screen_off() {
        let mut worker = ready(|d| d.with_startup_logo(3));
        let written = worker.logo_remove(&mut |_| {}).expect("removes");
        assert_eq!(written.duration_before, "5 s");
        assert_eq!(written.duration_set, "off");
        assert_eq!(json(&written)["settings"], "needs_menu_save");
    }

    // AC-QG6RNY.4 (backend side): a fresh read that is already saved carries its duration.
    #[test]
    fn qg6rny_a_saved_fresh_read_carries_its_duration() {
        let w = LogoWritten {
            duration_before: StartupLogo::Off,
            duration_set: StartupLogo::Secs3,
            settings_after: SettingsSave::Saved(StartupLogo::Secs5),
            calibration: powerz_protocol::decode::settings::CalibrationSnapshot([0; 64]),
        };
        let view = LogoWrittenView::from(w);
        assert_eq!(view.duration_before, "off");
        assert_eq!(view.duration_set, "3 s");
        assert_eq!(view.startup_duration.as_deref(), Some("5 s"));
        assert_eq!(json(&view)["settings"], "saved");
    }

    #[test]
    fn qg6rny_a_fresh_read_with_other_calibration_says_so() {
        let w = LogoWritten {
            duration_before: StartupLogo::Secs1,
            duration_set: StartupLogo::Secs3,
            settings_after: SettingsSave::CalibrationChanged,
            calibration: powerz_protocol::decode::settings::CalibrationSnapshot([0; 64]),
        };
        let view = LogoWrittenView::from(w);
        assert_eq!(view.duration_before, "1 s");
        assert_eq!(view.startup_duration, None);
        assert_eq!(json(&view)["settings"], "calibration_changed");
    }

    // AC-QG6RNY.2 / .3 (backend side): one read-only check per call against the snapshot.
    #[test]
    fn qg6rny_a_check_without_a_pending_logo_write_is_a_usage_error() {
        let mut worker = ready(|d| d);
        let err = worker.logo_settings_check().expect_err("nothing to check");
        assert_eq!(err.kind, ErrorKind::Usage);
        assert_eq!(err.message, "No logo write is waiting for a settings save.");
    }

    #[test]
    fn qg6rny_the_check_says_not_saved_until_the_menu_save_then_saved() {
        let clock = SimClock::new(1_790_000_000_000);
        let (mut worker, mut twin) = ready_with_twin(&clock, |d| d);
        worker
            .logo_upload(&blob_100x60(), &mut |_| {})
            .expect("verifies");
        for _ in 0..3 {
            let check = worker.logo_settings_check().expect("checks");
            assert_eq!(json(&check)["settings"], "needs_menu_save");
            assert_eq!(check.startup_duration, None);
        }
        // The checks are read-only: the block is still the stale one.
        assert!(twin.settings_crc_a_stale());
        twin.menu_save(3);
        let check = worker.logo_settings_check().expect("checks");
        assert_eq!(json(&check)["settings"], "saved");
        assert_eq!(check.startup_duration.as_deref(), Some("5 s"));
        // Saved clears the pending write.
        let err = worker.logo_settings_check().expect_err("cleared");
        assert_eq!(err.kind, ErrorKind::Usage);
        // The meter's settings read again fine.
        worker
            .read_settings()
            .expect("settings read after the save");
    }

    #[test]
    fn qg6rny_the_check_follows_an_automatic_menu_save_on_the_sim() {
        let clock = SimClock::new(1_790_000_000_000);
        let (mut worker, _twin) = ready_with_twin(&clock, |d| {
            d.with_auto_menu_save(std::time::Duration::from_secs(4), 2)
        });
        let written = worker
            .logo_upload(&blob_100x60(), &mut |_| {})
            .expect("verifies");
        assert_eq!(json(&written)["settings"], "needs_menu_save");
        assert_eq!(
            json(&worker.logo_settings_check().expect("checks"))["settings"],
            "needs_menu_save"
        );
        clock.advance(std::time::Duration::from_secs(5));
        let check = worker.logo_settings_check().expect("checks");
        assert_eq!(json(&check)["settings"], "saved");
        assert_eq!(check.startup_duration.as_deref(), Some("3 s"));
    }

    #[test]
    fn qg6rny_the_check_view_serializes_settings_and_duration() {
        let clock = SimClock::new(1_790_000_000_000);
        let (mut worker, _twin) = ready_with_twin(&clock, |d| d);
        worker
            .logo_upload(&blob_100x60(), &mut |_| {})
            .expect("verifies");
        let v = json(&worker.logo_settings_check().expect("checks"));
        let mut keys: Vec<String> = v.as_object().expect("object").keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["settings", "startup_duration"]);
    }

    // AC-QG6RNY.6 (backend side): reading or changing settings while stale.
    #[test]
    fn qg6rny_reading_settings_while_a_block_is_stale_gives_the_steps() {
        use powerz_protocol::decode::settings::MENU_SAVE_STEPS;
        let clock = SimClock::new(1_790_000_000_000);
        let (mut worker, twin) = ready_with_twin(&clock, |d| d);
        worker
            .logo_upload(&blob_100x60(), &mut |_| {})
            .expect("verifies");
        let err = worker.read_settings().expect_err("stale");
        assert_eq!(err.kind, ErrorKind::SettingsNotSaved);
        assert_eq!(
            err.message,
            "The meter has not saved its settings since the last change."
        );
        assert_eq!(err.hint.as_deref(), Some(MENU_SAVE_STEPS));
        assert_eq!(json(&err)["kind"], "settings_not_saved");
        assert!(twin.settings_crc_a_stale());
    }

    #[test]
    fn qg6rny_changing_settings_while_a_block_is_stale_is_refused_with_the_steps() {
        use powerz_protocol::decode::settings::MENU_SAVE_STEPS;
        let clock = SimClock::new(1_790_000_000_000);
        let (mut worker, twin) = ready_with_twin(&clock, |d| d);
        worker
            .logo_upload(&blob_100x60(), &mut |_| {})
            .expect("verifies");
        let changes = std::collections::BTreeMap::from([("brightness".to_owned(), 3)]);
        let err = worker.write_settings(&changes).expect_err("refused");
        assert_eq!(err.kind, ErrorKind::SettingsNotSaved);
        assert_eq!(err.hint.as_deref(), Some(MENU_SAVE_STEPS));
        // Still the stale block: nothing was saved over it.
        assert!(twin.settings_crc_a_stale());
    }

    // AC-QG6RNY.5 / registration: the check is a Tauri command like the other logo commands.
    #[test]
    fn qg6rny_the_settings_check_command_is_registered() {
        let source = include_str!("lib.rs");
        let handlers = source
            .split("generate_handler![")
            .nth(1)
            .and_then(|rest| rest.split(']').next())
            .expect("the handler list");
        assert!(
            handlers
                .split(',')
                .any(|h| h.trim() == "logo_settings_check"),
            "logo_settings_check is not registered"
        );
    }

    // AC-QG6RNY.8: the demo meter saves its settings by itself after a logo write.
    #[test]
    fn qg6rny_the_demo_meter_does_its_own_menu_save() {
        use crate::devices::{AppOpener, DEMO_MENU_SAVE_AFTER, Opener, list_devices_from};
        use powerz_protocol::session::SystemClock;
        let listing = list_devices_from(Ok(Vec::new()))
            .into_iter()
            .find(|d| d.demo_scenario.as_deref() == Some("load-5v-1a"))
            .expect("the demo listing");
        let transport = AppOpener.open(&listing).expect("the demo opens");
        let config = WorkerConfig::new(Rate::Sps50, 1);
        let mut worker = Worker::start(transport, SystemClock, config, |_e: BackendEvent| {})
            .unwrap_or_else(|e| panic!("the worker did not start: {e:?}"));
        worker.read_settings().expect("settings read");
        let upload_started = std::time::Instant::now();
        let written = worker
            .logo_upload(&blob_100x60(), &mut |_| {})
            .expect("verifies");
        let upload_took = upload_started.elapsed();
        // The save timer starts at the 0x1C write, inside the upload. A slow
        // runner can spend longer than the delay on the chunks and read-back,
        // so "saved" right after the upload is fine, but never sooner than the
        // delay (FR 30GN6B).
        let duration = match json(&written)["settings"].as_str() {
            Some("saved") => {
                assert!(
                    upload_took >= DEMO_MENU_SAVE_AFTER,
                    "saved {upload_took:?} after the upload began, before the demo's delay"
                );
                written.startup_duration
            }
            Some("needs_menu_save") => {
                let started = std::time::Instant::now();
                loop {
                    let check = worker.logo_settings_check().expect("checks");
                    if json(&check)["settings"] == "saved" {
                        break check.startup_duration;
                    }
                    assert_eq!(json(&check)["settings"], "needs_menu_save");
                    assert!(
                        started.elapsed() < std::time::Duration::from_secs(15),
                        "the demo meter never saved its settings"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(250));
                }
            }
            other => panic!("unexpected settings state after the upload: {other:?}"),
        };
        assert_eq!(duration.as_deref(), Some("3 s"));
    }

    // ---- FR 70X5YX: the built-in open-powerz logos ----
    //
    // `preview_source(path, builtin, alpha)` and `blob_from_source(path,
    // builtin, alpha)` take exactly one of a file path and a built-in name
    // (`surf` or `surf-wordmark`); a built-in is always the transparent mode.

    use powerz_protocol::logo::builtin::BuiltinLogo;

    fn builtin_blob(b: BuiltinLogo) -> LogoBlob {
        b.encode().expect("the built-in logo encodes")
    }

    // AC-70X5YX.5: picking a built-in shows its preview and sizes as a file does.
    #[test]
    fn _70x5yx_5_preview_of_a_builtin_reports_200x200_alpha_and_469_chunks() {
        for (name, b) in [
            ("surf", BuiltinLogo::Surf),
            ("surf-wordmark", BuiltinLogo::SurfWordmark),
        ] {
            let p = preview_source(None, Some(name), false).expect("previews");
            assert_eq!((p.width, p.height), (200, 200), "{name}");
            assert_eq!(p.mode, "alpha", "{name}");
            assert_eq!(p.blob_size, 120_016, "{name}");
            assert_eq!(p.padded_size, 120_064, "{name}");
            assert_eq!(p.chunks, 469, "{name}");
            assert!(!p.small_side_warning, "{name}");
            let px = b64_decode(&p.rgba);
            assert_eq!(
                px,
                logo::decode(&builtin_blob(b).bytes).expect("decodes").px,
                "{name}: the preview pixels are the blob that would be written"
            );
        }
    }

    #[test]
    fn _70x5yx_5_a_file_source_previews_as_before() {
        let dir = scratch("70x5yx-file");
        let path = write_bmp(&dir, "logo.bmp", 100, 60);
        let text = path.to_string_lossy().into_owned();
        let p = preview_source(Some(text.as_str()), None, false).expect("previews");
        assert_eq!(p, preview_file(&path, false).expect("previews"));
        let blob = blob_from_source(Some(text.as_str()), None, true).expect("encodes");
        assert_eq!(blob, blob_from_file(&path, true).expect("encodes"));
    }

    #[test]
    fn _70x5yx_5_the_blob_of_a_builtin_is_its_encoding() {
        for (name, b) in [
            ("surf", BuiltinLogo::Surf),
            ("surf-wordmark", BuiltinLogo::SurfWordmark),
        ] {
            let blob = blob_from_source(None, Some(name), false).expect("encodes");
            assert_eq!(blob, builtin_blob(b), "{name}");
        }
    }

    #[test]
    fn _70x5yx_5_upload_of_a_builtin_on_the_worker_reports_progress_and_verifies() {
        let mut worker = ready(|d| d);
        let blob = blob_from_source(None, Some("surf"), false).expect("encodes");
        let mut seen = Vec::new();
        let written = worker
            .logo_upload(&blob, &mut |p| seen.push(p))
            .expect("the upload verifies");
        let chunks: Vec<_> = seen.iter().filter(|p| p.step == LogoStep::Chunks).collect();
        assert_eq!(chunks.last().map(|p| (p.done, p.total)), Some((469, 469)));
        assert!(seen.iter().any(|p| p.step == LogoStep::ReadBack));
        assert_eq!(written.duration_before, "off");
        assert_eq!(written.duration_set, "3 s");
        assert_eq!(json(&written)["settings"], "needs_menu_save");
        let read = worker.logo_read(true).expect("reads");
        assert_eq!(read.status, "logo");
        assert_eq!((read.width, read.height), (Some(200), Some(200)));
        assert_eq!(read.mode.as_deref(), Some("alpha"));
    }

    // AC-70X5YX.6: a built-in always uses the transparent mode.
    #[test]
    fn _70x5yx_6_the_alpha_flag_changes_nothing_for_a_builtin() {
        for name in ["surf", "surf-wordmark"] {
            assert_eq!(
                preview_source(None, Some(name), false).expect("previews"),
                preview_source(None, Some(name), true).expect("previews"),
                "{name}"
            );
            assert_eq!(
                blob_from_source(None, Some(name), false).expect("encodes"),
                blob_from_source(None, Some(name), true).expect("encodes"),
                "{name}"
            );
        }
    }

    #[test]
    fn _70x5yx_5_an_unknown_builtin_is_a_usage_error_listing_the_names() {
        for result in [
            preview_source(None, Some("nope"), true).map(|_| ()),
            blob_from_source(None, Some("nope"), true).map(|_| ()),
        ] {
            let err = result.expect_err("refused");
            assert_eq!(err.kind, ErrorKind::Usage);
            assert!(err.message.contains("surf-wordmark"), "{}", err.message);
            assert!(
                err.message.replace("surf-wordmark", "").contains("surf"),
                "{}",
                err.message
            );
        }
    }

    #[test]
    fn _70x5yx_5_a_path_and_a_builtin_together_or_neither_is_a_usage_error() {
        let dir = scratch("70x5yx-both");
        let path = write_bmp(&dir, "logo.bmp", 100, 60);
        let path = path.to_string_lossy();
        for (p, b) in [(Some(&*path), Some("surf")), (None, None)] {
            let err = preview_source(p, b, false).expect_err("refused");
            assert_eq!(err.kind, ErrorKind::Usage, "{p:?} {b:?}");
            assert_ne!(err.message, "not implemented");
            let err = blob_from_source(p, b, false).expect_err("refused");
            assert_eq!(err.kind, ErrorKind::Usage, "{p:?} {b:?}");
        }
    }

    #[test]
    fn _70x5yx_5_the_preview_and_upload_commands_take_an_optional_builtin() {
        let source = include_str!("lib.rs");
        for cmd in ["async fn logo_preview(", "async fn logo_upload("] {
            let sig = source
                .split(cmd)
                .nth(1)
                .and_then(|rest| rest.split(')').next())
                .unwrap_or_else(|| panic!("no {cmd}"));
            assert!(sig.contains("path: Option<String>"), "{cmd}{sig})");
            assert!(sig.contains("builtin: Option<String>"), "{cmd}{sig})");
        }
    }

    // AC-70X5YX.7: the app's demo meter takes a built-in upload like a file upload.
    #[test]
    fn _70x5yx_7_the_demo_meter_accepts_a_builtin_upload() {
        use crate::devices::{AppOpener, Opener, list_devices_from};
        use powerz_protocol::session::SystemClock;
        let listing = list_devices_from(Ok(Vec::new()))
            .into_iter()
            .find(|d| d.demo_scenario.as_deref() == Some("load-5v-1a"))
            .expect("the demo listing");
        let transport = AppOpener.open(&listing).expect("the demo opens");
        let config = WorkerConfig::new(Rate::Sps50, 1);
        let mut worker = Worker::start(transport, SystemClock, config, |_e: BackendEvent| {})
            .unwrap_or_else(|e| panic!("the worker did not start: {e:?}"));
        worker.read_settings().expect("settings read");
        let blob = blob_from_source(None, Some("surf-wordmark"), false).expect("encodes");
        let written = worker.logo_upload(&blob, &mut |_| {}).expect("verifies");
        assert_eq!(written.duration_set, "3 s");
        let read = worker.logo_read(false).expect("reads");
        assert_eq!(read.status, "logo");
        assert_eq!((read.width, read.height), (Some(200), Some(200)));
        assert_eq!(read.mode.as_deref(), Some("alpha"));
    }
}
