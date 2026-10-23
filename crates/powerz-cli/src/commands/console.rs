//! `powerz console [--allow-high-voltage]` (FR DGSQ6T): the raw PDM text
//! console. Command lines come from `input`; each is sent through the PDM
//! session (guard, pacing, GoodCRC refusal and risk gate unchanged) and the
//! transcript is printed as `HH:MM:SS.mmm > <sent>` / `HH:MM:SS.mmm < <reply>`
//! (UTC), a binary block parsed into words.
//!
//! Ctrl-C (the stop flag) ends the line loop, so the console still returns
//! to 5 V and sends `pdm close`; `input` is read on a helper thread so a
//! blocked read does not hold the console.

use std::io::{BufRead, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use powerz_protocol::pdm::console::{self, ConsoleEntry, Direction, Effect};
use powerz_protocol::pdm::{Error, HighVoltageConfirmation, PdmSession, Risk};
use powerz_protocol::pdm::{legacy, pd};
use powerz_protocol::session::Clock;

use super::close_after;
use super::trigger::return_to_5v;
use crate::error::CliError;

/// How often the line loop looks at the stop flag while waiting for input.
const STOP_POLL: Duration = Duration::from_millis(50);

/// What the console sent that has to be undone before `pdm close`.
#[derive(Debug, Default, PartialEq, Eq)]
struct Undo {
    /// PD was the last protocol entered.
    pd_entered: bool,
    /// A PD request / message above 5 V (or raw text with PD entered) was sent.
    pd: bool,
    /// A legacy protocol was entered.
    legacy: bool,
}

/// `powerz console`: send every non-blank line of `input`, print the
/// transcript on `out`, report refused lines on stderr and go on, until
/// `input` ends or `stop` is set; at the end return to 5 V when a line
/// could have raised VBUS, then `pdm close`.
pub fn console<P, C, R>(
    mut session: PdmSession<P, C>,
    allow_high_voltage: bool,
    input: R,
    stop: &AtomicBool,
    out: &mut dyn Write,
) -> Result<(), CliError>
where
    P: Read + Write,
    C: Clock,
    R: BufRead + Send + 'static,
{
    let lines = spawn_reader(input);
    let mut undo = Undo::default();
    let result = run_lines(
        &mut session,
        allow_high_voltage,
        &lines,
        stop,
        out,
        &mut undo,
    );
    // Both ways back are tried whatever the other did; the first failure wins.
    let pd_back = if undo.pd {
        let offers = pd::offers(session.capabilities());
        return_to_5v(&mut session, &offers)
    } else {
        Ok(())
    };
    let legacy_back = if undo.legacy {
        legacy::return_to_5v(&mut session).map_err(CliError::from)
    } else {
        Ok(())
    };
    // A failed return to 5 V outranks the console's own error.
    close_after(session, pd_back.and(legacy_back).and(result))
}

/// Read `input` line by line on a helper thread; the channel ends with the
/// input (or after the first read error, which is passed on).
fn spawn_reader<R: BufRead + Send + 'static>(
    mut input: R,
) -> mpsc::Receiver<std::io::Result<String>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        loop {
            let mut line = String::new();
            match input.read_line(&mut line) {
                Ok(0) => return,
                Ok(_) => {
                    if tx.send(Ok(line)).is_err() {
                        return;
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(e));
                    return;
                }
            }
        }
    });
    rx
}

fn run_lines<P: Read + Write, C: Clock>(
    session: &mut PdmSession<P, C>,
    allow_high_voltage: bool,
    lines: &mpsc::Receiver<std::io::Result<String>>,
    stop: &AtomicBool,
    out: &mut dyn Write,
    undo: &mut Undo,
) -> Result<(), CliError> {
    loop {
        // Checked before every line: Ctrl-C ends the loop, the caller
        // returns to 5 V and closes.
        if stop.load(Ordering::SeqCst) {
            return Ok(());
        }
        let line = match lines.recv_timeout(STOP_POLL) {
            Ok(line) => line?,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return Ok(()),
        };
        let text = line.trim();
        if text.is_empty() {
            continue;
        }
        let command = console::command_of(text);
        let above = session.risk(&command) == Risk::Above5V;
        if above && !allow_high_voltage {
            eprintln!(
                "refused: `{text}` can raise VBUS above 5 V (or is not a known command); \
                 pass --allow-high-voltage to send it"
            );
            continue;
        }
        let confirmation = above.then(|| HighVoltageConfirmation::for_command(&command));
        match console::send_line(session, text, confirmation, console::LISTEN) {
            Ok(sent) => {
                // Recorded first: the line was written, whatever comes next.
                track(sent.effect, undo);
                for e in &sent.entries {
                    writeln!(out, "{}", format_entry(e))?;
                }
                if let Some(e) = sent.error {
                    return Err(e.into());
                }
            }
            Err(
                e @ (Error::Refused { .. }
                | Error::HighVoltageNotConfirmed { .. }
                | Error::Forbidden { .. }
                | Error::UnsupportedOnCdc { .. }),
            ) => {
                eprintln!("refused: {e}");
            }
            Err(e) => return Err(e.into()),
        }
    }
}

/// Remember what a sent line entered or raised (`entry pd` alone leaves
/// nothing to undo: the charger is still at 5 V). Raw text of unknown
/// effect is ended through PD once PD is entered, else by `pdm close`.
fn track(effect: Option<Effect>, undo: &mut Undo) {
    match effect {
        Some(Effect::EnteredPd) => undo.pd_entered = true,
        Some(Effect::PdContract) => undo.pd = true,
        Some(Effect::Legacy) => {
            undo.legacy = true;
            undo.pd_entered = false;
        }
        Some(Effect::RawLine) => undo.pd |= undo.pd_entered,
        None => {}
    }
}

/// `HH:MM:SS.mmm > text` / `HH:MM:SS.mmm < text` (a block in words).
pub fn format_entry(e: &ConsoleEntry) -> String {
    let arrow = match e.direction {
        Direction::Tx => '>',
        Direction::Rx => '<',
    };
    let text = match &e.block {
        Some(block) => console::describe_block(block),
        None => e.text.clone(),
    };
    format!("{} {arrow} {text}", format_time(e.at_ms))
}

/// `HH:MM:SS.mmm` (UTC) of a Unix time in ms.
fn format_time(at_ms: u64) -> String {
    let ms = at_ms % 86_400_000;
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}

#[cfg(test)]
mod tests {
    use std::io::{self, BufReader, Cursor};
    use std::sync::Arc;

    use powerz_sim::{SimCharger, SimClock, SimPdm, SimPdmPort};

    use super::*;

    fn sim() -> (SimPdm, SimClock) {
        let clock = SimClock::new(1_790_000_000_000);
        (SimPdm::new(SimCharger::Epr140W, clock.clone()), clock)
    }

    fn open<P: Read + Write>(port: P, clock: SimClock) -> PdmSession<P, SimClock> {
        PdmSession::open_with_clock(port, clock).expect("pdm open")
    }

    fn writes(sim: &SimPdm) -> Vec<String> {
        sim.writes()
            .into_iter()
            .map(|w| w.trim().to_owned())
            .collect()
    }

    fn pos(writes: &[String], cmd: &str) -> Option<usize> {
        writes.iter().rposition(|w| w == cmd)
    }

    /// The simulated port, with chosen commands swallowed (never answered)
    /// and one read failing after a chosen command.
    struct Faulty {
        port: SimPdmPort,
        swallow: &'static [&'static str],
        fail_read_after: Option<&'static str>,
        fail_next_read: bool,
        /// Every write, swallowed ones included.
        log: Arc<std::sync::Mutex<Vec<String>>>,
    }

    impl Faulty {
        fn new(
            port: SimPdmPort,
            swallow: &'static [&'static str],
            fail_read_after: Option<&'static str>,
        ) -> (Faulty, Arc<std::sync::Mutex<Vec<String>>>) {
            let log = Arc::new(std::sync::Mutex::new(Vec::new()));
            let f = Faulty {
                port,
                swallow,
                fail_read_after,
                fail_next_read: false,
                log: Arc::clone(&log),
            };
            (f, log)
        }
    }

    impl Read for Faulty {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if std::mem::take(&mut self.fail_next_read) {
                return Err(io::Error::other("read failed"));
            }
            self.port.read(buf)
        }
    }

    impl Write for Faulty {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let text = String::from_utf8_lossy(buf);
            self.log
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(text.trim().to_owned());
            if self.fail_read_after == Some(text.as_ref()) {
                self.fail_next_read = true;
            }
            if self.swallow.contains(&text.as_ref()) {
                return Ok(buf.len());
            }
            self.port.write(buf)
        }
        fn flush(&mut self) -> io::Result<()> {
            self.port.flush()
        }
    }

    /// Input that yields `text`, then (once the meter has seen `wait_for`)
    /// sets `stop` and blocks until the test's sender is dropped: a user
    /// pressing Ctrl-C at the prompt.
    struct StopThenBlock {
        text: Cursor<Vec<u8>>,
        sim: SimPdm,
        wait_for: &'static str,
        stop: Arc<AtomicBool>,
        block: mpsc::Receiver<()>,
    }

    impl Read for StopThenBlock {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let n = self.text.read(buf)?;
            if n > 0 {
                return Ok(n);
            }
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            while !self.sim.writes().iter().any(|w| w == self.wait_for)
                && std::time::Instant::now() < deadline
            {
                std::thread::sleep(Duration::from_millis(5));
            }
            self.stop.store(true, Ordering::SeqCst);
            let _ = self.block.recv();
            Ok(0)
        }
    }

    #[test]
    fn ctrl_c_ends_the_console_and_still_returns_to_5v_and_closes() {
        let (sim, clock) = sim();
        let session = open(sim.open_port(), clock);
        let stop = Arc::new(AtomicBool::new(false));
        let (hold, block) = mpsc::channel::<()>();
        let input = BufReader::new(StopThenBlock {
            text: Cursor::new(b"entry pd\npd req=2\n".to_vec()),
            sim: sim.clone(),
            wait_for: "pd req=2",
            stop: Arc::clone(&stop),
            block,
        });
        let mut out = Vec::new();
        console(session, true, input, &stop, &mut out).expect("ends cleanly");
        drop(hold);
        let w = writes(&sim);
        let req = pos(&w, "pd req=2").unwrap_or_else(|| panic!("{w:?}"));
        let back = pos(&w, "pd req=1").unwrap_or_else(|| panic!("no return to 5 V: {w:?}"));
        assert!(back > req, "{w:?}");
        assert_eq!(w.last().map(String::as_str), Some("pdm close"), "{w:?}");
        assert!(!sim.pdm_active());
    }

    #[test]
    fn a_stop_set_before_any_line_sends_nothing_but_pdm_close() {
        let (sim, clock) = sim();
        let session = open(sim.open_port(), clock);
        let before = sim.writes().len();
        let stop = AtomicBool::new(true);
        let mut out = Vec::new();
        console(
            session,
            true,
            Cursor::new(b"pd req=2\n".to_vec()),
            &stop,
            &mut out,
        )
        .expect("ends");
        let w: Vec<String> = writes(&sim).into_iter().skip(before).collect();
        assert_eq!(w, ["pdm close"]);
    }

    #[test]
    fn a_read_error_after_a_high_voltage_line_still_returns_to_5v() {
        let (sim, clock) = sim();
        let (port, _log) = Faulty::new(sim.open_port(), &[], Some("pd req=2"));
        let session = open(port, clock);
        let stop = AtomicBool::new(false);
        let input = Cursor::new(b"entry pd\npd req=2\npdm parm\n".to_vec());
        let mut out = Vec::new();
        let err = console(session, true, input, &stop, &mut out).expect_err("the read failed");
        assert!(err.to_string().contains("read failed"), "{err}");
        let w = writes(&sim);
        let req = pos(&w, "pd req=2").unwrap_or_else(|| panic!("{w:?}"));
        let back = pos(&w, "pd req=1").unwrap_or_else(|| panic!("no return to 5 V: {w:?}"));
        assert!(back > req, "{w:?}");
        // The console stopped at the error.
        assert_eq!(pos(&w, "pdm parm"), None, "{w:?}");
        assert_eq!(w.last().map(String::as_str), Some("pdm close"), "{w:?}");
    }

    #[test]
    fn the_legacy_reset_is_tried_even_when_the_pd_return_fails() {
        let (sim, clock) = sim();
        let (port, log) = Faulty::new(sim.open_port(), &["pd req=1", "pd hrst"], None);
        let session = open(port, clock);
        let stop = AtomicBool::new(false);
        let input = Cursor::new(b"entry pd\npd req=2\nentry qc\n".to_vec());
        let mut out = Vec::new();
        console(session, true, input, &stop, &mut out).expect_err("the PD return failed");
        let w = log.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let back = pos(&w, "pd hrst").unwrap_or_else(|| panic!("{w:?}"));
        let reset = pos(&w, "reset").unwrap_or_else(|| panic!("no legacy reset: {w:?}"));
        assert!(reset > back, "{w:?}");
        assert_eq!(w.last().map(String::as_str), Some("pdm close"), "{w:?}");
    }

    #[test]
    fn a_confirmed_raw_line_is_undone_through_pd_once_pd_is_entered() {
        let mut undo = Undo::default();
        track(Some(Effect::RawLine), &mut undo);
        assert_eq!(undo, Undo::default(), "before PD: pdm close ends it");
        track(Some(Effect::EnteredPd), &mut undo);
        track(Some(Effect::RawLine), &mut undo);
        assert!(undo.pd);
        let mut undo = Undo::default();
        track(Some(Effect::EnteredPd), &mut undo);
        track(Some(Effect::Legacy), &mut undo);
        track(Some(Effect::RawLine), &mut undo);
        assert!(!undo.pd && undo.legacy, "{undo:?}");
    }
}
