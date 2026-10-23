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
        };
        // On any error below, dropping `session` sends `pdm close` before
        // the port is released.
        session.handshake()?;
        Ok(session)
    }

    /// `pdm open`, wait for `pdm mode entry` \[device\], and `reset` if the
    /// device reports `busy` \[static\] \[doc\] (not yet seen on the device).
    fn handshake(&mut self) -> Result<(), Error> {
        self.write_command(&Command::PdmOpen.render())?;
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
            self.write_command(&Command::Reset.render())?;
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
        self.write_command(&Command::PdmClose.render())?;
        self.wait_for(&["ok"], wait).map(|_| ())
    }

    /// Send `command`; refused with [`Error::HighVoltageNotConfirmed`] (and
    /// nothing written) when it classifies [`Risk::Above5V`].
    pub fn send(&mut self, command: &Command) -> Result<(), Error> {
        if self.risk(command) == Risk::Above5V {
            return Err(Error::HighVoltageNotConfirmed {
                command: command_text(command),
            });
        }
        self.write_command(&command.render())
    }

    /// Send `command` with a confirmation made for that exact command.
    pub fn send_confirmed(
        &mut self,
        command: &Command,
        confirmation: HighVoltageConfirmation,
    ) -> Result<(), Error> {
        if confirmation.command != *command {
            return Err(Error::HighVoltageNotConfirmed {
                command: command_text(command),
            });
        }
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
    /// it; other lines are skipped, blocks update the capabilities.
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
            if let ReplyItem::Line(line) = self.read_item(deadline, &expected)?
                && prefixes.iter().any(|p| line.starts_with(p))
            {
                return Ok(line);
            }
        }
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
                if let ReplyItem::Block(block) = &item {
                    self.caps.update(block);
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
