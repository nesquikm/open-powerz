//! Scripted mock transport for tests: replays expected requests and replies.

use std::collections::VecDeque;
use std::time::Duration;

use super::{Error, Transport};

/// One scripted step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Expect `transact(request)` and answer with `reply`.
    Transact {
        /// Exact request bytes expected.
        request: Vec<u8>,
        /// Reply returned for it.
        reply: Result<Vec<u8>, Error>,
    },
    /// Expect `read()` and answer with `reply`.
    Read {
        /// Reply returned.
        reply: Result<Vec<u8>, Error>,
    },
}

impl Step {
    /// Expect `request`, reply with `reply`.
    pub fn transact(request: impl Into<Vec<u8>>, reply: impl Into<Vec<u8>>) -> Step {
        Step::Transact {
            request: request.into(),
            reply: Ok(reply.into()),
        }
    }

    /// Expect `request`, fail with `err`.
    pub fn transact_err(request: impl Into<Vec<u8>>, err: Error) -> Step {
        Step::Transact {
            request: request.into(),
            reply: Err(err),
        }
    }

    /// Expect a read, return `reply`.
    pub fn read(reply: impl Into<Vec<u8>>) -> Step {
        Step::Read {
            reply: Ok(reply.into()),
        }
    }

    /// Expect a read, fail with `err`.
    pub fn read_err(err: Error) -> Step {
        Step::Read { reply: Err(err) }
    }
}

/// A [`Transport`] that replays a script and fails on any deviation.
#[derive(Debug, Clone, Default)]
pub struct MockTransport {
    script: VecDeque<Step>,
    written: Vec<Vec<u8>>,
}

impl MockTransport {
    /// A mock that will replay `script` in order.
    pub fn new(script: Vec<Step>) -> MockTransport {
        MockTransport {
            script: script.into(),
            written: Vec::new(),
        }
    }

    /// Every request passed to `transact`, in order.
    pub fn written(&self) -> &[Vec<u8>] {
        &self.written
    }

    /// Steps not yet consumed.
    pub fn remaining(&self) -> usize {
        self.script.len()
    }
}

impl Transport for MockTransport {
    fn transact(&mut self, request: &[u8], _timeout: Duration) -> Result<Vec<u8>, Error> {
        self.written.push(request.to_vec());
        match self.script.front() {
            Some(Step::Transact {
                request: expected, ..
            }) if expected == request => {}
            Some(Step::Transact {
                request: expected, ..
            }) => {
                return Err(Error::Mock(format!(
                    "unexpected request: expected {expected:02x?}, got {request:02x?}"
                )));
            }
            Some(Step::Read { .. }) => {
                return Err(Error::Mock(format!(
                    "unexpected request: expected a read, got {request:02x?}"
                )));
            }
            None => {
                return Err(Error::Mock(format!(
                    "unexpected request: script ended, got {request:02x?}"
                )));
            }
        }
        match self.script.pop_front() {
            Some(Step::Transact { reply, .. }) => reply,
            _ => unreachable!("front was checked to be a Transact step"),
        }
    }

    fn read(&mut self, _timeout: Duration) -> Result<Vec<u8>, Error> {
        match self.script.front() {
            Some(Step::Read { .. }) => match self.script.pop_front() {
                Some(Step::Read { reply }) => reply,
                _ => unreachable!("front was checked to be a Read step"),
            },
            Some(Step::Transact { request, .. }) => Err(Error::Mock(format!(
                "unexpected read: expected request {request:02x?}"
            ))),
            None => Err(Error::Mock("unexpected read: script ended".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{Attribute, get_data};

    const T: Duration = Duration::from_millis(1000);

    // ---- AC-SJGK7E.3: Transport trait + MockTransport ----

    #[test]
    fn mock_replays_transact_and_read_in_order() {
        let req = get_data(1, Attribute::ADC);
        let mut m = MockTransport::new(vec![
            Step::transact(req, vec![0x41, 0x01, 0x00, 0x00]),
            Step::read(vec![0x41, 0x02, 0x00, 0x00]),
        ]);
        assert_eq!(m.transact(&req, T).unwrap(), vec![0x41, 0x01, 0x00, 0x00]);
        assert_eq!(m.read(T).unwrap(), vec![0x41, 0x02, 0x00, 0x00]);
        assert_eq!(m.written(), &[req.to_vec()]);
        assert_eq!(m.remaining(), 0);
    }

    #[test]
    fn mock_replays_scripted_errors() {
        let req = get_data(3, Attribute::ADC);
        let mut m = MockTransport::new(vec![
            Step::transact_err(req, Error::Timeout),
            Step::read_err(Error::Disconnected),
        ]);
        assert_eq!(m.transact(&req, T), Err(Error::Timeout));
        assert_eq!(m.read(T), Err(Error::Disconnected));
    }

    #[test]
    fn mock_rejects_unexpected_request_with_clear_message() {
        let expected = get_data(1, Attribute::ADC);
        let sent = get_data(1, Attribute::SETTINGS);
        let mut m = MockTransport::new(vec![Step::transact(expected, vec![0x05, 0x01, 0, 0])]);
        match m.transact(&sent, T) {
            Err(Error::Mock(msg)) => {
                assert!(msg.contains("unexpected request"), "message: {msg}");
                // Both the expected and the actual bytes are shown.
                assert!(
                    msg.contains(&format!("{:02x?}", expected)),
                    "message: {msg}"
                );
                assert!(msg.contains(&format!("{:02x?}", sent)), "message: {msg}");
            }
            other => panic!("expected Error::Mock, got {other:?}"),
        }
    }

    #[test]
    fn mock_rejects_request_after_script_ends() {
        let mut m = MockTransport::new(vec![]);
        match m.transact(&[0x0C, 0x01, 0x02, 0x00], T) {
            Err(Error::Mock(msg)) => assert!(msg.contains("unexpected request"), "{msg}"),
            other => panic!("expected Error::Mock, got {other:?}"),
        }
    }

    #[test]
    fn mock_rejects_read_when_transact_expected() {
        let mut m = MockTransport::new(vec![Step::transact(vec![0x0C, 1, 2, 0], vec![0x41])]);
        assert!(matches!(m.read(T), Err(Error::Mock(_))));
    }

    #[test]
    fn mock_is_usable_as_dyn_transport() {
        let req = [0x0C, 0x09, 0x02, 0x00];
        let mut m = MockTransport::new(vec![Step::transact(req, vec![0xAA])]);
        let t: &mut dyn Transport = &mut m;
        assert_eq!(t.transact(&req, T).unwrap(), vec![0xAA]);
    }
}
