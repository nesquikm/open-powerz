//! The read-only firmware probe (FR V9SK56, AC .6): CONNECT, then HEAD,
//! and nothing else, in application or bootloader mode.

use super::{Clock, Error, Session};
use crate::decode::head::{self, HeadReply};
use crate::transport::{self, Transport};

/// How the meter answered CONNECT.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectOutcome {
    /// ACCEPT.
    Accepted,
    /// REJECT.
    Rejected,
    /// No answer, even after one recovery and retry.
    TimedOut,
}

/// How the meter answered HEAD.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeadOutcome {
    /// A HEAD reply with this old-firmware field.
    Field(u32),
    /// A reply that does not decode as HEAD, kept as sent.
    Raw(Vec<u8>),
    /// REJECT, kept as sent.
    Rejected(Vec<u8>),
    /// No answer.
    TimedOut,
}

/// What [`Session::probe`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeReport {
    /// The answer to CONNECT.
    pub connect: ConnectOutcome,
    /// The answer to HEAD.
    pub head: HeadOutcome,
}

impl<T: Transport, C: Clock> Session<T, C> {
    /// Send CONNECT, then (whatever CONNECT gave) one HEAD, and report both
    /// answers. Only CONNECT (0x02) and HEAD (0x40) frames are sent; a
    /// REJECT, a timeout or an undecodable reply is data; any other error
    /// is returned (no HEAD is sent after such a CONNECT failure).
    pub fn probe(&mut self) -> Result<ProbeReport, Error> {
        self.probe_with_head_reply().map(|(report, _)| report)
    }

    /// [`Session::probe`], also returning the HEAD reply exactly as the
    /// meter sent it (`None` when HEAD timed out), so a caller can show the
    /// raw bytes next to a decoded field.
    pub fn probe_with_head_reply(&mut self) -> Result<(ProbeReport, Option<Vec<u8>>), Error> {
        let connect = match self.connect() {
            Ok(()) => ConnectOutcome::Accepted,
            Err(Error::Rejected { .. }) => ConnectOutcome::Rejected,
            Err(Error::Transport(transport::Error::Timeout)) => ConnectOutcome::TimedOut,
            Err(e) => return Err(e),
        };
        let (head, reply) = match self.head_exchange() {
            Ok(reply) if head::is_reject(&reply) => (HeadOutcome::Rejected(reply.clone()), reply),
            Ok(reply) => match HeadReply::decode(&reply) {
                Ok(h) => (HeadOutcome::Field(h.old_firmware_field()), reply),
                Err(_) => (HeadOutcome::Raw(reply.clone()), reply),
            },
            Err(Error::Transport(transport::Error::Timeout)) => {
                return Ok((
                    ProbeReport {
                        connect,
                        head: HeadOutcome::TimedOut,
                    },
                    None,
                ));
            }
            Err(e) => return Err(e),
        };
        Ok((ProbeReport { connect, head }, Some(reply)))
    }
}
