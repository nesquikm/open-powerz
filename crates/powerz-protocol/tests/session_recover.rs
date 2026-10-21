//! CONNECT recovery after an aborted transfer (FR X2SPDW, AC-X2SPDW.9).
//!
//! A meter left mid-transfer by a quit can hold a stale DISCONNECT
//! (`03 xx 00 00`, unrelated id) and a halted bulk pipe. CONNECT then gets no
//! matching ACCEPT; the session asks the transport to recover (clear the halt
//! on both bulk endpoints, drain stale data) and retries CONNECT once.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use powerz_protocol::session::Session;
use powerz_protocol::transport::{Error, GuardedTransport, Transport};

/// CONNECT message type.
const CONNECT: u8 = 0x02;

/// What the fake meter saw.
#[derive(Debug, Default)]
struct Seen {
    /// Every CONNECT request with the timeout it was sent with.
    connects: Vec<(Vec<u8>, Duration)>,
    /// `recover` calls.
    recovers: usize,
}

/// How the fake meter answers CONNECT.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// ACCEPT at once.
    Healthy,
    /// A stale DISCONNECT with an unrelated id, then nothing, until the
    /// transport recovers; ACCEPT after that.
    StaleUntilRecovered,
    /// Never answers.
    Dead,
    /// The meter is gone: every transfer fails as unplugged.
    Unplugged,
}

struct FakeMeter {
    mode: Mode,
    recovered: bool,
    seen: Arc<Mutex<Seen>>,
}

impl FakeMeter {
    fn new(mode: Mode) -> (FakeMeter, Arc<Mutex<Seen>>) {
        let seen = Arc::new(Mutex::new(Seen::default()));
        (
            FakeMeter {
                mode,
                recovered: false,
                seen: Arc::clone(&seen),
            },
            seen,
        )
    }
}

impl Transport for FakeMeter {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, Error> {
        assert_eq!(
            request.first().map(|b| b & 0x7F),
            Some(CONNECT),
            "only CONNECT is expected here: {request:02x?}"
        );
        self.seen
            .lock()
            .unwrap()
            .connects
            .push((request.to_vec(), timeout));
        let id = request[1];
        match self.mode {
            Mode::Healthy => Ok(vec![0x05, id, 0x00, 0x00]),
            Mode::StaleUntilRecovered if self.recovered => Ok(vec![0x05, id, 0x00, 0x00]),
            // The leftover DISCONNECT of the earlier session, with an id that
            // matches nothing sent now.
            Mode::StaleUntilRecovered => Ok(vec![0x03, id ^ 0x55, 0x00, 0x00]),
            Mode::Dead => Err(Error::Timeout),
            Mode::Unplugged => Err(Error::Disconnected),
        }
    }

    fn read(&mut self, _timeout: Duration) -> Result<Vec<u8>, Error> {
        // Nothing else is waiting on the IN pipe.
        Err(Error::Timeout)
    }

    fn recover(&mut self) -> Result<(), Error> {
        self.seen.lock().unwrap().recovers += 1;
        self.recovered = true;
        Ok(())
    }
}

#[test]
fn a_stale_disconnect_is_skipped_and_connect_is_retried_once_after_recovery() {
    let (meter, seen) = FakeMeter::new(Mode::StaleUntilRecovered);
    // Through the guard, as the app and the CLI open every pipe.
    let mut s = Session::open(GuardedTransport::new(meter));
    s.connect()
        .expect("CONNECT after recovering the pipe must succeed");
    let seen = seen.lock().unwrap();
    assert_eq!(seen.recovers, 1, "the pipe was not recovered exactly once");
    assert_eq!(seen.connects.len(), 2, "CONNECT is retried exactly once");
}

#[test]
fn the_first_connect_waits_at_most_500_ms_for_its_accept() {
    let (meter, seen) = FakeMeter::new(Mode::StaleUntilRecovered);
    let mut s = Session::open(meter);
    let _ = s.connect();
    let seen = seen.lock().unwrap();
    let (_, first_timeout) = seen.connects.first().expect("no CONNECT sent");
    assert!(
        *first_timeout <= Duration::from_millis(500),
        "the first CONNECT waits {first_timeout:?}, more than 500 ms"
    );
}

#[test]
fn a_meter_that_never_accepts_gets_one_recovery_and_one_retry_then_fails() {
    let (meter, seen) = FakeMeter::new(Mode::Dead);
    let mut s = Session::open(meter);
    assert!(s.connect().is_err(), "a dead meter connected");
    let seen = seen.lock().unwrap();
    assert_eq!(seen.recovers, 1, "recovered {} times", seen.recovers);
    assert_eq!(
        seen.connects.len(),
        2,
        "CONNECT sent {} times",
        seen.connects.len()
    );
}

#[test]
fn a_healthy_meter_connects_without_recovery() {
    let (meter, seen) = FakeMeter::new(Mode::Healthy);
    let mut s = Session::open(meter);
    s.connect().expect("CONNECT");
    let seen = seen.lock().unwrap();
    assert_eq!(seen.recovers, 0, "recovered a healthy pipe");
    assert_eq!(seen.connects.len(), 1);
}

#[test]
fn the_default_recovery_does_nothing_and_succeeds() {
    /// A transport that does not override `recover`.
    struct Plain;
    impl Transport for Plain {
        fn transact(&mut self, _request: &[u8], _timeout: Duration) -> Result<Vec<u8>, Error> {
            Err(Error::Timeout)
        }
        fn read(&mut self, _timeout: Duration) -> Result<Vec<u8>, Error> {
            Err(Error::Timeout)
        }
    }
    assert_eq!(Plain.recover(), Ok(()));
}

#[test]
fn an_unplugged_meter_is_reported_as_such_without_recovery() {
    // Recovery cannot help a meter that is gone, and its own failure would
    // hide the real cause from the user.
    let (meter, seen) = FakeMeter::new(Mode::Unplugged);
    let mut session = Session::open(GuardedTransport::new(meter));
    let err = session.connect().expect_err("an unplugged meter connected");
    assert!(
        matches!(
            err,
            powerz_protocol::session::Error::Transport(Error::Disconnected)
        ),
        "{err:?}"
    );
    let seen = seen.lock().unwrap();
    assert_eq!(seen.recovers, 0, "recovery ran for an unplugged meter");
    assert_eq!(seen.connects.len(), 1, "CONNECT was retried");
}
