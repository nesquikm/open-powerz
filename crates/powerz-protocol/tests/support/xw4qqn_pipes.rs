//! Hand-written pipes for the FR XW4QQN tests: a scripted pipe that
//! records every call (with the timeout it was given), and a field-fitted
//! old bootloader (2.1) that numbers its replies itself and answers the
//! first CONNECT late.
//!
//! Field bytes are from one trace; every other value is synthetic.

#![allow(dead_code)]

use std::collections::VecDeque;
use std::time::Duration;

use powerz_protocol::session::{ProbeFrame, ProbePipe, ProbeVariant};
use powerz_protocol::transport::{Error as TransportError, TimeoutDirection, Transport};

/// ACCEPT as the 2.1 bootloader sends it, with its own id.
pub fn field_accept(id: u8) -> Vec<u8> {
    vec![0x05, id, 0x00, 0x00]
}

/// HEAD reply as the 2.1 bootloader sends it (field 0), with its own id:
/// `c0 id 01 01`, 8 zero bytes, `ff ff ff ff`, 4 zero bytes.
pub fn field_head(id: u8) -> Vec<u8> {
    let mut r = vec![0xC0, id, 0x01, 0x01];
    r.extend([0u8; 8]);
    r.extend([0xFF; 4]);
    r.extend([0u8; 4]);
    r
}

/// The CONNECT request `02 id 00 00`.
pub fn connect_req(id: u8) -> Vec<u8> {
    vec![0x02, id, 0x00, 0x00]
}

/// The HEAD request `40 id 00 01` and 16 zero bytes.
pub fn head_req(id: u8) -> Vec<u8> {
    let mut r = vec![0x40, id, 0x00, 0x01];
    r.extend([0u8; 16]);
    r
}

/// One call a pipe saw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ev {
    /// `transact(request, timeout)`.
    Transact(Vec<u8>, Duration),
    /// `read(timeout)`.
    Read(Duration),
    /// `recover()`.
    Recover,
    /// `probe_variant(variant, frame, _)`.
    Variant(ProbeVariant, Vec<u8>),
}

/// A pipe that answers each `transact`, each `read` and each variant call
/// from its own queue, in order (a timeout once a queue is empty), and
/// records every call.
#[derive(Debug)]
pub struct Scripted {
    pub transacts: VecDeque<Result<Vec<u8>, TransportError>>,
    pub reads: VecDeque<Result<Vec<u8>, TransportError>>,
    pub variants: VecDeque<Result<Vec<u8>, TransportError>>,
    pub recover_ok: bool,
    pub events: Vec<Ev>,
}

impl Scripted {
    pub fn new(
        transacts: Vec<Result<Vec<u8>, TransportError>>,
        reads: Vec<Result<Vec<u8>, TransportError>>,
    ) -> Scripted {
        Scripted {
            transacts: transacts.into(),
            reads: reads.into(),
            variants: VecDeque::new(),
            recover_ok: true,
            events: Vec::new(),
        }
    }

    /// A pipe that never answers anything.
    pub fn silent() -> Scripted {
        Scripted::new(Vec::new(), Vec::new())
    }

    pub fn with_recover_ok(mut self, ok: bool) -> Scripted {
        self.recover_ok = ok;
        self
    }

    pub fn with_variants(mut self, replies: Vec<Result<Vec<u8>, TransportError>>) -> Scripted {
        self.variants = replies.into();
        self
    }

    /// Timeouts given to `transact`, in order.
    pub fn transact_timeouts(&self) -> Vec<Duration> {
        self.events
            .iter()
            .filter_map(|e| match e {
                Ev::Transact(_, t) => Some(*t),
                _ => None,
            })
            .collect()
    }

    /// Requests given to `transact`, in order.
    pub fn transacted(&self) -> Vec<Vec<u8>> {
        self.events
            .iter()
            .filter_map(|e| match e {
                Ev::Transact(r, _) => Some(r.clone()),
                _ => None,
            })
            .collect()
    }

    /// How many times `recover` was called.
    pub fn recovers(&self) -> usize {
        self.events.iter().filter(|e| **e == Ev::Recover).count()
    }
}

impl Transport for Scripted {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, TransportError> {
        self.events.push(Ev::Transact(request.to_vec(), timeout));
        self.transacts
            .pop_front()
            .unwrap_or(Err(TransportError::Timeout))
    }

    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, TransportError> {
        self.events.push(Ev::Read(timeout));
        self.reads
            .pop_front()
            .unwrap_or(Err(TransportError::Timeout))
    }

    fn recover(&mut self) -> Result<(), TransportError> {
        self.events.push(Ev::Recover);
        if self.recover_ok {
            Ok(())
        } else {
            Err(TransportError::Io("clear-halt refused".into()))
        }
    }
}

impl ProbePipe for Scripted {
    fn probe_variant(
        &mut self,
        variant: ProbeVariant,
        frame: &ProbeFrame,
        _timeout: Duration,
    ) -> Result<Vec<u8>, TransportError> {
        self.events
            .push(Ev::Variant(variant, frame.bytes().to_vec()));
        self.variants
            .pop_front()
            .unwrap_or(Err(TransportError::Timeout))
    }
}

/// The old bootloader of the field trace, fitted to it: replies carry its
/// own counter (from 0, one step per reply sent in time); the first
/// CONNECT is answered only within a wait of at least 800 ms, else its
/// ACCEPT (id 0, counter not advanced) is left pending and is what the
/// next transfer returns, that request getting no reply of its own; the
/// pipe recovery fails; every timeout is the IN read's.
#[derive(Debug, Default)]
pub struct FieldMeter {
    own: u8,
    first_connect_seen: bool,
    pending: Option<Vec<u8>>,
    pub events: Vec<Ev>,
}

impl FieldMeter {
    pub fn new() -> FieldMeter {
        FieldMeter::default()
    }

    fn answer(&mut self, request: &[u8]) -> Result<Vec<u8>, TransportError> {
        let id = self.own;
        let reply = match request.first().map(|b| b & 0x7F) {
            Some(0x02) => field_accept(id),
            Some(0x40) => field_head(id),
            _ => return Err(TransportError::Timeout),
        };
        self.own = self.own.wrapping_add(1);
        Ok(reply)
    }

    pub fn recovers(&self) -> usize {
        self.events.iter().filter(|e| **e == Ev::Recover).count()
    }

    pub fn transact_timeouts(&self) -> Vec<Duration> {
        self.events
            .iter()
            .filter_map(|e| match e {
                Ev::Transact(_, t) => Some(*t),
                _ => None,
            })
            .collect()
    }
}

impl Transport for FieldMeter {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, TransportError> {
        self.events.push(Ev::Transact(request.to_vec(), timeout));
        if let Some(late) = self.pending.take() {
            return Ok(late);
        }
        let is_connect = request.first().map(|b| b & 0x7F) == Some(0x02);
        if is_connect && !self.first_connect_seen {
            self.first_connect_seen = true;
            if timeout < Duration::from_millis(800) {
                self.pending = Some(field_accept(self.own));
                return Err(TransportError::Timeout);
            }
        }
        self.answer(request)
    }

    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, TransportError> {
        self.events.push(Ev::Read(timeout));
        self.pending.take().ok_or(TransportError::Timeout)
    }

    fn recover(&mut self) -> Result<(), TransportError> {
        self.events.push(Ev::Recover);
        Err(TransportError::Io("clear-halt refused".into()))
    }

    fn last_timeout_direction(&self) -> Option<TimeoutDirection> {
        Some(TimeoutDirection::In)
    }
}

impl ProbePipe for FieldMeter {
    fn probe_variant(
        &mut self,
        variant: ProbeVariant,
        frame: &ProbeFrame,
        _timeout: Duration,
    ) -> Result<Vec<u8>, TransportError> {
        self.events
            .push(Ev::Variant(variant, frame.bytes().to_vec()));
        self.answer(frame.bytes())
    }
}
