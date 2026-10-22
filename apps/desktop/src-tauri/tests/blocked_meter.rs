//! FR KQF5GM (AC-KQF5GM.2): the worker's blocked-time meter. A worker
//! spawned with a [`BlockedMeter`] adds up the wall time it spends blocked
//! inside its own calls (sink hand-off, channel send, store lock, meter
//! I/O), so the slow-sink test can tell a stall in the worker from a runner
//! that took the poll thread off the CPU. The store-lock case lives in the
//! worker's unit tests (`src/worker.rs`): the lock is crate-private.
//!
//! Real time, the demo simulator behind the transport. Every identifier is
//! synthetic.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use open_powerz_desktop_lib::store::SessionStore;
use open_powerz_desktop_lib::worker::{self, BackendEvent, BlockedMeter, WorkerConfig};
use powerz_protocol::decode::Rate;
use powerz_protocol::frame::{self, Attribute};
use powerz_protocol::session::SystemClock;
use powerz_protocol::transport::{self, Transport};
use powerz_sim::{Persona, Scenario, SimDevice};

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

/// Stream for `run` with a meter whose queue polls from `slow_from` on take
/// `delay` longer; returns the meter's total, the queue polls made and the
/// wall time the stream ran.
fn stream(slow_from: usize, delay: Duration, run: Duration) -> (Duration, usize, Duration) {
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
    let started = Instant::now();
    let handle = worker::spawn_metered(
        open,
        SystemClock,
        WorkerConfig {
            rate: Rate::Sps1000,
            session_id: 1,
        },
        SessionStore::new(),
        sink,
        Arc::new(AtomicBool::new(false)),
        meter.clone(),
    );
    std::thread::sleep(run);
    let total = meter.total();
    let ran = started.elapsed();
    handle.disconnect();
    assert!(*events.lock().unwrap() > 0, "the worker sent no event");
    (total, polls.load(Ordering::SeqCst), ran)
}

#[test]
fn the_meter_adds_up_the_time_blocked_in_meter_io() {
    // From the third queue poll on, each one blocks 30 ms in the transport.
    let (total, polls, ran) = stream(3, Duration::from_millis(30), Duration::from_millis(600));
    let slow = polls.saturating_sub(3);
    assert!(slow >= 3, "too few slow polls to measure: {polls}");
    let floor = Duration::from_millis(30) * u32::try_from(slow - 1).unwrap_or(u32::MAX);
    assert!(
        total >= floor,
        "{slow} polls blocked 30 ms each in meter I/O, but the meter read {total:?}"
    );
    assert!(
        total <= ran,
        "the meter read {total:?} in {ran:?} of wall time"
    );
}

#[test]
fn the_meter_counts_blocked_time_not_wall_time() {
    // The demo meter answers at once: the stream is mostly asleep.
    let (total, polls, ran) = stream(usize::MAX, Duration::ZERO, Duration::from_millis(600));
    assert!(polls >= 5, "the worker barely polled: {polls}");
    assert!(
        total < ran / 4,
        "an idle stream read {total:?} blocked in {ran:?}"
    );
}
