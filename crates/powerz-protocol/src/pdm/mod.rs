//! PDM text session over the meter's virtual serial (CDC) port (FR PR41FT).
//!
//! The meter does protocol triggering through short ASCII commands
//! (`pdm open`, `entry pd`, `pd req=2`, ...). Replies are `\n`-terminated
//! lines, some of which carry binary capability blocks (`pdo:`, `ufcs:`,
//! `scp:`, `vfcp:`, `tfcp:`); binary protocol frames may interleave on the
//! same byte stream at any time. \[device\] (firmware 2.0.6)
//!
//! - [`reply`]: the pure stream reader (lines, blocks, skipped frames).
//! - [`caps`]: typed capability-block parsers.
//! - [`command`]: the command enum and its exact ASCII rendering.
//! - [`risk`]: the pure risk classifier (read-only / ≤ 5 V / > 5 V).
//! - [`session`]: the paced session with high-voltage confirmation.
//! - [`port`]: finding the meter's serial port.
//! - [`pd`]: USB PD triggering (offers, requests, hard reset, settings).
//! - [`detect`]: charger protocol detection and the D+ / D- hint table.

pub mod caps;
pub mod command;
pub mod detect;
pub mod pd;
pub mod port;
pub mod reply;
pub mod risk;
pub mod session;

pub use caps::{CapBlock, Capabilities, PdoBlock, ScpCaps, TfcpCaps, UfcsCap, UfcsCaps, VfcpCaps};
pub use command::{Command, FixedProtocol, Protocol};
pub use port::{find_port, open_port};
pub use reply::{ReplyItem, ReplyReader};
pub use risk::{Risk, classify};
pub use session::{HighVoltageConfirmation, PdmSession};

/// Which binary capability block a reply carried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockTag {
    /// `pdo:` USB PD source capabilities.
    Pdo,
    /// `ufcs:` UFCS output capabilities.
    Ufcs,
    /// `scp:` Huawei SCP capabilities.
    Scp,
    /// `vfcp:` vivo VFCP capabilities.
    Vfcp,
    /// `tfcp:` Transsion TFCP capabilities.
    Tfcp,
}

/// Errors from the PDM layer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// No matching reply line arrived before the timeout.
    #[error("timed out waiting for {expected}")]
    Timeout {
        /// What the session was waiting for (the expected prefixes).
        expected: String,
    },
    /// A binary capability block was malformed.
    #[error("malformed {tag:?} block: {reason}")]
    Malformed {
        /// The block's tag.
        tag: BlockTag,
        /// What was wrong with it.
        reason: String,
    },
    /// The command is classified above 5 V and no matching confirmation was given.
    #[error("refusing to send {command:?}: it can raise VBUS above 5 V and was not confirmed")]
    HighVoltageNotConfirmed {
        /// The command text.
        command: String,
    },
    /// The guard refused the bytes (their first byte maps to a forbidden frame type).
    #[error("refusing to send: first byte maps to forbidden frame type {ty:#04x}")]
    Forbidden {
        /// Frame type (bits 0..6 of the first byte).
        ty: u8,
    },
    /// The first byte maps to an ADCNEW control type on CDC.
    #[error("refusing to send: first byte maps to CDC frame type {ty:#04x}")]
    UnsupportedOnCdc {
        /// Frame type (bits 0..6 of the first byte).
        ty: u8,
    },
    /// The device sent more than [`reply::MAX_LINE`] bytes without ending
    /// the line (or block, or frame); they were dropped.
    #[error("reply line too long ({len} bytes without a line end); dropped")]
    LineTooLong {
        /// Bytes dropped.
        len: usize,
    },
    /// The reply did not have the expected form.
    #[error("unexpected reply: expected {expected}, got {got:?}")]
    UnexpectedReply {
        /// What was expected.
        expected: String,
        /// What arrived.
        got: String,
    },
    /// No POWER-Z serial port was found.
    #[error("no POWER-Z serial port found")]
    NotFound,
    /// Several POWER-Z serial ports and no serial number to choose between them.
    #[error("{count} POWER-Z serial ports found; pick one by serial number")]
    Ambiguous {
        /// How many candidate ports there were.
        count: usize,
    },
    /// The port went away.
    #[error("meter disconnected")]
    Disconnected,
    /// Any other I/O failure.
    #[error("I/O error: {0}")]
    Io(String),
}

/// Map a port I/O error to a PDM [`Error`]: unplug kinds are
/// [`Error::Disconnected`], everything else [`Error::Io`].
fn map_io(err: &std::io::Error) -> Error {
    match crate::transport::map_io_error(err, std::env::consts::OS) {
        crate::transport::Error::Disconnected => Error::Disconnected,
        _ => Error::Io(err.to_string()),
    }
}
