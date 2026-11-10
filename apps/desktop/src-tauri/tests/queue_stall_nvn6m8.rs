//! FR NVN6M8 AC-NVN6M8.4: the desktop worker tells the UI when the meter
//! stops filling the ADC queue (another program polls it over CDC) and when
//! samples come back, under the web edition's rule: 2 s of unheld queue
//! polling with no records sends `stall { stalled: true }` once; the first
//! records send `stall { stalled: false }`. Held polling does not count, and
//! shorter empty runs say nothing.
//!
//! Over the simulator on its fake clock, behind a pipe whose queue replies can
//! be switched to the bare `41 id 02 00` the meter sends while the CLI polls
//! it over CDC.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use common::*;
use open_powerz_desktop_lib::worker::{BackendEvent, Worker, WorkerConfig};
use powerz_protocol::decode::Rate;
use powerz_protocol::frame::{self, Attribute};
use powerz_protocol::transport::{self, Transport};

/// The simulator's pipe, with a switch that empties every queue reply.
struct Stalling {
    inner: Pipe,
    stalled: Arc<AtomicBool>,
}

fn is_queue_poll(req: &[u8]) -> bool {
    frame::get_data_mask(req).is_some_and(|m| m.bits() & Attribute::ADC_QUEUE.bits() != 0)
}

impl Transport for Stalling {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, transport::Error> {
        if self.stalled.load(Ordering::SeqCst) && is_queue_poll(request) {
            // [device] The meter's queue reply while another program polls ADC
            // snapshots over CDC: a bare PUT_DATA at level 1, nothing new.
            return Ok(vec![0x41, request[1], 0x02, 0x00]);
        }
        self.inner.transact(request, timeout)
    }

    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, transport::Error> {
        self.inner.read(timeout)
    }

    fn settings_pipe(&mut self) -> Option<Result<Box<dyn Transport>, transport::Error>> {
        self.inner.settings_pipe()
    }
}

struct Run {
    worker: Worker<Stalling, SimClock>,
    clock: SimClock,
    stalled: Arc<AtomicBool>,
    events: Events,
}

fn start(rate: Rate) -> Run {
    let clock = clock();
    let stalled = Arc::new(AtomicBool::new(false));
    let pipe = Stalling {
        inner: Pipe::sim(
            Scenario::Load5v1a,
            FaultPlan::new(),
            &clock,
            PipeOpts::default(),
        ),
        stalled: Arc::clone(&stalled),
    };
    let (events, sink) = capture(&clock);
    let worker = Worker::start(pipe, clock.clone(), WorkerConfig::new(rate, 1), sink)
        .unwrap_or_else(|e| panic!("worker did not start: {e:?}"));
    Run {
        worker,
        clock,
        stalled,
        events,
    }
}

impl Run {
    fn run(&mut self, ms: u64) {
        run_for(&mut self.worker, &self.clock, ms);
    }

    fn stall(&self, on: bool) {
        self.stalled.store(on, Ordering::SeqCst);
    }

    /// The `stalled` flags of the stall events so far, in order.
    fn stalls(&self) -> Vec<bool> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(_, e)| match e {
                BackendEvent::Stall(s) => Some(s.stalled),
                _ => None,
            })
            .collect()
    }

    fn sample_count(&self) -> usize {
        samples(&self.events).len()
    }
}

#[test]
fn nvn6m8_two_seconds_of_empty_queue_polls_send_one_stall_event() {
    let mut run = start(Rate::Sps50);
    run.run(1_000);
    assert!(run.sample_count() > 0, "the stream never delivered samples");
    assert_eq!(run.stalls(), Vec::<bool>::new());

    run.stall(true);
    run.run(1_500);
    assert_eq!(run.stalls(), Vec::<bool>::new(), "a stall before 2 s");
    run.run(1_500);
    assert_eq!(run.stalls(), vec![true]);
    run.run(5_000);
    assert_eq!(run.stalls(), vec![true], "a long stall reports once");
}

#[test]
fn nvn6m8_the_stall_event_serializes_as_the_ui_reads_it() {
    let mut run = start(Rate::Sps50);
    run.run(1_000);
    run.stall(true);
    run.run(3_000);
    let events = run.events.lock().unwrap();
    let stall = events
        .iter()
        .find(|(_, e)| matches!(e, BackendEvent::Stall(_)))
        .map(|(_, e)| e.clone())
        .expect("no stall event");
    assert_eq!(
        serde_json::to_value(&stall).unwrap(),
        serde_json::json!({ "type": "stall", "data": { "stalled": true } })
    );
}

#[test]
fn nvn6m8_samples_coming_back_end_the_stall_and_a_new_stall_reports_again() {
    let mut run = start(Rate::Sps50);
    run.run(1_000);
    run.stall(true);
    run.run(3_000);
    assert_eq!(run.stalls(), vec![true]);

    let before = run.sample_count();
    run.stall(false);
    run.run(1_000);
    assert!(run.sample_count() > before, "no samples after the stall");
    assert_eq!(run.stalls(), vec![true, false]);
    run.run(3_000);
    assert_eq!(run.stalls(), vec![true, false]);

    run.stall(true);
    run.run(3_000);
    assert_eq!(run.stalls(), vec![true, false, true]);
}

#[test]
fn nvn6m8_empty_runs_shorter_than_two_seconds_say_nothing() {
    let mut run = start(Rate::Sps50);
    run.run(1_000);
    for _ in 0..3 {
        run.stall(true);
        run.run(1_500);
        run.stall(false);
        run.run(1_000);
    }
    assert_eq!(run.stalls(), Vec::<bool>::new());
}

#[test]
fn nvn6m8_a_normal_session_never_stalls_at_any_rate() {
    for rate in [Rate::Sps2, Rate::Sps10, Rate::Sps50, Rate::Sps1000] {
        let mut run = start(rate);
        run.run(10_000);
        assert!(run.sample_count() > 0, "{rate:?}: no samples");
        assert_eq!(run.stalls(), Vec::<bool>::new(), "{rate:?}: a stall");
    }
}

#[test]
fn nvn6m8_held_polling_does_not_count_toward_the_two_seconds() {
    let mut run = start(Rate::Sps50);
    run.run(1_000);
    run.stall(true);
    run.run(1_200);
    run.worker.hold_polling(true);
    run.run(5_000);
    assert_eq!(run.stalls(), Vec::<bool>::new(), "held time counted");
    run.worker.hold_polling(false);
    // About 1.7 s of unheld empty polling in all: still no stall.
    run.run(500);
    assert_eq!(run.stalls(), Vec::<bool>::new());
    // The unheld time goes on adding up past 2 s.
    run.run(1_500);
    assert_eq!(run.stalls(), vec![true]);
}
