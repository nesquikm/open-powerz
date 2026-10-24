//! Custom PD messages and the raw console over a scripted PDM port
//! (FR DGSQ6T, AC .1, .3, .4, .5).
//!
//! The fake port answers exact command bytes with reply bytes written from
//! the device-confirmed session-2 transcript (140 W EPR charger). A fake
//! clock shares the port's time line, so reply timeouts run without sleeping.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::io::{self, Read, Write};
use std::rc::Rc;
use std::time::Duration;

use powerz_protocol::pdm::caps::CapBlock;
use powerz_protocol::pdm::console::{self, Direction, Effect};
use powerz_protocol::pdm::message::{PdMessage, Preset, SopTarget};
use powerz_protocol::pdm::{Command, Error, HighVoltageConfirmation, PdmSession, Risk};
use powerz_protocol::session::Clock;

const T0: u64 = 1_790_000_000_000;
const LISTEN: Duration = Duration::from_millis(300);

fn hex(s: &str) -> Vec<u8> {
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

/// SPR `pdo:` block of the 140 W charger, with its `\n`. [device]
fn spr_block() -> Vec<u8> {
    hex("70646f3a 34 0b 2c918109 2cd10200 2cb10400 d6410600
         00000000 00000000 00000000 00000000 00000000 00000000 00000000 0a")
}

fn entry_pd_reply() -> Vec<u8> {
    let mut b = b"ok\ncc1 attach\n".to_vec();
    b.extend(spr_block());
    b.extend(b"ready:5191mV,0mA\n");
    b
}

#[derive(Clone, Debug)]
struct FakeClock(Rc<Cell<u64>>);

impl Clock for FakeClock {
    fn now_ms(&mut self) -> u64 {
        self.0.get()
    }
    fn random_u64(&mut self) -> u64 {
        4
    }
    fn sleep(&mut self, d: Duration) {
        self.0.set(self.0.get() + d.as_millis() as u64);
    }
}

#[derive(Default)]
struct State {
    script: HashMap<Vec<u8>, Vec<u8>>,
    writes: Vec<Vec<u8>>,
    pending: VecDeque<u8>,
    /// After this command is written, the next read fails once.
    fail_after: Option<Vec<u8>>,
    fail_next_read: bool,
}

/// Scripted serial port; clones share state.
#[derive(Clone, Default)]
struct FakePort {
    state: Rc<RefCell<State>>,
}

impl FakePort {
    fn on(self, cmd: &str, reply: &[u8]) -> FakePort {
        self.state
            .borrow_mut()
            .script
            .insert(cmd.as_bytes().to_vec(), reply.to_vec());
        self
    }
    fn fail_read_after(&self, cmd: &str) {
        self.state.borrow_mut().fail_after = Some(cmd.as_bytes().to_vec());
    }
    fn writes(&self) -> Vec<String> {
        self.state
            .borrow()
            .writes
            .iter()
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .collect()
    }
}

impl std::fmt::Debug for FakePort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FakePort")
    }
}

impl Read for FakePort {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut s = self.state.borrow_mut();
        if s.fail_next_read {
            s.fail_next_read = false;
            return Err(io::Error::other("fake read failure"));
        }
        if s.pending.is_empty() {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "fake timeout"));
        }
        let n = buf.len().min(s.pending.len()).min(9);
        for b in buf.iter_mut().take(n) {
            *b = s.pending.pop_front().unwrap();
        }
        Ok(n)
    }
}

impl Write for FakePort {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut s = self.state.borrow_mut();
        s.writes.push(buf.to_vec());
        if s.fail_after.as_deref() == Some(buf) {
            s.fail_next_read = true;
        }
        if let Some(r) = s.script.get(buf).cloned() {
            s.pending.extend(r);
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn port() -> FakePort {
    FakePort::default()
        .on("pdm open", b"pdm mode entry\nver1.0\n")
        .on("pdm parm", b"type=0,em=2,sink=0\n")
        .on("entry pd", &entry_pd_reply())
        .on("pd req=1", b"req:5000mV,3000mA\nready:5180mV,0mA\n")
        .on("pd req=2", b"req:9000mV,3000mA\nready:8554mV,0mA\n")
        .on("pdm close", b"ok\n")
}

type Session = PdmSession<FakePort, FakeClock>;

fn open(port: &FakePort, now: &Rc<Cell<u64>>) -> Session {
    PdmSession::open_with_clock(port.clone(), FakeClock(Rc::clone(now))).expect("pdm open")
}

fn open_in_pd(port: &FakePort, now: &Rc<Cell<u64>>) -> Session {
    let mut s = open(port, now);
    s.send(&Command::parse("entry pd")).expect("entry pd");
    s.wait_for(&["ready:"], Duration::from_millis(500))
        .expect("offers");
    assert!(s.capabilities().pd.is_some(), "pdo block seen");
    s
}

/// The data message `Request` for fixed 9 V (RDO position 2).
fn request_9v() -> PdMessage {
    PdMessage::data(SopTarget::Sop, 2, vec![0x2304_B12C])
}

mod pdm_message {
    use super::*;

    // ---- AC-DGSQ6T.1: built messages go out as pd cmd / pd data ----

    #[test]
    fn built_control_and_cable_messages_are_written_as_pd_cmd_and_pd_data() {
        let port = port();
        let now = Rc::new(Cell::new(T0));
        let mut s = open(&port, &now);
        for (msg, wire) in [
            (PdMessage::control(SopTarget::Sop, 13), "pd cmd=13"),
            (
                PdMessage::control(SopTarget::SopPrime, 13),
                "pd data=018D00",
            ),
            (
                PdMessage::control(SopTarget::SopDoublePrime, 13),
                "pd data=028D00",
            ),
        ] {
            let cmd = msg.to_command().expect("builds");
            let conf = HighVoltageConfirmation::for_command(&cmd);
            s.send_confirmed(&cmd, conf).expect("sent");
            assert_eq!(port.writes().last().map(String::as_str), Some(wire));
        }
    }

    #[test]
    fn the_cable_preset_is_written_as_pd_data_on_sop_prime() {
        let port = port();
        let now = Rc::new(Cell::new(T0));
        let mut s = open(&port, &now);
        let cmd = Preset::DiscoverIdentityCable
            .message()
            .to_command()
            .expect("builds");
        let conf = HighVoltageConfirmation::for_command(&cmd);
        s.send_confirmed(&cmd, conf).expect("sent");
        let last = port.writes().last().cloned().unwrap_or_default();
        assert!(last.starts_with("pd data=01"), "{last}");
        assert_eq!(last.len(), "pd data=".len() + 14, "{last}");
        assert_eq!(last, last.to_uppercase().replace("PD DATA=", "pd data="));
    }

    // ---- AC-DGSQ6T.3: GoodCRC is never written, confirmed or not ----

    #[test]
    fn the_session_refuses_goodcrc_with_a_reason_and_writes_nothing() {
        let port = port();
        let now = Rc::new(Cell::new(T0));
        let mut s = open(&port, &now);
        let before = port.writes().len();
        for cmd in [Command::PdCmd(1), Command::parse("pd data=014100")] {
            let err = s.send(&cmd).expect_err("refused");
            match &err {
                Error::Refused { reason, .. } => {
                    assert!(reason.contains("GoodCRC"), "{reason}")
                }
                other => panic!("expected Refused, got {other:?}"),
            }
            let conf = HighVoltageConfirmation::for_command(&cmd);
            let err = s.send_confirmed(&cmd, conf).expect_err("refused");
            assert!(matches!(err, Error::Refused { .. }), "{err:?}");
        }
        assert_eq!(port.writes().len(), before, "{:?}", port.writes());
    }

    #[test]
    fn the_console_refuses_goodcrc_even_when_confirmed() {
        let port = port();
        let now = Rc::new(Cell::new(T0));
        let mut s = open(&port, &now);
        let before = port.writes().len();
        let conf = HighVoltageConfirmation::for_command(&Command::parse("pd cmd=1"));
        let err = console::send_line(&mut s, "pd cmd=1", Some(conf), LISTEN).expect_err("refused");
        match err {
            Error::Refused { reason, .. } => assert!(reason.contains("GoodCRC"), "{reason}"),
            other => panic!("expected Refused, got {other:?}"),
        }
        assert_eq!(port.writes().len(), before);
    }

    // ---- AC-DGSQ6T.4: the console sends any line and shows both sides with times ----

    #[test]
    fn the_console_shows_the_sent_line_and_the_reply_with_times() {
        let port = port();
        let now = Rc::new(Cell::new(T0));
        let mut s = open(&port, &now);
        let start = now.get();
        let t = console::send_line(&mut s, "pdm parm", None, LISTEN)
            .expect("sent")
            .entries;
        assert_eq!(port.writes().last().map(String::as_str), Some("pdm parm"));
        assert!(t.len() >= 2, "{t:?}");
        assert_eq!(t[0].direction, Direction::Tx);
        assert_eq!(t[0].text, "pdm parm");
        assert!(t[0].at_ms >= start, "{t:?}");
        let rx = t
            .iter()
            .find(|e| e.direction == Direction::Rx)
            .expect("a reply line");
        assert_eq!(rx.text, "type=0,em=2,sink=0");
        assert!(rx.at_ms >= t[0].at_ms, "{t:?}");
        assert!(rx.at_ms <= now.get(), "{t:?}");
        assert_eq!(rx.block, None);
    }

    #[test]
    fn the_console_parses_binary_blocks_in_replies() {
        let port = port();
        let now = Rc::new(Cell::new(T0));
        let mut s = open(&port, &now);
        let t = console::send_line(&mut s, "entry pd", None, LISTEN)
            .expect("sent")
            .entries;
        let texts: Vec<&str> = t.iter().map(|e| e.text.as_str()).collect();
        assert!(texts.contains(&"ok"), "{texts:?}");
        assert!(texts.contains(&"ready:5191mV,0mA"), "{texts:?}");
        let block = t
            .iter()
            .find_map(|e| e.block.as_ref())
            .unwrap_or_else(|| panic!("no parsed block in {t:?}"));
        let CapBlock::Pdo(pdo) = block else {
            panic!("expected a pdo block, got {block:?}");
        };
        assert!(pdo.pdo(2).is_some(), "9 V slot decoded");
        // Blocks still update what the risk gate knows.
        assert!(s.capabilities().pd.is_some());
    }

    #[test]
    fn the_console_sends_any_text_once_confirmed() {
        let port = port();
        let now = Rc::new(Cell::new(T0));
        let mut s = open(&port, &now);
        let line = "pdm hello world";
        assert_eq!(console::command_of(line), Command::Raw(line.to_string()));
        assert_eq!(console::command_of("  pdm parm \n"), Command::PdmParm);
        let conf = HighVoltageConfirmation::for_command(&console::command_of(line));
        let t = console::send_line(&mut s, line, Some(conf), LISTEN)
            .expect("sent")
            .entries;
        assert_eq!(port.writes().last().map(String::as_str), Some(line));
        // No reply to unknown text: only the sent line.
        assert_eq!(t.len(), 1, "{t:?}");
        assert_eq!(t[0].direction, Direction::Tx);
    }

    // ---- AC-DGSQ6T.5: above 5 V asks for the confirmation first ----

    #[test]
    fn an_unparsable_console_line_needs_a_confirmation() {
        let port = port();
        let now = Rc::new(Cell::new(T0));
        let mut s = open(&port, &now);
        assert_eq!(s.risk(&console::command_of("bogus")), Risk::Above5V);
        let before = port.writes().len();
        let err = console::send_line(&mut s, "bogus", None, LISTEN).expect_err("refused");
        assert!(
            matches!(err, Error::HighVoltageNotConfirmed { .. }),
            "{err:?}"
        );
        // A confirmation for another command does not count.
        let wrong = HighVoltageConfirmation::for_command(&Command::Raw("other".into()));
        let err = console::send_line(&mut s, "bogus", Some(wrong), LISTEN).expect_err("refused");
        assert!(
            matches!(err, Error::HighVoltageNotConfirmed { .. }),
            "{err:?}"
        );
        assert_eq!(port.writes().len(), before);
    }

    #[test]
    fn a_console_request_above_5v_needs_a_confirmation_and_5v_does_not() {
        let port = port();
        let now = Rc::new(Cell::new(T0));
        let mut s = open_in_pd(&port, &now);
        let before = port.writes().len();
        let err = console::send_line(&mut s, "pd req=2", None, LISTEN).expect_err("refused");
        assert!(
            matches!(err, Error::HighVoltageNotConfirmed { .. }),
            "{err:?}"
        );
        assert_eq!(port.writes().len(), before);
        console::send_line(&mut s, "pd req=1", None, LISTEN).expect("5 V needs no confirmation");
        let conf = HighVoltageConfirmation::for_command(&Command::parse("pd req=2"));
        let sent = console::send_line(&mut s, "pd req=2", Some(conf), LISTEN).expect("confirmed");
        assert!(
            sent.entries.iter().any(|e| e.text == "ready:8554mV,0mA"),
            "{sent:?}"
        );
        assert_eq!(sent.effect, Some(Effect::PdContract));
        assert_eq!(sent.error, None);
    }

    // ---- Review: GoodCRC in any raw spelling never reaches the meter ----

    #[test]
    fn the_session_and_console_refuse_goodcrc_in_any_raw_spelling() {
        let port = port();
        let now = Rc::new(Cell::new(T0));
        let mut s = open(&port, &now);
        let before = port.writes().len();
        for line in [
            "pd cmd=01",
            "pd cmd=+1",
            "pd cmd=0x1",
            "pd  cmd=1",
            "pd data=014100",
        ] {
            let cmd = console::command_of(line);
            let conf = HighVoltageConfirmation::for_command(&cmd);
            let err = s.send_confirmed(&cmd, conf).expect_err("refused");
            assert!(matches!(err, Error::Refused { .. }), "{line}: {err:?}");
            let conf = HighVoltageConfirmation::for_command(&cmd);
            let err = console::send_line(&mut s, line, Some(conf), LISTEN).expect_err("refused");
            match err {
                Error::Refused { reason, .. } => {
                    assert!(reason.contains("GoodCRC"), "{line}: {reason}")
                }
                other => panic!("{line}: expected Refused, got {other:?}"),
            }
        }
        assert_eq!(port.writes().len(), before, "{:?}", port.writes());
    }

    // ---- Review: a read failure after the write keeps the line's effect ----

    #[test]
    fn a_read_error_after_the_write_still_reports_the_effect() {
        let port = port();
        let now = Rc::new(Cell::new(T0));
        let mut s = open_in_pd(&port, &now);
        port.fail_read_after("pd req=2");
        let conf = HighVoltageConfirmation::for_command(&Command::parse("pd req=2"));
        let sent = console::send_line(&mut s, "pd req=2", Some(conf), LISTEN)
            .expect("written, so not an Err");
        assert_eq!(port.writes().last().map(String::as_str), Some("pd req=2"));
        assert_eq!(sent.effect, Some(Effect::PdContract));
        assert!(matches!(sent.error, Some(Error::Io(_))), "{sent:?}");
        assert_eq!(sent.entries[0].direction, Direction::Tx);
    }

    // ---- Review: confirmed raw lines and session control in the console ----

    #[test]
    fn a_confirmed_raw_line_reports_its_unknown_effect() {
        let port = port();
        let now = Rc::new(Cell::new(T0));
        let mut s = open(&port, &now);
        let conf = HighVoltageConfirmation::for_command(&console::command_of("hello meter"));
        let sent = console::send_line(&mut s, "hello meter", Some(conf), LISTEN).expect("sent");
        assert_eq!(sent.effect, Some(Effect::RawLine));
        assert_eq!(
            console::effect_of(&Command::Raw("x".into()), true),
            Some(Effect::RawLine)
        );
    }

    #[test]
    fn the_console_refuses_pdm_open_and_pdm_close() {
        let port = port();
        let now = Rc::new(Cell::new(T0));
        let mut s = open(&port, &now);
        let before = port.writes().len();
        for line in ["pdm close", "pdm open", "PDM  Close", "pdm closenow"] {
            let cmd = console::command_of(line);
            let conf = HighVoltageConfirmation::for_command(&cmd);
            let err = console::send_line(&mut s, line, Some(conf), LISTEN).expect_err("refused");
            match err {
                Error::Refused { reason, .. } => {
                    assert!(reason.contains("pdm close"), "{line}: {reason}")
                }
                other => panic!("{line}: expected Refused, got {other:?}"),
            }
        }
        assert_eq!(port.writes().len(), before, "{:?}", port.writes());
    }

    #[test]
    fn a_built_request_for_a_higher_voltage_is_above_5v_and_gated() {
        let port = port();
        let now = Rc::new(Cell::new(T0));
        let mut s = open_in_pd(&port, &now);
        let cmd = request_9v().to_command().expect("builds");
        assert_eq!(s.risk(&cmd), Risk::Above5V);
        let before = port.writes().len();
        let err = s.send(&cmd).expect_err("refused");
        assert!(
            matches!(err, Error::HighVoltageNotConfirmed { .. }),
            "{err:?}"
        );
        assert_eq!(port.writes().len(), before);
        // The 5 V slot as a built Request is not above 5 V.
        let five = PdMessage::data(SopTarget::Sop, 2, vec![0x1304_B12C])
            .to_command()
            .expect("builds");
        assert_eq!(s.risk(&five), Risk::AtMost5V);
        // A control message never raises VBUS.
        let soft = PdMessage::control(SopTarget::Sop, 13)
            .to_command()
            .expect("builds");
        assert_eq!(s.risk(&soft), Risk::AtMost5V);
    }
}
