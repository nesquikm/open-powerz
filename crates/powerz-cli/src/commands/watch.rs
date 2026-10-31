//! `powerz watch`.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use powerz_protocol::adcnew::Content;
use powerz_protocol::decode::Rate;
use powerz_protocol::session::{Clock, Session};
use powerz_protocol::transport::Transport;

use super::stream::{MAX_START_ECHOES, too_many_echoes};
use crate::error::CliError;
use crate::output::{Reading, is_closed_pipe, write_adc_line, write_adc_lines};

/// Record rate `watch --source cdc` asks the ADCNEW stream for: fresher
/// than any `--interval` allows, light on the port.
const CDC_RATE: Rate = Rate::Sps10;
/// How long `watch --source cdc` waits for the first pushed record.
const FIRST_RECORD_TIMEOUT_MS: u64 = 2_000;
/// Wait between looks at the port while no record has arrived yet.
const FIRST_RECORD_POLL: Duration = Duration::from_millis(20);

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
/// `stop` is set (Ctrl-C); a closed output pipe ends it with
/// [`CliError::OutputClosed`].
pub fn watch<T: Transport, C: Clock>(
    session: &mut Session<T, C>,
    opts: &WatchOptions,
    stop: &AtomicBool,
    sleep: &mut dyn FnMut(Duration),
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let mut view = View::default();
    while !stop.load(Ordering::SeqCst) {
        let a = session.read_adc()?;
        view.draw(opts, &Reading::from(&a), out)?;
        sleep(opts.interval);
    }
    Ok(())
}

/// `powerz watch --source cdc`: start the ADCNEW push stream (ADC records
/// only, no sign-in, no CONNECT), then on every refresh draw the latest
/// pushed record in the same format as [`watch`] (temperature `-`: records
/// carry none) and call `sleep(interval)`. The stream is stopped (`03`)
/// when `stop` is set, the output pipe closes or a refresh fails. Fails at once where the pipe has
/// no ADCNEW stream (anything but CDC).
pub fn watch_cdc<T: Transport, C: Clock>(
    session: &mut Session<T, C>,
    opts: &WatchOptions,
    stop: &AtomicBool,
    sleep: &mut dyn FnMut(Duration),
    out: &mut dyn Write,
) -> Result<(), CliError> {
    session.start_adcnew(Content::Adc, CDC_RATE)?;
    let result = watch_stream(session, opts, stop, sleep, out);
    let stopped = session.stop_adcnew();
    result?;
    stopped?;
    Ok(())
}

/// The refresh loop of [`watch_cdc`] on a started stream.
fn watch_stream<T: Transport, C: Clock>(
    session: &mut Session<T, C>,
    opts: &WatchOptions,
    stop: &AtomicBool,
    sleep: &mut dyn FnMut(Duration),
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let mut view = View::default();
    let mut latest: Option<Reading> = None;
    let mut waited_since: Option<u64> = None;
    while !stop.load(Ordering::SeqCst) {
        if let Some(r) = session.poll_adcnew()?.records.last() {
            latest = Some(Reading::from(r));
        }
        if session.adcnew_counters().start_echoes > MAX_START_ECHOES {
            return Err(too_many_echoes());
        }
        if latest.is_none() && session.adcnew_restart_pending() {
            // The meter echoed the start (right after a bulk session); the
            // session sends it again after a quiet spell. Not a timeout.
            waited_since = None;
            session.sleep(FIRST_RECORD_POLL);
            continue;
        }
        let Some(reading) = latest else {
            // Nothing pushed yet: look again shortly, within a limit.
            let now = session.now_ms();
            let since = *waited_since.get_or_insert(now);
            if now.saturating_sub(since) >= FIRST_RECORD_TIMEOUT_MS {
                return Err(CliError::Device(
                    "the meter pushed no ADCNEW record on the CDC port".into(),
                ));
            }
            session.sleep(FIRST_RECORD_POLL);
            continue;
        };
        view.draw(opts, &reading, out)?;
        sleep(opts.interval);
    }
    Ok(())
}

/// What one refresh draws: a view redrawn in place on a terminal, else one
/// line per refresh.
#[derive(Debug, Default)]
struct View {
    /// Lines drawn by the previous refresh (tty mode), to move back over.
    drawn: usize,
}

impl View {
    /// Draw one refresh; a closed output pipe is [`CliError::OutputClosed`].
    fn draw(
        &mut self,
        opts: &WatchOptions,
        reading: &Reading,
        out: &mut dyn Write,
    ) -> Result<(), CliError> {
        self.write(opts, reading, out).map_err(|e| {
            if is_closed_pipe(&e) {
                CliError::OutputClosed
            } else {
                e.into()
            }
        })
    }

    fn write(
        &mut self,
        opts: &WatchOptions,
        reading: &Reading,
        out: &mut dyn Write,
    ) -> std::io::Result<()> {
        if opts.tty {
            let mut view = Vec::new();
            write_adc_lines(&mut view, reading)?;
            if self.drawn > 0 {
                // Cursor up over the last view, then clear to end of screen.
                write!(out, "\x1b[{}A\x1b[J", self.drawn)?;
            }
            out.write_all(&view)?;
            self.drawn = view.iter().filter(|&&b| b == b'\n').count();
        } else {
            write_adc_line(out, reading)?;
        }
        out.flush()
    }
}
