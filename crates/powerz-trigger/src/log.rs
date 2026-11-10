//! The session's log: every command written to the port, as a line, with
//! the lines before a sink is set kept until it is.

use std::io::{Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Where the session's log lines go.
pub type LogSink = Box<dyn FnMut(String) + Send>;

/// Lines kept for a sink that has not arrived yet; older ones are dropped.
const EARLY_LINES_MAX: usize = 256;

/// The session's log: the sink once it is set, the lines before it kept
/// until it is (`pdm open` goes out while the session opens, before its owner can
/// set one).
#[derive(Default)]
pub struct SessionLogLines {
    sink: Option<LogSink>,
    early: std::collections::VecDeque<String>,
}

impl std::fmt::Debug for SessionLogLines {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionLogLines")
            .field("sink", &self.sink.is_some())
            .field("early", &self.early.len())
            .finish()
    }
}

/// The session's log, shared by the session, its port and its owner.
pub type SharedLog = Arc<std::sync::Mutex<SessionLogLines>>;

/// The log's lines, a poisoned lock taken as is: a panicked writer leaves
/// whole lines behind.
fn lock_log(log: &SharedLog) -> std::sync::MutexGuard<'_, SessionLogLines> {
    log.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Add `line` to the log: handed to the sink, or kept until one is set (at
/// most the latest 256).
pub fn log_shared(log: &SharedLog, line: String) {
    let mut guard = lock_log(log);
    if guard.early.len() >= EARLY_LINES_MAX {
        guard.early.pop_front();
    }
    guard.early.push_back(line);
    // With no sink here, it is not set yet or another call is handing lines
    // to it, which takes this one too.
    if let Some(sink) = guard.sink.take() {
        deliver(log, guard, sink);
    }
}

/// Send the log to `sink` from here on, the lines kept so far first.
pub fn set_log_sink(log: &SharedLog, sink: LogSink) {
    let guard = lock_log(log);
    deliver(log, guard, sink);
}

/// Hands the waiting lines to `sink` one at a time, outside the lock, then
/// puts it back. A line the sink panics on is lost, the rest are not, and the
/// panic goes no further: logging runs in drops too.
fn deliver<'a>(
    log: &'a SharedLog,
    mut guard: std::sync::MutexGuard<'a, SessionLogLines>,
    mut sink: LogSink,
) {
    while let Some(line) = guard.early.pop_front() {
        drop(guard);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sink(line)));
        guard = lock_log(log);
    }
    guard.sink = Some(sink);
}

/// The longest command text logged, in characters.
const SENT_TEXT_MAX: usize = 256;

/// `bytes` as one log line's text: control characters as `\xNN`, at most
/// [`SENT_TEXT_MAX`] characters and a marker, so console text cannot forge
/// log lines.
fn log_text(bytes: &[u8]) -> String {
    let mut text = String::new();
    let mut count = 0;
    for c in String::from_utf8_lossy(bytes).chars() {
        // Control characters are all below U+00A0.
        let shown = if c.is_control() {
            format!("\\x{:02x}", u32::from(c))
        } else {
            c.to_string()
        };
        count += shown.chars().count();
        if count > SENT_TEXT_MAX {
            text.push_str(" [cut]");
            break;
        }
        text.push_str(&shown);
    }
    text
}

/// The PDM port with every command logged as `trigger sent: <text>`: the one
/// place all of the session's writes pass, the library's blind safety-path
/// writes included. Only the command text; no serial, no port name.
pub struct LoggedPort<P> {
    port: P,
    log: SharedLog,
    /// What the port took of the command being written. Commands carry no
    /// terminator: the flush after each one ends it.
    pending: Vec<u8>,
    /// Commands ended so far (one per flush that carried one), shared with
    /// the session so it can tell whether anything went out since.
    sent: Arc<AtomicUsize>,
}

impl<P> LoggedPort<P> {
    /// `port`, its commands logged to `log`.
    pub fn new(port: P, log: SharedLog) -> LoggedPort<P> {
        LoggedPort {
            port,
            log,
            pending: Vec::new(),
            sent: Arc::default(),
        }
    }

    /// The count of commands ended so far, shared.
    pub(crate) fn sent(&self) -> Arc<AtomicUsize> {
        Arc::clone(&self.sent)
    }

    /// The command written so far, as one line.
    fn log_pending(&mut self) {
        if !self.pending.is_empty() {
            self.sent.fetch_add(1, Ordering::Relaxed);
            let text = log_text(std::mem::take(&mut self.pending).trim_ascii());
            log_shared(&self.log, format!("trigger sent: {text}"));
        }
    }
}

impl<P> Drop for LoggedPort<P> {
    fn drop(&mut self) {
        self.log_pending();
    }
}

impl<P: Read> Read for LoggedPort<P> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.port.read(buf)
    }
}

impl<P: Write> Write for LoggedPort<P> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self.port.write(buf) {
            Ok(n) => {
                self.pending.extend_from_slice(buf.get(..n).unwrap_or(buf));
                Ok(n)
            }
            // Nothing was taken, and the caller writes it again.
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => Err(e),
            // It may still have reached the meter: the command, marked failed.
            Err(e) => {
                let mut tried = std::mem::take(&mut self.pending);
                tried.extend_from_slice(buf);
                let text = log_text(tried.trim_ascii());
                log_shared(&self.log, format!("trigger send failed: {text}"));
                Err(e)
            }
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let result = self.port.flush();
        self.log_pending();
        result
    }
}
