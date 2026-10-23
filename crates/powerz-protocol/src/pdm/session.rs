//! The PDM session: open, paced commands, reply reading, risk gate, close.
//!
//! - Open: `pdm open`, wait for `pdm mode entry`; if a `busy` line comes
//!   with it (within [`OPEN_SETTLE`]), send `reset` and wait for `ok`.
//! - Every command is one write of its exact bytes, at least
//!   [`MIN_COMMAND_GAP`] after the previous one (timed on the injected
//!   [`Clock`]), after the binary-frame guard.
//! - Commands classified [`Risk::Above5V`] need a [`HighVoltageConfirmation`]
//!   made for that exact command; it is consumed by the send.
//! - `pdm close` is sent before the port is released: by [`PdmSession::close`],
//!   on an error inside [`PdmSession::open_with_clock`], and on drop.
//! - An idle port read (timed out / would block) sleeps [`IDLE_POLL`] on the
//!   clock, so reply timeouts run on the clock too.
//! - A reply that only another command gets latches the session out of
//!   sync ([`Error::OutOfSync`]): every later command is refused, except the
//!   way back to 5 V through [`PdmSession::send_safety`] and `pdm close`.

use std::io::{Read, Write};
use std::time::Duration;

use super::caps::Capabilities;
use super::command::Command;
use super::reply::{ReplyItem, ReplyReader};
use super::risk::Risk;
use super::{Error, map_io};
use crate::session::{Clock, SystemClock};

/// Minimum spacing between two command writes. \[static\] \[device\]
pub const MIN_COMMAND_GAP: Duration = Duration::from_millis(60);
/// Default time to wait for a reply line.
pub const REPLY_TIMEOUT: Duration = Duration::from_millis(1000);
/// How long to keep reading after `pdm mode entry` for a `busy` line.
pub const OPEN_SETTLE: Duration = Duration::from_millis(100);
/// Clock sleep after an idle port read.
pub const IDLE_POLL: Duration = Duration::from_millis(5);
/// How long dropping a session waits for the `ok` to `pdm close`; an
/// explicit [`PdmSession::close`] waits the full [`REPLY_TIMEOUT`].
pub const DROP_CLOSE_WAIT: Duration = Duration::from_millis(100);
/// Clock sleep after a [`PdmSession::send_safety`] write, so the charger
/// can act on it before the next command. \[inferred\]
pub const SAFETY_SETTLE: Duration = Duration::from_millis(250);

/// Single-use permission to send one command classified above 5 V.
/// Not `Clone`: each confirmation covers exactly one send of one command.
#[derive(Debug, PartialEq, Eq)]
pub struct HighVoltageConfirmation {
    command: Command,
}

impl HighVoltageConfirmation {
    /// Confirm sending `command` once.
    pub fn for_command(command: &Command) -> HighVoltageConfirmation {
        HighVoltageConfirmation {
            command: command.clone(),
        }
    }
}

/// An open PDM text session over port `P`.
#[derive(Debug)]
pub struct PdmSession<P: Read + Write, C: Clock = SystemClock> {
    port: P,
    clock: C,
    reader: ReplyReader,
    caps: Capabilities,
    last_write_ms: Option<u64>,
    closed: bool,
    /// The last command written: whose reply is awaited.
    current: Option<Command>,
    /// Set once a reply belonging to another command arrived; every later
    /// command is refused with it until [`PdmSession::reopen`].
    out_of_sync: Option<Error>,
    /// A `pdm mode entry` line arrived since the last `pdm open`: a late
    /// `ver<n>` is then the rest of that reply, not a foreign one.
    opened: bool,
}

impl<P: Read + Write> PdmSession<P, SystemClock> {
    /// Open the PDM session on an already-open port, real clock.
    pub fn open(port: P) -> Result<Self, Error> {
        PdmSession::open_with_clock(port, SystemClock)
    }
}

impl<P: Read + Write, C: Clock> PdmSession<P, C> {
    /// Open the PDM session on `port` with an injected clock.
    pub fn open_with_clock(port: P, clock: C) -> Result<Self, Error> {
        let mut session = PdmSession {
            port,
            clock,
            reader: ReplyReader::new(),
            caps: Capabilities::default(),
            last_write_ms: None,
            closed: false,
            current: None,
            out_of_sync: None,
            opened: false,
        };
        // On any error below, dropping `session` sends `pdm close` before
        // the port is released.
        session.handshake()?;
        Ok(session)
    }

    /// `pdm open`, wait for `pdm mode entry` \[device\], and `reset` if the
    /// device reports `busy` \[static\] \[doc\] (not yet seen on the device).
    fn handshake(&mut self) -> Result<(), Error> {
        self.write(&Command::PdmOpen)?;
        // The vendor app looks for both strings in one reply chunk, so
        // `busy` may come before `pdm mode entry` as well as after. \[static\]
        // One deadline covers them all, so endless `busy` lines time out.
        let deadline = self.deadline(REPLY_TIMEOUT);
        let mut busy = false;
        loop {
            let line = self.wait_until(&["pdm mode entry", "busy"], deadline)?;
            if line.starts_with("busy") {
                busy = true;
            } else {
                break;
            }
        }
        if busy || self.busy_after_open()? {
            self.write(&Command::Reset)?;
            self.wait_for(&["ok"], REPLY_TIMEOUT)?;
        }
        Ok(())
    }

    /// Read for [`OPEN_SETTLE`] after `pdm mode entry`: did a `busy` line
    /// come with it? \[static\]
    fn busy_after_open(&mut self) -> Result<bool, Error> {
        let deadline = self.deadline(OPEN_SETTLE);
        loop {
            // Checked every round: unrelated lines must not extend the wait.
            if self.clock.now_ms() >= deadline {
                return Ok(false);
            }
            match self.read_item(deadline, "busy") {
                Ok(ReplyItem::Line(line)) if line.starts_with("busy") => return Ok(true),
                Ok(_) => {}
                Err(Error::Timeout { .. }) => return Ok(false),
                Err(e) => return Err(e),
            }
        }
    }

    /// Send `pdm close` and wait for `ok`, at most once per session.
    fn send_close(&mut self, wait: Duration) -> Result<(), Error> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        // Sent even when out of sync: closing is always allowed.
        self.write(&Command::PdmClose)?;
        self.wait_for(&["ok"], wait).map(|_| ())
    }

    /// Send `pdm close` now, keeping the handle (later calls, [`close`] and
    /// drop send nothing more); waits at most [`DROP_CLOSE_WAIT`] for `ok`.
    /// Allowed out of sync, e.g. after [`PdmSession::send_safety`].
    ///
    /// [`close`]: PdmSession::close
    pub fn close_in_place(&mut self) -> Result<(), Error> {
        self.send_close(DROP_CLOSE_WAIT)
    }

    /// Write one of the commands that bring VBUS back to 5 V — `pd hrst`,
    /// `pd req=1` (no voltage / current) or `reset` — even when the session
    /// is out of sync. No reply is waited for or trusted: the write is
    /// followed by a [`SAFETY_SETTLE`] sleep and nothing is read. Any other
    /// command is [`Error::Refused`] with nothing written.
    pub fn send_safety(&mut self, command: &Command) -> Result<(), Error> {
        let safe = matches!(
            command,
            Command::PdHardReset
                | Command::Reset
                | Command::PdRequest {
                    position: 1,
                    voltage_mv: None,
                    current_ma: None,
                }
        );
        if !safe {
            return Err(Error::Refused {
                command: command_text(command),
                reason: "only `pd hrst`, `pd req=1` and `reset` are sent on the safety path"
                    .to_string(),
            });
        }
        self.write(command)?;
        self.clock.sleep(SAFETY_SETTLE);
        Ok(())
    }

    /// Send `command`; refused with [`Error::HighVoltageNotConfirmed`] (and
    /// nothing written) when it classifies [`Risk::Above5V`].
    pub fn send(&mut self, command: &Command) -> Result<(), Error> {
        self.check_in_step()?;
        refuse(command)?;
        if self.risk(command) == Risk::Above5V {
            return Err(Error::HighVoltageNotConfirmed {
                command: command_text(command),
            });
        }
        self.write(command)
    }

    /// Send `command` with a confirmation made for that exact command.
    pub fn send_confirmed(
        &mut self,
        command: &Command,
        confirmation: HighVoltageConfirmation,
    ) -> Result<(), Error> {
        self.check_in_step()?;
        refuse(command)?;
        if confirmation.command != *command {
            return Err(Error::HighVoltageNotConfirmed {
                command: command_text(command),
            });
        }
        self.write(command)
    }

    /// [`Error::OutOfSync`] (nothing written) once the meter's replies were
    /// seen out of step with the commands.
    pub fn check_in_step(&self) -> Result<(), Error> {
        match &self.out_of_sync {
            Some(err) => Err(err.clone()),
            None => Ok(()),
        }
    }

    /// Record that the meter is out of step: the last command written got
    /// `got` (e.g. no reply at all) instead of its own reply. Every later
    /// command is refused with the returned [`Error::OutOfSync`] until
    /// [`PdmSession::reopen`].
    pub fn mark_out_of_sync(&mut self, got: &str) -> Error {
        let command = self.current.as_ref().map(command_text).unwrap_or_default();
        let err = Error::OutOfSync {
            command,
            got: got.to_string(),
        };
        self.out_of_sync = Some(err.clone());
        err
    }

    /// [`Error::OutOfSync`] (latched, see [`PdmSession::mark_out_of_sync`])
    /// when `line`, read while the last command's reply is awaited, is a
    /// reply only another command gets (`pdm mode entry`, `type=`, `req:` /
    /// `req error`, a bare `ok` to `pdm open` / `pdm parm`; `ver<n>` only
    /// before `pdm mode entry`); for readers that take every line, like the
    /// automatic list.
    pub fn reject_foreign(&mut self, line: &str) -> Result<(), Error> {
        let foreign = self
            .current
            .as_ref()
            .is_some_and(|command| belongs_to_another_command(command, line, self.opened));
        if foreign {
            return Err(self.mark_out_of_sync(line));
        }
        Ok(())
    }

    /// Write `command` and remember it as the one whose reply is awaited.
    fn write(&mut self, command: &Command) -> Result<(), Error> {
        if *command == Command::PdmOpen {
            self.opened = false;
        }
        self.current = Some(command.clone());
        self.write_command(&command.render())
    }

    /// Risk of `command` given the capabilities seen so far.
    pub fn risk(&self, command: &Command) -> Risk {
        super::risk::classify(command, &self.caps)
    }

    /// The next reply item (line or block) within `timeout`; blocks also
    /// update [`capabilities`](Self::capabilities).
    pub fn next_item(&mut self, timeout: Duration) -> Result<ReplyItem, Error> {
        let deadline = self.deadline(timeout);
        self.read_item(deadline, "a reply")
    }

    /// Read until a line starting with one of `prefixes` arrives and return
    /// it; other lines are skipped, blocks update the capabilities. A line
    /// that is unmistakably another command's reply (see
    /// [`PdmSession::reject_foreign`]) stops the wait with
    /// [`Error::OutOfSync`], and the session refuses every later command.
    pub fn wait_for(&mut self, prefixes: &[&str], timeout: Duration) -> Result<String, Error> {
        let deadline = self.deadline(timeout);
        self.wait_until(prefixes, deadline)
    }

    /// [`PdmSession::wait_for`] with an absolute deadline on the clock.
    fn wait_until(&mut self, prefixes: &[&str], deadline: u64) -> Result<String, Error> {
        let expected = prefixes.join(" | ");
        loop {
            // Checked every round, so a device that keeps sending other
            // lines cannot hold the wait open past the deadline.
            if self.clock.now_ms() >= deadline {
                return Err(Error::Timeout { expected });
            }
            if let ReplyItem::Line(line) = self.read_item(deadline, &expected)? {
                if prefixes.iter().any(|p| line.starts_with(p)) {
                    return Ok(line);
                }
                self.reject_foreign(&line)?;
            }
        }
    }

    /// The session clock's time, ms since the Unix epoch (for transcripts).
    pub fn now_ms(&mut self) -> u64 {
        self.clock.now_ms()
    }

    /// Capabilities from every block seen so far, solicited or not.
    pub fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    /// Swap in a freshly reopened port and re-open the PDM session on it
    /// before anything else (the device drops the text session on reopen).
    pub fn reopen(&mut self, port: P) -> Result<(), Error> {
        // The old handle's text session is already dead on the device, so
        // it is released without `pdm close`. [device]
        drop(std::mem::replace(&mut self.port, port));
        self.reader = ReplyReader::new();
        self.closed = false;
        self.current = None;
        self.out_of_sync = None;
        // Another charger may be attached now: its offers are unknown until
        // its blocks arrive, so the risk gate must not use the old ones.
        self.caps = Capabilities::default();
        self.handshake()
    }

    /// Write one command's bytes: binary-frame guard, then pacing on the
    /// clock so writes are at least [`MIN_COMMAND_GAP`] apart, then a
    /// single `write` of the exact bytes. \[static\] \[device\]
    fn write_command(&mut self, bytes: &[u8]) -> Result<(), Error> {
        guard(bytes)?;
        if let Some(last) = self.last_write_ms {
            let gap = u64::try_from(MIN_COMMAND_GAP.as_millis()).unwrap_or(u64::MAX);
            let due = last.saturating_add(gap);
            let now = self.clock.now_ms();
            if now < due {
                self.clock.sleep(Duration::from_millis(due - now));
            }
        }
        let mut written = 0;
        while written < bytes.len() {
            let rest = bytes.get(written..).unwrap_or_default();
            match self.port.write(rest) {
                Ok(0) => return Err(Error::Disconnected),
                Ok(n) => written += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return Err(map_io(&e)),
            }
        }
        self.port.flush().map_err(|e| map_io(&e))?;
        self.last_write_ms = Some(self.clock.now_ms());
        Ok(())
    }

    /// Clock time (ms) `timeout` from now.
    fn deadline(&mut self, timeout: Duration) -> u64 {
        let ms = u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX);
        self.clock.now_ms().saturating_add(ms)
    }

    /// The next reply item before the clock reaches `deadline`; blocks update
    /// the capabilities. An idle read sleeps [`IDLE_POLL`] on the clock.
    fn read_item(&mut self, deadline: u64, expected: &str) -> Result<ReplyItem, Error> {
        let mut chunk = [0u8; READ_CHUNK];
        loop {
            if let Some(item) = self.reader.next_item() {
                let item = item?;
                match &item {
                    ReplyItem::Block(block) => self.caps.update(block),
                    ReplyItem::Line(line) if line.starts_with("pdm mode entry") => {
                        self.opened = true;
                    }
                    ReplyItem::Line(_) => {}
                }
                return Ok(item);
            }
            if self.clock.now_ms() >= deadline {
                return Err(Error::Timeout {
                    expected: expected.to_string(),
                });
            }
            match self.port.read(&mut chunk) {
                Ok(0) => self.clock.sleep(IDLE_POLL),
                Ok(n) => self.reader.push(chunk.get(..n).unwrap_or_default()),
                Err(e) => match e.kind() {
                    std::io::ErrorKind::TimedOut
                    | std::io::ErrorKind::WouldBlock
                    | std::io::ErrorKind::Interrupted => self.clock.sleep(IDLE_POLL),
                    _ => return Err(map_io(&e)),
                },
            }
        }
    }

    /// Send `pdm close`, wait for `ok`, and release the port.
    pub fn close(mut self) -> Result<(), Error> {
        self.send_close(REPLY_TIMEOUT)
    }
}

/// Bytes requested per port read.
const READ_CHUNK: usize = 256;

/// A command's text, for error messages.
fn command_text(command: &Command) -> String {
    String::from_utf8_lossy(&command.render()).into_owned()
}

/// Whether `line`, read while waiting for `command`'s reply, is a reply
/// that only another command gets: `pdm mode entry` / `ver<n>` (`pdm open`),
/// `type=` (`pdm parm`), `req:` / `req error` (`pd req=`), and a bare `ok`
/// for the two commands that never answer `ok` (`pdm open`, `pdm parm`).
/// Status lines the meter sends unprompted (`cc1 attach`, `ready:`, blocks)
/// are never taken as foreign. \[device\] 2026-10-23: a lagging meter
/// answered `pdm open` with `ok`, `pdm close` with `pdm mode entry` /
/// `ver1.0` and `pdm parm` with `req error 252`.
///
/// `ver<n>` counts only while no `pdm mode entry` came since the last
/// `pdm open` (`opened` false): after it, a late `ver1.0` is the rest of
/// that reply. A `req:` the meter might send unprompted while another reply
/// is awaited still counts (fail-closed) \[inferred\]: the latch leaves the
/// way back to 5 V open ([`PdmSession::send_safety`]).
fn belongs_to_another_command(command: &Command, line: &str, opened: bool) -> bool {
    let l = line.trim();
    let version = !opened
        && l.strip_prefix("ver")
            .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()));
    let open = l.starts_with("pdm mode entry") || version;
    let parm = l.starts_with("type=");
    let req = l.starts_with("req:") || l.starts_with("req error");
    let ok = l == "ok";
    match command {
        Command::PdmOpen => parm || req || ok,
        Command::PdmParm => open || req || ok,
        Command::PdRequest { .. } => open || parm,
        // The handshake's `reset` may still find the `ver1.0` of `pdm open`.
        Command::Reset => parm || req,
        // Raw console text may be any command: its replies are not judged.
        Command::Raw(_) => false,
        _ => open || parm || req,
    }
}

/// [`Error::Refused`] for a command that is never sent (GoodCRC).
fn refuse(command: &Command) -> Result<(), Error> {
    match super::message::refusal(command) {
        Some(reason) => Err(Error::Refused {
            command: command_text(command),
            reason,
        }),
        None => Ok(()),
    }
}

/// The binary-frame guard: refuse bytes whose first byte maps to a
/// forbidden frame type or to a CDC-unsupported type.
fn guard(bytes: &[u8]) -> Result<(), Error> {
    use crate::transport::Error as TransportError;
    let mapped = |e: TransportError| match e {
        TransportError::Forbidden { ty } => Error::Forbidden { ty },
        TransportError::UnsupportedOnCdc { ty } => Error::UnsupportedOnCdc { ty },
        other => Error::Io(other.to_string()),
    };
    crate::transport::guard::check(bytes).map_err(mapped)?;
    crate::transport::cdc::check_cdc(bytes).map_err(mapped)
}

impl<P: Read + Write, C: Clock> Drop for PdmSession<P, C> {
    fn drop(&mut self) {
        // Best effort: the port may already be gone.
        // Bounded, so a destructor never stalls a UI thread for long.
        let _ = self.send_close(DROP_CLOSE_WAIT);
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::collections::{HashMap, VecDeque};
    use std::rc::Rc;

    use super::*;
    use crate::pdm::Protocol;

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

    #[derive(Default, Debug)]
    struct Script {
        replies: HashMap<Vec<u8>, Vec<u8>>,
        writes: Vec<String>,
        pending: VecDeque<u8>,
    }

    /// A port answering exact command bytes with scripted replies.
    #[derive(Clone, Default, Debug)]
    struct Port(Rc<RefCell<Script>>);

    impl Port {
        fn on(self, cmd: &str, reply: &[u8]) -> Port {
            self.0
                .borrow_mut()
                .replies
                .insert(cmd.as_bytes().to_vec(), reply.to_vec());
            self
        }
        fn writes(&self) -> Vec<String> {
            self.0.borrow().writes.clone()
        }
    }

    impl Read for Port {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let mut s = self.0.borrow_mut();
            if s.pending.is_empty() {
                return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "idle"));
            }
            let n = buf.len().min(s.pending.len());
            for b in buf.iter_mut().take(n) {
                *b = s.pending.pop_front().unwrap_or_default();
            }
            Ok(n)
        }
    }

    impl Write for Port {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let mut s = self.0.borrow_mut();
            s.writes.push(String::from_utf8_lossy(buf).into_owned());
            if let Some(r) = s.replies.get(buf).cloned() {
                s.pending.extend(r);
            }
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn port(open_reply: &[u8]) -> Port {
        Port::default()
            .on("pdm open", open_reply)
            .on("pdm close", b"ok\n")
    }

    fn open(port: &Port) -> PdmSession<Port, FakeClock> {
        PdmSession::open_with_clock(port.clone(), FakeClock(Rc::new(Cell::new(1_000))))
            .expect("pdm open")
    }

    fn out_of_sync(e: &Error) -> bool {
        matches!(e, Error::OutOfSync { .. })
    }

    /// A session latched out of sync: `pdm parm` answered by `req error 252`.
    fn latched(port: Port) -> (Port, PdmSession<Port, FakeClock>) {
        let port = port.on("pdm parm", b"req error 252\n");
        let mut s = open(&port);
        s.send(&Command::PdmParm).expect("send");
        let err = s.wait_for(&["type="], REPLY_TIMEOUT).expect_err("foreign");
        assert!(out_of_sync(&err), "{err:?}");
        (port, s)
    }

    fn req(position: u8, current_ma: Option<u32>) -> Command {
        Command::PdRequest {
            position,
            voltage_mv: None,
            current_ma,
        }
    }

    // ---- the safety write path ----

    #[test]
    fn a_latched_session_writes_pd_hrst_on_the_safety_path_and_nothing_else() {
        let (port, mut s) = latched(port(b"pdm mode entry\nver1.0\n").on("pd hrst", b"type=0\n"));
        let before = port.writes().len();
        // The normal paths stay shut, even for the way back to 5 V.
        for command in [req(1, None), Command::PdHardReset, Command::Reset] {
            let err = s.send(&command).expect_err("latched");
            assert!(out_of_sync(&err), "{err:?}");
        }
        assert_eq!(port.writes().len(), before);
        // The safety path writes it, and a foreign reply to it is not read.
        s.send_safety(&Command::PdHardReset).expect("safety write");
        assert_eq!(&port.writes()[before..], ["pd hrst"]);
        assert!(out_of_sync(&s.check_in_step().unwrap_err()));
    }

    #[test]
    fn the_safety_path_refuses_every_other_command() {
        let (port, mut s) = latched(port(b"pdm mode entry\nver1.0\n"));
        let before = port.writes().len();
        for command in [
            req(2, None),
            req(1, Some(3000)),
            Command::PdRequest {
                position: 1,
                voltage_mv: Some(5000),
                current_ma: Some(1000),
            },
            Command::Entry(Protocol::Pd),
            Command::PdmParm,
            Command::PdmOpen,
            Command::Raw("pd hrst".into()),
        ] {
            let err = s.send_safety(&command).expect_err("not a safety command");
            assert!(matches!(err, Error::Refused { .. }), "{command:?}: {err:?}");
        }
        assert_eq!(
            port.writes().len(),
            before,
            "{:?}",
            &port.writes()[before..]
        );
    }

    #[test]
    fn pd_req_1_and_reset_are_safety_commands_too_and_pdm_close_follows() {
        let (port, mut s) = latched(port(b"pdm mode entry\nver1.0\n"));
        let before = port.writes().len();
        s.send_safety(&req(1, None)).expect("pd req=1");
        s.send_safety(&Command::Reset).expect("reset");
        s.close_in_place().expect("pdm close");
        drop(s);
        assert_eq!(&port.writes()[before..], ["pd req=1", "reset", "pdm close"]);
    }

    // ---- foreign replies: intended false positives and the narrowing ----

    #[test]
    fn an_unsolicited_req_while_waiting_for_pdm_parm_latches_fail_closed() {
        // [inferred] Fail-closed on purpose: the safety path stays open.
        let port = port(b"pdm mode entry\nver1.0\n")
            .on("pdm parm", b"req:5000mV,3000mA\ntype=0,em=2,sink=0\n");
        let mut s = open(&port);
        s.send(&Command::PdmParm).expect("send");
        let err = s
            .wait_for(&["type="], REPLY_TIMEOUT)
            .expect_err("req: is foreign");
        assert!(out_of_sync(&err), "{err:?}");
        s.send_safety(&Command::PdHardReset)
            .expect("way back stays open");
    }

    #[test]
    fn a_late_ver_after_pdm_mode_entry_does_not_latch_the_next_command() {
        // `pdm open` answered by `pdm mode entry` only; its `ver1.0` comes
        // with the next command's reply.
        let port = port(b"pdm mode entry\n").on("entry bc", b"ver1.0\nfail\n");
        let mut s = open(&port);
        s.send(&Command::Entry(Protocol::Bc)).expect("send");
        let line = s.wait_for(&["fail"], REPLY_TIMEOUT).expect("own reply");
        assert_eq!(line, "fail");
        s.check_in_step().expect("still in step");
    }

    #[test]
    fn ver_is_foreign_only_before_pdm_mode_entry() {
        assert!(belongs_to_another_command(
            &Command::PdmParm,
            "ver1.0",
            false
        ));
        assert!(!belongs_to_another_command(
            &Command::PdmParm,
            "ver1.0",
            true
        ));
        assert!(!belongs_to_another_command(&req(2, None), "ver1.0", true));
        // `pdm mode entry` itself always is.
        assert!(belongs_to_another_command(
            &Command::PdmParm,
            "pdm mode entry",
            true
        ));
    }

    #[test]
    fn req_error_and_req_stay_foreign_for_pdm_parm() {
        for opened in [false, true] {
            assert!(belongs_to_another_command(
                &Command::PdmParm,
                "req error 252",
                opened
            ));
            assert!(belongs_to_another_command(
                &Command::PdmParm,
                "req:5000mV,3000mA",
                opened
            ));
        }
    }

    #[test]
    fn reject_foreign_latches_for_the_awaited_command() {
        let port = port(b"pdm mode entry\nver1.0\n");
        let mut s = open(&port);
        s.send(&Command::Entry(Protocol::List)).expect("send");
        s.reject_foreign("qc2.0:5V,9V,12V").expect("a list line");
        s.reject_foreign("ver1.0").expect("rest of the open reply");
        let err = s.reject_foreign("type=0,em=2,sink=0").expect_err("foreign");
        assert!(out_of_sync(&err), "{err:?}");
        assert!(out_of_sync(&s.check_in_step().unwrap_err()));
    }
}
