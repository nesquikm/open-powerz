//! `powerz logo` (FR YCRQRQ).

use std::io::{BufRead, Write};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use powerz_protocol::decode::settings::StartupLogo;
use powerz_protocol::logo::{self, CHUNK, LogoBlob, LogoMode, MIN_SIDE};
use powerz_protocol::session::{Clock, LogoProgress, LogoStatus, LogoStep, LogoWritten, Session};
use powerz_protocol::transport::Transport;

use super::records::confirm_typed;
use crate::error::CliError;

/// What `--i-understand` acknowledges for `logo write`.
pub const LOGO_WRITE_RISK: &str = "writing the startup logo rewrites the meter's flash; \
     pass --i-understand to write it";

/// What `--i-understand` acknowledges for `logo remove`.
pub const LOGO_REMOVE_RISK: &str = "removing the startup logo rewrites the meter's flash; \
     pass --i-understand to remove it";

/// `powerz logo preview IMG`: encode the image as the meter would store it
/// and print the final size, mode, blob size, padded size and chunk count;
/// warn on `warn` when a side ends up below [`MIN_SIDE`]. With `output`,
/// the decoded logo is written there as a PNG. Opens no device.
pub fn logo_preview(
    file: &Path,
    alpha: bool,
    output: Option<&Path>,
    out: &mut dyn Write,
    warn: &mut dyn Write,
) -> Result<(), CliError> {
    let name = file.display();
    let (img, blob) = encode_file(file, alpha)?;
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
}

/// The acknowledgement flags shared by `logo write` and `logo remove`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LogoConfirm {
    /// `--i-understand`: the user acknowledged the flash write.
    pub i_understand: bool,
    /// `--yes`: skip the typed confirmation (still needs `--i-understand`).
    pub yes: bool,
}

/// `powerz logo write IMG`: refused without `--i-understand`; encode the
/// image, authenticate, read SETTINGS and the model (the upload's
/// preconditions), ask for "write" to be typed unless `--yes`, then upload
/// with per-chunk and read-back progress on `progress` (refreshed in place
/// with `\r` when `opts.progress_tty`, one line per update otherwise, so a
/// `--trace` on the same stream stays line by line). Ends with the
/// verified logo and the startup-screen duration on `out`, plus a hint on
/// `progress` when the startup screen is off.
///
/// `arm_ctrl_c` is called once the write is confirmed, just before the
/// first chunk, and returns the Ctrl-C flag. A Ctrl-C during the chunk
/// phase does not stop the write (a half-written logo is worse than a
/// finished one): one note says the write finishes first, and the tool
/// exits when it is done.
pub fn logo_write<T: Transport, C: Clock>(
    session: &mut Session<T, C>,
    file: &Path,
    opts: &LogoWriteOptions,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
    progress: &mut dyn Write,
    arm_ctrl_c: &mut dyn FnMut() -> Result<Arc<AtomicBool>, CliError>,
) -> Result<(), CliError> {
    if !opts.confirm.i_understand {
        return Err(CliError::Usage(LOGO_WRITE_RISK.into()));
    }
    let (_, blob) = encode_file(file, opts.alpha)?;
    let p = blob.preview();
    prepare_write(
        session,
        "write",
        "overwrite the meter's logo",
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
    print_written(written, out, progress)
}

/// `powerz logo remove`: the same rules as [`logo_write`] (refused without
/// `--i-understand`, "remove" typed unless `--yes`), then clear the logo
/// area, reset the flag and read it back, ending with the verified result.
pub fn logo_remove<T: Transport, C: Clock>(
    session: &mut Session<T, C>,
    confirm: LogoConfirm,
    progress_tty: bool,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
    progress: &mut dyn Write,
) -> Result<(), CliError> {
    if !confirm.i_understand {
        return Err(CliError::Usage(LOGO_REMOVE_RISK.into()));
    }
    prepare_write(
        session,
        "remove",
        "remove the meter's logo",
        confirm.yes,
        input,
        out,
    )?;
    let written = session.logo_remove(&mut |pr| show_progress(progress, progress_tty, pr))?;
    writeln!(out, "Logo:    removed and verified (logo area empty)")?;
    print_written(written, out, progress)
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
/// preconditions), then ask for `word` to be typed unless `yes`.
fn prepare_write<T: Transport, C: Clock>(
    session: &mut Session<T, C>,
    word: &str,
    action: &str,
    yes: bool,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    ensure_auth(session)?;
    session.read_settings()?;
    session.device_info()?;
    // A wrong model or a missing step is refused before the prompt.
    session.logo_preconditions()?;
    if !yes {
        confirm_typed(word, action, "nothing was written", input, out)?;
    }
    Ok(())
}

/// The startup-screen duration on `out`, plus a hint on `progress` when
/// the startup screen is off.
fn print_written(
    written: LogoWritten,
    out: &mut dyn Write,
    progress: &mut dyn Write,
) -> Result<(), CliError> {
    writeln!(out, "Startup: {}", startup_label(written.startup_duration))?;
    if written.startup_screen_off {
        writeln!(
            progress,
            "hint: the startup screen is off, so the logo will not show; turn it on with \
             `powerz settings set startup_duration=3 --i-understand`"
        )?;
    }
    Ok(())
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

fn startup_label(d: StartupLogo) -> &'static str {
    match d {
        StartupLogo::Off => "off (the logo will not show)",
        StartupLogo::Secs1 => "1 s",
        StartupLogo::Secs3 => "3 s",
        StartupLogo::Secs5 => "5 s",
    }
}

fn mode_label(mode: LogoMode) -> &'static str {
    match mode {
        LogoMode::Opaque => "opaque (RGB565)",
        LogoMode::Alpha => "alpha (RGB565 + alpha)",
    }
}
