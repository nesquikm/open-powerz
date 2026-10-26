//! Shutdown on quit or window close (FR X2SPDW, AC-X2SPDW.11): every running
//! worker stops its queue and releases the interface within 1 s.

mod common;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::*;
use open_powerz_desktop_lib::devices::{DeviceListing, DeviceManager, Opener};
use open_powerz_desktop_lib::worker::BackendEvent;
use powerz_protocol::transport::{self, discovery::DeviceEntry, discovery::Model};

// ---- AC-X2SPDW.11: shutting down stops every worker within 1 s ----

struct PipeOpener {
    clock: SimClock,
    log: Arc<Mutex<Option<Arc<Mutex<Log>>>>>,
}

impl Opener for PipeOpener {
    type Transport = Pipe;
    fn open(&self, _device: &DeviceListing) -> Result<Pipe, transport::Error> {
        let pipe = Pipe::sim(
            Scenario::Load5v1a,
            FaultPlan::new(),
            &self.clock,
            PipeOpts::default(),
        );
        *self.log.lock().unwrap() = Some(Arc::clone(&pipe.log));
        Ok(pipe)
    }
}

type Slot = Arc<Mutex<Option<Arc<Mutex<Log>>>>>;

fn pipe_manager() -> (DeviceManager<PipeOpener, SimClock>, Slot) {
    let clock = clock();
    let slot: Slot = Arc::new(Mutex::new(None));
    let m = DeviceManager::new(
        || {
            Ok(vec![DeviceEntry {
                model: Model::Km003c,
                pid: 0x0063,
                serial: Some("SERIAL-0001".to_owned()),
                product: Some("POWER-Z KM003C".to_owned()),
                interface_string: Some("APP".to_owned()),
                bus: "1".to_owned(),
                address: 7,
            }])
        },
        PipeOpener {
            clock: clock.clone(),
            log: Arc::clone(&slot),
        },
        clock,
    );
    (m, slot)
}

/// Connect and wait (real time) for the first samples; `slow` makes the sink
/// block 3 s per event once raised.
fn connect_streaming(m: &mut DeviceManager<PipeOpener, SimClock>, slow: Arc<AtomicBool>) {
    let (tx, rx) = mpsc::channel();
    m.connect("demo:load-5v-1a", 1000, move |e: BackendEvent| {
        if slow.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_secs(3));
        }
        if matches!(e, BackendEvent::Batch(_)) {
            let _ = tx.send(());
        }
    })
    .expect("connect");
    rx.recv_timeout(Duration::from_secs(10))
        .expect("no samples arrived");
}

fn assert_stopped_and_released(slot: &Slot) {
    let log = slot.lock().unwrap().clone().expect("pipe never opened");
    let log = log.lock().unwrap();
    assert!(!log.of_type(0x0F).is_empty(), "the queue was not stopped");
    assert!(log.closed, "the interface was not released");
}

#[test]
fn shutdown_stops_the_queue_and_releases_the_interface_within_1_s() {
    let (mut m, slot) = pipe_manager();
    connect_streaming(&mut m, Arc::new(AtomicBool::new(false)));
    let t = Instant::now();
    m.shutdown();
    let took = t.elapsed();
    assert!(took < Duration::from_secs(1), "shutdown took {took:?}");
    assert_stopped_and_released(&slot);
}

#[test]
fn shutdown_is_quick_even_while_the_ui_is_slow() {
    let (mut m, slot) = pipe_manager();
    let slow = Arc::new(AtomicBool::new(false));
    connect_streaming(&mut m, Arc::clone(&slow));
    slow.store(true, Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(100));
    let t = Instant::now();
    m.shutdown();
    let took = t.elapsed();
    slow.store(false, Ordering::SeqCst);
    assert!(took < Duration::from_secs(1), "shutdown took {took:?}");
    assert_stopped_and_released(&slot);
}

#[test]
fn shutdown_without_a_session_does_nothing() {
    let (mut m, slot) = pipe_manager();
    let t = Instant::now();
    m.shutdown();
    assert!(t.elapsed() < Duration::from_secs(1));
    assert!(slot.lock().unwrap().is_none(), "shutdown opened a meter");
}
