//! AC-4PJRVH.6: after a queue backlog overflow the meter rejected the stop
//! request 0x0F at exit `[device 2026-10-24]`. `powerz stream` keeps what it
//! recorded and exits cleanly with a note. Runs against the simulator on its
//! simulated clock, with a long pause in the polling as the overflow.

use std::sync::atomic::AtomicBool;
use std::time::Duration;

use powerz_cli::StreamOptions;
use powerz_cli::commands::stream;
use powerz_protocol::decode::Rate;
use powerz_protocol::session::Session;
use powerz_protocol::transport::{self, Transport};
use powerz_sim::{Persona, Scenario, SimClock, SimDevice};

const T0: u64 = 1_790_000_000_000;
const GET_DATA: u8 = 0x0C;
const STOP_QUEUE: u8 = 0x0F;
const REJECT: u8 = 0x06;
/// Long enough at 50 SPS for the queue to drop its oldest records.
const PAUSE: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy)]
enum StopReply {
    Rejected,
    NoAnswer,
}

/// The simulated meter with one long pause between polls and a stop that
/// does not go through.
struct PausedMeter {
    sim: SimDevice,
    clock: SimClock,
    polls: usize,
    stop: StopReply,
}

impl Transport for PausedMeter {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, transport::Error> {
        match request.first() {
            Some(&STOP_QUEUE) => match self.stop {
                StopReply::Rejected => return Ok(vec![REJECT, request[1], 0, 0]),
                StopReply::NoAnswer => return Err(transport::Error::Timeout),
            },
            Some(&GET_DATA) => {
                self.polls += 1;
                if self.polls == 20 {
                    self.clock.advance(PAUSE);
                }
            }
            _ => {}
        }
        self.sim.transact(request, timeout)
    }

    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, transport::Error> {
        self.sim.read(timeout)
    }
}

fn session(stop: StopReply) -> Session<PausedMeter, SimClock> {
    let clock = SimClock::new(T0);
    let sim = SimDevice::with_clock(Persona::Bulk, Scenario::Load5v1a, clock.clone());
    let meter = PausedMeter {
        sim,
        clock: clock.clone(),
        polls: 0,
        stop,
    };
    let mut session = Session::with_clock(meter, clock);
    session.connect().expect("CONNECT");
    session
}

fn opts() -> StreamOptions {
    StreamOptions {
        rate: Rate::Sps50,
        duration: Some(Duration::from_secs(45)),
        start_unix_ms: T0,
    }
}

#[test]
fn a_rejected_stop_after_an_overflow_keeps_the_recording_and_adds_a_note() {
    let mut session = session(StopReply::Rejected);
    let (mut csv, mut err) = (Vec::new(), Vec::new());

    let summary = stream(
        &mut session,
        &opts(),
        &AtomicBool::new(false),
        &mut csv,
        &mut err,
    )
    .expect("a rejected stop ends the stream cleanly");

    assert!(!summary.gaps.is_empty(), "no overflow happened");
    assert!(summary.samples > 0);
    let rows = String::from_utf8_lossy(&csv).lines().count() as u64 - 1;
    assert_eq!(rows, summary.samples, "the CSV lost rows");
    let err = String::from_utf8_lossy(&err);
    assert!(err.contains("note:"), "{err}");
    assert!(
        !err.contains("overflow"),
        "gaps are not proof of one: {err}"
    );
    assert!(err.to_lowercase().contains("stop"), "{err}");
    assert!(err.contains("samples recorded"), "{err}");
}

#[test]
fn a_stop_that_gets_no_answer_is_still_an_error() {
    let mut session = session(StopReply::NoAnswer);
    let (mut csv, mut err) = (Vec::new(), Vec::new());

    let result = stream(
        &mut session,
        &opts(),
        &AtomicBool::new(false),
        &mut csv,
        &mut err,
    );

    let e = result.expect_err("a lost stop is not hidden");
    assert!(e.to_string().contains("timed out"), "{e}");
    assert!(
        !String::from_utf8_lossy(&csv).is_empty(),
        "the stream never ran"
    );
}
