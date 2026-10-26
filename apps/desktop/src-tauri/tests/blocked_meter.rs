//! FR KQF5GM (AC-KQF5GM.2): the worker's blocked-time meter. A worker
//! spawned with a [`BlockedMeter`] adds up the wall time it spends blocked
//! inside its own calls (sink hand-off, channel send, store lock, meter
//! I/O), so the slow-sink test can tell a stall in the worker from a runner
//! that took the poll thread off the CPU. The store-lock case lives in the
//! worker's unit tests (`src/worker.rs`): the lock is crate-private.
//!
//! Real time, the demo simulator behind the transport. Every identifier is
//! synthetic. A CI runner may take the poll thread off the CPU for a long
//! while, inside a wrapped call or not, so neither test runs for a fixed
//! time: each streams until it has seen enough polls (under a generous wall
//! cap), and the idle test checks the meter against the worker's own sleeps
//! as the recording clock logged them, not against a fraction of the wall
//! time.

mod common;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use open_powerz_desktop_lib::store::SessionStore;
use open_powerz_desktop_lib::worker::{self, BackendEvent, BlockedMeter, WorkerConfig};
use powerz_protocol::decode::Rate;
use powerz_protocol::frame::{self, Attribute};
use powerz_protocol::transport::{self, Transport};
use powerz_sim::{Persona, Scenario, SimDevice};

use common::slow_sink::RecordingClock;

/// The demo meter, with every queue poll from number `slow_from` on taking
/// `delay` longer to answer.
struct SlowMeter {
    inner: SimDevice,
    polls: Arc<AtomicUsize>,
    slow_from: usize,
    delay: Duration,
}

impl Transport for SlowMeter {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, transport::Error> {
        let queue_poll = frame::get_data_mask(request)
            .is_some_and(|m| m.bits() & Attribute::ADC_QUEUE.bits() != 0);
        if queue_poll {
            let n = self.polls.fetch_add(1, Ordering::SeqCst) + 1;
            if n >= self.slow_from {
                std::thread::sleep(self.delay);
            }
        }
        self.inner.transact(request, timeout)
    }

    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, transport::Error> {
        self.inner.read(timeout)
    }
}

/// The most wall time a stream may take to reach its polls.
const WALL_CAP: Duration = Duration::from_secs(20);

/// What one metered stream measured.
struct Run {
    /// The meter's total.
    total: Duration,
    /// Queue polls made before the total was read.
    polls: usize,
    /// Wall time from the spawn to the read of the total.
    ran: Duration,
    /// Wall time the poll thread spent in the clock's sleeps that ended
    /// before the total was read.
    slept: Duration,
}

/// Stream with a meter whose queue polls from `slow_from` on take `delay`
/// longer, until `want` queue polls have been made or [`WALL_CAP`] passed.
fn stream(slow_from: usize, delay: Duration, want: usize) -> Run {
    let polls = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&polls);
    let open = move || {
        Ok(SlowMeter {
            inner: SimDevice::new(Persona::Bulk, Scenario::Load5v1a),
            polls: counted,
            slow_from,
            delay,
        })
    };
    let events: Arc<Mutex<usize>> = Arc::new(Mutex::new(0));
    let seen = Arc::clone(&events);
    let sink = move |_event: BackendEvent| *seen.lock().unwrap() += 1;
    let meter = BlockedMeter::new();
    let clock = RecordingClock::new();
    let started = Instant::now();
    let handle = worker::spawn_metered(
        open,
        clock.clone(),
        WorkerConfig::new(Rate::Sps1000, 1),
        SessionStore::new(),
        sink,
        Arc::new(AtomicBool::new(false)),
        meter.clone(),
    );
    while polls.load(Ordering::SeqCst) < want && started.elapsed() < WALL_CAP {
        std::thread::sleep(Duration::from_millis(5));
    }
    // In this order: every poll counted has begun, every sleep logged has
    // ended, before the total is read; the wall time covers all of them.
    let made = polls.load(Ordering::SeqCst);
    let sleeps = clock.evidence();
    let total = meter.total();
    let ran = started.elapsed();
    handle.disconnect();
    assert!(*events.lock().unwrap() > 0, "the worker sent no event");
    assert!(made >= want, "only {made} of {want} queue polls in {ran:?}");
    let slept = sleeps
        .sleeps()
        .iter()
        .map(|s| s.woke.saturating_sub(s.started))
        .sum();
    Run {
        total,
        polls: made,
        ran,
        slept,
    }
}

#[test]
fn the_meter_adds_up_the_time_blocked_in_meter_io() {
    // From the third queue poll on, each one blocks 30 ms in the transport;
    // stream until six of them have begun.
    let delay = Duration::from_millis(30);
    let run = stream(3, delay, 2 + 6);
    // The poll thread makes one poll at a time: all but the last slow poll
    // counted had returned before the total was read.
    let slow = run.polls - 2;
    let floor = delay * u32::try_from(slow - 1).unwrap_or(u32::MAX);
    assert!(
        run.total >= floor,
        "{slow} polls blocked 30 ms each in meter I/O, but the meter read {:?}",
        run.total
    );
    assert!(
        run.total <= run.ran,
        "the meter read {:?} in {:?} of wall time",
        run.total,
        run.ran
    );
}

#[test]
fn the_meter_counts_blocked_time_not_wall_time() {
    // The demo meter answers at once: the stream is mostly asleep.
    let run = stream(usize::MAX, Duration::ZERO, 10);
    // The poll thread is never blocked in its own calls while it sleeps, so
    // the meter and the sleeps fit in the wall time side by side. A meter
    // that counted sleeping would add the sleeps twice.
    assert!(
        run.slept >= run.ran / 2,
        "the idle worker slept only {:?} of {:?}: nothing to tell apart",
        run.slept,
        run.ran
    );
    assert!(
        run.total + run.slept <= run.ran + Duration::from_millis(1),
        "an idle stream read {:?} blocked beside {:?} asleep in {:?} of wall time",
        run.total,
        run.slept,
        run.ran
    );
}
