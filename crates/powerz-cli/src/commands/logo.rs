//! `powerz logo` (FR YCRQRQ).

use std::io::{BufRead, Write};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use powerz_protocol::decode::settings::MENU_SAVE_STEPS;
use powerz_protocol::logo::builtin::BuiltinLogo;
use powerz_protocol::logo::{self, CHUNK, LogoBlob, LogoMode, MIN_SIDE};
use powerz_protocol::session::{
    Clock, LogoProgress, LogoStatus, LogoStep, LogoWritten, SETTINGS_SAVE_POLL_GAP, Session,
    SettingsSave, check_logo_dump,
};
use powerz_protocol::transport::Transport;

use super::records::confirm_typed;
use crate::error::CliError;

/// What `--i-understand` acknowledges for `logo write`.
pub const LOGO_WRITE_RISK: &str = "writing the startup logo rewrites the meter's flash; \
     pass --i-understand to write it";

/// What `--i-understand` acknowledges for `logo remove`.
pub const LOGO_REMOVE_RISK: &str = "removing the startup logo rewrites the meter's flash; \
     pass --i-understand to remove it";

/// Where a logo image comes from: a file or a built-in logo (FR 70X5YX).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogoSource<'a> {
    /// An image file (PNG, JPEG, BMP or raw .bin).
    File(&'a Path),
    /// One of the embedded brand logos, always in the transparent mode.
    Builtin(BuiltinLogo),
}

impl<'a> LogoSource<'a> {
    /// The source an `IMG` argument and a `--builtin NAME` option give
    /// (clap keeps them exclusive); neither is a usage error for `command`.
    pub fn from_args(
        file: Option<&'a Path>,
        builtin: Option<BuiltinLogo>,
        command: &str,
    ) -> Result<Self, CliError> {
        match (builtin, file) {
            (Some(b), _) => Ok(LogoSource::Builtin(b)),
            (None, Some(f)) => Ok(LogoSource::File(f)),
            (None, None) => Err(CliError::Usage(format!(
                "logo {command} needs IMG or --builtin"
            ))),
        }
    }

    /// The source's name for messages, its image and its blob: a file in
    /// the opaque or (`alpha`) alpha mode, a built-in logo always in its own.
    fn encode(self, alpha: bool) -> Result<(String, logo::Rgba, LogoBlob), CliError> {
        match self {
            LogoSource::File(file) => {
                let (img, blob) = encode_file(file, alpha)?;
                Ok((file.display().to_string(), img, blob))
            }
            LogoSource::Builtin(b) => {
                let (img, blob) = encode_builtin(b)?;
                Ok((b.name().to_owned(), img, blob))
            }
        }
    }
}

/// `powerz logo preview IMG`: encode the image as the meter would store it
/// and print the final size, mode, blob size, padded size and chunk count;
/// warn on `warn` when a side ends up below [`MIN_SIDE`]. With `output`,
/// the decoded logo is written there as a PNG. Opens no device.
pub fn logo_preview(
    source: LogoSource<'_>,
    alpha: bool,
    output: Option<&Path>,
    out: &mut dyn Write,
    warn: &mut dyn Write,
) -> Result<(), CliError> {
    let (name, img, blob) = source.encode(alpha)?;
    let p = blob.preview();
    writeln!(out, "Input:   {}x{}", img.w, img.h)?;
    writeln!(out, "Size:    {}x{}", p.width, p.height)?;
    writeln!(out, "Mode:    {}", mode_label(p.mode))?;
    writeln!(out, "Blob:    {} bytes", p.blob_size)?;
    writeln!(out, "Padded:  {} bytes", p.padded_size)?;
    writeln!(out, "Chunks:  {} ({CHUNK} bytes each)", p.chunks)?;
    if p.small_side_warning {
        writeln!(
            warn,
            "warning: {}x{} has a side below {MIN_SIDE} pixels; the logo may be hard to see",
            p.width, p.height
        )?;
    }
    if let Some(path) = output {
        let decoded =
            logo::decode(&blob.bytes).map_err(|e| CliError::Device(format!("{name}: {e}")))?;
        save_png(path, &decoded)?;
        writeln!(out, "Preview: {}", path.display())?;
    }
    Ok(())
}

/// `powerz logo read`: print whether the meter's logo area is empty, holds
/// a logo (its size and mode) or holds unrecognised data. With `output`, the
/// whole logo is read and written there as a PNG. Only memory reads are
/// sent, after authenticating as the other memory reads do.
pub fn logo_read<T: Transport, C: Clock>(
    session: &mut Session<T, C>,
    output: Option<&Path>,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    ensure_auth(session)?;
    match session.logo_read()? {
        LogoStatus::Empty => writeln!(out, "Logo:    empty (no startup logo stored)")?,
        LogoStatus::Logo {
            width,
            height,
            mode,
        } => {
            writeln!(out, "Logo:    {width}x{height}")?;
            writeln!(out, "Mode:    {}", mode_label(mode))?;
        }
        LogoStatus::Unrecognised => {
            writeln!(out, "Logo:    unrecognised data in the logo area")?;
        }
    }
    if let Some(path) = output {
        save_png(path, &session.logo_read_full()?)?;
        writeln!(out, "Saved:   {}", path.display())?;
    }
    Ok(())
}

/// What `logo read --dump` allows.
const DUMP_WINDOW: &str = "0x98300000–0x9831FFFF";

/// Bytes per `logo read --dump` line.
const DUMP_LINE: usize = 16;

/// Refuse a `logo read --dump ADDR LEN` span outside the logo window
/// (usage error); sends nothing, so it runs before a device is opened.
pub fn logo_dump_check(span: &[u32]) -> Result<(), CliError> {
    let [addr, len] = *span else {
        return Err(CliError::Usage("--dump takes ADDR and LEN".into()));
    };
    check_logo_dump(addr, len).map_err(|_| {
        CliError::Usage(format!(
            "--dump {addr:#x} {len}: ADDR and ADDR + LEN must lie inside the logo window \
             {DUMP_WINDOW}, with LEN at least 1"
        ))
    })
}

/// `powerz logo read --dump ADDR LEN`: authenticate, read LEN bytes of the
/// logo window from ADDR (at most 256 bytes per read, at least 50 ms apart)
/// and print them as `ADDRESS: bytes…` lines. Sends no write.
pub fn logo_dump<T: Transport, C: Clock>(
    session: &mut Session<T, C>,
    span: &[u32],
    out: &mut dyn Write,
) -> Result<(), CliError> {
    logo_dump_check(span)?;
    let (addr, len) = (span[0], span[1]);
    ensure_auth(session)?;
    let data = session.logo_dump(addr, len)?;
    for (k, line) in data.chunks(DUMP_LINE).enumerate() {
        let at = addr + (k * DUMP_LINE) as u32;
        let bytes: Vec<String> = line.iter().map(|b| format!("{b:02x}")).collect();
        writeln!(out, "{at:08x}: {}", bytes.join(" "))?;
    }
    Ok(())
}

/// Options of `powerz logo write`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LogoWriteOptions {
    /// `--alpha`: encode with an alpha channel instead of opaque RGB565.
    pub alpha: bool,
    /// `--i-understand` and `--yes`, as for `logo remove`.
    pub confirm: LogoConfirm,
    /// Refresh progress in place with `\r` (progress goes to a terminal).
    pub progress_tty: bool,
    /// Test-only: set the Ctrl-C flag once this many chunks have been sent.
    pub ctrl_c_after_chunks: Option<usize>,
    /// `--menu-save-timeout`: how long to wait for a save in the meter's
    /// menu when the write leaves its settings unsaved (zero: no wait).
    pub menu_save_timeout: Duration,
}

/// The acknowledgement flags shared by `logo write` and `logo remove`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LogoConfirm {
    /// `--i-understand`: the user acknowledged the flash write.
    pub i_understand: bool,
    /// `--yes`: skip the typed confirmation (still needs `--i-understand`).
    pub yes: bool,
}

/// `powerz logo write IMG|--builtin NAME`: refused without `--i-understand`;
/// encode the image (a built-in logo in its transparent mode), authenticate,
/// read SETTINGS and the model (the upload's preconditions), ask for
/// "write" to be typed unless `--yes`, then upload with per-chunk and
/// read-back progress on `progress` (refreshed in place with `\r` when
/// `opts.progress_tty`, one line per update otherwise, so a `--trace` on
/// the same stream stays line by line). Ends with the
/// verified logo and the startup duration from a fresh SETTINGS read on
/// `out`; when the meter left its settings unsaved, the menu-save steps go
/// to `progress` and the tool waits up to `opts.menu_save_timeout` for the
/// save (see `finish_written`).
///
/// `arm_ctrl_c` is called once the write is confirmed, just before the
/// first chunk, and returns the Ctrl-C flag. A Ctrl-C during the chunk
/// phase does not stop the write (a half-written logo is worse than a
/// finished one): one note says the write finishes first, and the tool
/// exits when it is done. A Ctrl-C also stops the wait for the menu save.
pub fn logo_write<T: Transport, C: Clock>(
    session: &mut Session<T, C>,
    source: LogoSource<'_>,
    opts: &LogoWriteOptions,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
    progress: &mut dyn Write,
    arm_ctrl_c: &mut dyn FnMut() -> Result<Arc<AtomicBool>, CliError>,
) -> Result<(), CliError> {
    if !opts.confirm.i_understand {
        return Err(CliError::Usage(LOGO_WRITE_RISK.into()));
    }
    let (_, _, blob) = source.encode(opts.alpha)?;
    let p = blob.preview();
    prepare_write(
        session,
        "write",
        "overwrite the meter's logo",
        "the upload sets it to 3 s",
        opts.confirm.yes,
        input,
        out,
    )?;
    let stop = arm_ctrl_c()?;
    let mut noted = false;
    let written = session.logo_upload(&blob, &mut |pr| {
        if matches!(pr.step, LogoStep::Chunks) {
            if opts.ctrl_c_after_chunks.is_some_and(|k| pr.done >= k) {
                stop.store(true, Ordering::SeqCst);
            }
            if !noted && stop.load(Ordering::SeqCst) {
                noted = true;
                let _ = writeln!(
                    progress,
                    "{}Ctrl-C: the logo write will finish first (stopping now would leave \
                     a half-written logo), then powerz exits",
                    if opts.progress_tty { "\n" } else { "" }
                );
            }
        }
        show_progress(progress, opts.progress_tty, pr)
    })?;
    writeln!(
        out,
        "Logo:    {}x{}, {}, written and verified",
        p.width,
        p.height,
        mode_label(p.mode)
    )?;
    finish_written(
        session,
        written,
        false,
        opts.menu_save_timeout,
        &stop,
        out,
        progress,
    )
}

/// `powerz logo remove`: the same rules as [`logo_write`] (refused without
/// `--i-understand`, "remove" typed unless `--yes`), then clear the logo
/// area, turn the startup screen off and read it back, ending with the
/// verified result and the same menu-save wait (`menu_save_timeout`,
/// stopped by the Ctrl-C flag `arm_ctrl_c` returns).
#[allow(clippy::too_many_arguments)]
pub fn logo_remove<T: Transport, C: Clock>(
    session: &mut Session<T, C>,
    confirm: LogoConfirm,
    progress_tty: bool,
    menu_save_timeout: Duration,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
    progress: &mut dyn Write,
    arm_ctrl_c: &mut dyn FnMut() -> Result<Arc<AtomicBool>, CliError>,
) -> Result<(), CliError> {
    if !confirm.i_understand {
        return Err(CliError::Usage(LOGO_REMOVE_RISK.into()));
    }
    prepare_write(
        session,
        "remove",
        "remove the meter's logo",
        "the remove turns the startup screen off",
        confirm.yes,
        input,
        out,
    )?;
    let stop = arm_ctrl_c()?;
    let written = session.logo_remove(&mut |pr| show_progress(progress, progress_tty, pr))?;
    writeln!(out, "Logo:    removed and verified (logo area empty)")?;
    finish_written(
        session,
        written,
        true,
        menu_save_timeout,
        &stop,
        out,
        progress,
    )
}

/// Authenticate at level 1 unless the session already is.
fn ensure_auth<T: Transport, C: Clock>(session: &mut Session<T, C>) -> Result<(), CliError> {
    if session.auth_level() < 1 {
        session.authenticate(1)?;
    }
    Ok(())
}

/// Write `img` to `path` as an RGBA PNG.
fn save_png(path: &Path, img: &logo::Rgba) -> Result<(), CliError> {
    image::save_buffer_with_format(
        path,
        &img.px,
        img.w,
        img.h,
        image::ExtendedColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .map_err(|e| CliError::Usage(format!("cannot write {}: {e}", path.display())))
}

/// Authenticate, read SETTINGS and the model (a logo write's
/// preconditions), print the startup duration now and `change` (what the
/// write sets it to), then ask for `word` to be typed unless `yes`.
fn prepare_write<T: Transport, C: Clock>(
    session: &mut Session<T, C>,
    word: &str,
    action: &str,
    change: &str,
    yes: bool,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    ensure_auth(session)?;
    let settings = session.read_settings()?;
    session.device_info()?;
    // A wrong model or a missing step is refused before the prompt.
    session.logo_preconditions()?;
    let now = settings
        .meter_settings()
        .map_err(powerz_protocol::session::Error::from)?
        .startup_logo
        .value;
    writeln!(out, "Startup: now {}; {change}", now.label())?;
    if !yes {
        confirm_typed(word, action, "nothing was written", input, out)?;
    }
    Ok(())
}

/// After a verified write: the startup duration from the fresh SETTINGS
/// read on `out` when the meter saved it. When the meter left its settings
/// unsaved, print the menu-save steps on `progress` (plus "Pick Disable"
/// for a `remove`) and wait up to `timeout` with read-only checks, stopped
/// early by `stop` (Ctrl-C); still unsaved is [`CliError::SettingsNotSaved`]
/// (exit 6), changed calibration bytes are [`CliError::Device`] (exit 5).
fn finish_written<T: Transport, C: Clock>(
    session: &mut Session<T, C>,
    written: LogoWritten,
    remove: bool,
    timeout: Duration,
    stop: &AtomicBool,
    out: &mut dyn Write,
    progress: &mut dyn Write,
) -> Result<(), CliError> {
    let pick = if remove {
        " Pick Disable to keep the startup screen off."
    } else {
        ""
    };
    let mut result = written.settings_after;
    if result == SettingsSave::NeedsMenuSave {
        writeln!(
            progress,
            "The meter changed its startup duration but left its settings unsaved."
        )?;
        writeln!(progress, "{MENU_SAVE_STEPS}{pick}")?;
        writeln!(
            progress,
            "Waiting up to {} s for the save (read-only checks every {} s)…",
            timeout.as_secs(),
            SETTINGS_SAVE_POLL_GAP.as_secs()
        )?;
        let _ = progress.flush();
        result = session.wait_settings_saved(&written.calibration, timeout, &mut |_| {
            !stop.load(Ordering::SeqCst)
        })?;
    }
    match result {
        SettingsSave::Saved(d) => {
            writeln!(out, "Startup: {}", d.label())?;
            Ok(())
        }
        SettingsSave::NeedsMenuSave => Err(CliError::SettingsNotSaved(format!(
            "the logo was written and verified, but the meter's settings are still unsaved. \
             {MENU_SAVE_STEPS}{pick}"
        ))),
        SettingsSave::CalibrationChanged => Err(CliError::Device(
            "the meter's calibration bytes changed after the logo write; check the meter's \
             readings before relying on them"
                .into(),
        )),
    }
}

/// Load `file` and encode it in the opaque or (`alpha`) alpha mode.
fn encode_file(file: &Path, alpha: bool) -> Result<(logo::Rgba, LogoBlob), CliError> {
    let name = file.display();
    let img = logo::load_file(file).map_err(|e| CliError::Usage(format!("{name}: {e}")))?;
    let mode = if alpha {
        LogoMode::Alpha
    } else {
        LogoMode::Opaque
    };
    let blob = logo::encode(&img, mode).map_err(|e| CliError::Usage(format!("{name}: {e}")))?;
    Ok((img, blob))
}

/// Decode built-in logo `b` and encode it in its own (transparent) mode.
fn encode_builtin(b: BuiltinLogo) -> Result<(logo::Rgba, LogoBlob), CliError> {
    let failed = |e: logo::LogoError| CliError::Device(format!("built-in logo {b}: {e}"));
    let img = b.load().map_err(failed)?;
    let blob = logo::encode(&img, b.mode()).map_err(failed)?;
    Ok((img, blob))
}

/// One progress update: on a terminal a `\r`-refreshed segment with a
/// newline ending each step, otherwise one line per update.
fn show_progress(w: &mut dyn Write, tty: bool, p: LogoProgress) {
    let step = match p.step {
        LogoStep::Chunks => "chunk",
        LogoStep::ReadBack => "read-back",
    };
    let (start, end) = match (tty, p.done == p.total) {
        (false, _) => ("", "\n"),
        (true, true) => ("\r", "\n"),
        (true, false) => ("\r", ""),
    };
    let _ = write!(w, "{start}{step} {}/{}{end}", p.done, p.total);
    let _ = w.flush();
}

fn mode_label(mode: LogoMode) -> &'static str {
    match mode {
        LogoMode::Opaque => "opaque (RGB565)",
        LogoMode::Alpha => "alpha (RGB565 + alpha)",
    }
}
