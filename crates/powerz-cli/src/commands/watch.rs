//! `powerz watch`.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use powerz_protocol::session::{Clock, Session};
use powerz_protocol::transport::Transport;

use crate::error::CliError;
use crate::output::{write_adc_line, write_adc_lines};

/// Options of `powerz watch`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatchOptions {
    /// Time between refreshes.
    pub interval: Duration,
    /// Stdout is a terminal: redraw in place with ANSI cursor movement;
    /// otherwise print one line per refresh.
    pub tty: bool,
}

/// `powerz watch`: checks `stop` before every refresh; each refresh reads one
/// ADC snapshot, draws it, then calls `sleep(interval)`. Returns `Ok` once
/// `stop` is set (Ctrl-C).
pub fn watch<T: Transport, C: Clock>(
    session: &mut Session<T, C>,
    opts: &WatchOptions,
    stop: &AtomicBool,
    sleep: &mut dyn FnMut(Duration),
    out: &mut dyn Write,
) -> Result<(), CliError> {
    // Lines drawn by the previous refresh (tty mode), to move back over.
    let mut drawn = 0usize;
    while !stop.load(Ordering::SeqCst) {
        let a = session.read_adc()?;
        if opts.tty {
            let mut view = Vec::new();
            write_adc_lines(&mut view, &a)?;
            if drawn > 0 {
                // Cursor up over the last view, then clear to end of screen.
                write!(out, "\x1b[{drawn}A\x1b[J")?;
            }
            out.write_all(&view)?;
            drawn = view.iter().filter(|&&b| b == b'\n').count();
        } else {
            write_adc_line(out, &a)?;
        }
        out.flush()?;
        sleep(opts.interval);
    }
    Ok(())
}
