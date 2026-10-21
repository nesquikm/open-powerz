//! CONNECT recovery through the worker (FR X2SPDW, AC-X2SPDW.9): a pipe
//! left mid-transfer answers CONNECT with a stale DISCONNECT; the worker's
//! session recovers the transport and retries once, else asks for a replug.

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::*;
use open_powerz_desktop_lib::ui_error::UiError;
use open_powerz_desktop_lib::worker::{Status, Worker, WorkerConfig};
use powerz_protocol::decode::Rate;
use powerz_protocol::transport::{self, Transport};

fn config(rate: Rate) -> WorkerConfig {
    WorkerConfig {
        rate,
        session_id: 1,
    }
}

fn connected_serial(events: &Events) -> String {
    statuses(events)
        .into_iter()
        .find_map(|s| match s {
            Status::Connected(info) => Some(info.serial),
            _ => None,
        })
        .expect("no connected status")
}

// ---- AC-X2SPDW.9: CONNECT recovery through the worker ----

/// The simulator behind a pipe that answers the first CONNECT with a stale
/// DISCONNECT (unrelated id) and nothing else until it is recovered.
struct StalePipe {
    sim: SimDevice,
    recovered: bool,
    recovers: Arc<Mutex<usize>>,
    /// Never recovers: every CONNECT times out.
    dead: bool,
}

impl Transport for StalePipe {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, transport::Error> {
        let is_connect = request.first().map(|b| b & 0x7F) == Some(0x02);
        if self.dead {
            return Err(transport::Error::Timeout);
        }
        if is_connect && !self.recovered {
            return Ok(vec![0x03, request[1] ^ 0x55, 0x00, 0x00]);
        }
        self.sim.transact(request, timeout)
    }

    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, transport::Error> {
        if !self.recovered {
            return Err(transport::Error::Timeout);
        }
        self.sim.read(timeout)
    }

    fn recover(&mut self) -> Result<(), transport::Error> {
        *self.recovers.lock().unwrap() += 1;
        self.recovered = true;
        // Recovery drains the stale data on the IN pipe.
        while self.sim.read(Duration::from_millis(1)).is_ok() {}
        Ok(())
    }
}

fn stale_pipe(clock: &SimClock, dead: bool) -> (StalePipe, Arc<Mutex<usize>>) {
    let recovers = Arc::new(Mutex::new(0));
    let pipe = StalePipe {
        sim: SimDevice::with_clock(Persona::Bulk, Scenario::Load5v1a, clock.clone()),
        recovered: false,
        recovers: Arc::clone(&recovers),
        dead,
    };
    (pipe, recovers)
}

#[test]
fn the_worker_connects_after_recovering_a_pipe_left_mid_transfer() {
    let clock = clock();
    let (pipe, recovers) = stale_pipe(&clock, false);
    let (events, sink) = capture(&clock);
    let mut worker = Worker::start(pipe, clock.clone(), config(Rate::Sps50), sink)
        .unwrap_or_else(|e| panic!("did not recover: {e:?}"));
    assert_eq!(*recovers.lock().unwrap(), 1);
    assert_eq!(connected_serial(&events), SIM_SERIAL);
    run_for(&mut worker, &clock, 1_000);
    assert!(
        !samples(&events).is_empty(),
        "no samples after the recovery"
    );
}

#[test]
fn a_failed_retry_asks_the_user_to_replug_the_meter() {
    let clock = clock();
    let (pipe, recovers) = stale_pipe(&clock, true);
    let (_events, sink) = capture(&clock);
    let err: UiError = Worker::start(pipe, clock.clone(), config(Rate::Sps50), sink)
        .expect_err("a dead meter connected");
    assert_eq!(
        *recovers.lock().unwrap(),
        1,
        "recover once before giving up"
    );
    let text = format!("{} {}", err.message, err.hint.unwrap_or_default()).to_lowercase();
    assert!(
        text.contains("replug"),
        "the error does not ask to replug: {text}"
    );
}
