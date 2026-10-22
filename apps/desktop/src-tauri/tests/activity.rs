//! The latency-critical activity held while a meter session is open (FR
//! X2SPDW, AC-X2SPDW.2). On macOS it keeps App Nap and timer throttling off
//! the worker; the hold count is tracked on every OS. One test in its own
//! binary, since the count is process-wide.

mod common;

use std::sync::mpsc;
use std::time::{Duration, Instant};

use common::*;
use open_powerz_desktop_lib::activity;
use open_powerz_desktop_lib::devices::{DeviceListing, DeviceManager, Opener};
use open_powerz_desktop_lib::worker::{BackendEvent, Status};
use powerz_protocol::transport::{self, discovery::DeviceEntry, discovery::Model};

struct SimOpener {
    clock: SimClock,
    faults: FaultPlan,
}

impl Opener for SimOpener {
    type Transport = SimDevice;
    fn open(&self, _device: &DeviceListing) -> Result<SimDevice, transport::Error> {
        Ok(
            SimDevice::with_clock(Persona::Bulk, Scenario::Load5v1a, self.clock.clone())
                .with_faults(self.faults.clone()),
        )
    }
}

fn manager(faults: FaultPlan) -> DeviceManager<SimOpener, SimClock> {
    let clock = clock();
    DeviceManager::new(
        || {
            Ok(vec![DeviceEntry {
                model: Model::Km003c,
                pid: 0x0063,
                serial: Some("SERIAL-0001".to_owned()),
                bus: "1".to_owned(),
                address: 7,
            }])
        },
        SimOpener {
            clock: clock.clone(),
            faults,
        },
        clock,
    )
}

fn wait_for(rx: &mpsc::Receiver<BackendEvent>, what: &str, hit: impl Fn(&BackendEvent) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(e) if hit(&e) => return,
            Ok(_) => {}
            Err(e) => panic!("{what} never arrived: {e:?}"),
        }
    }
}

/// Poll (real time) until the hold count is `n`.
fn wait_held(n: usize, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while activity::held() != n {
        assert!(
            Instant::now() < deadline,
            "{what}: {} activities held, expected {n}",
            activity::held()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_session_holds_the_activity_while_open_and_releases_it_when_it_ends() {
    assert_eq!(activity::held(), 0, "held before any session");

    // Ended on request.
    let mut m = manager(FaultPlan::new());
    let (tx, rx) = mpsc::channel();
    m.connect("demo:load-5v-1a", 1000, move |e| {
        let _ = tx.send(e);
    })
    .expect("connect");
    wait_for(&rx, "connected", |e| {
        matches!(e, BackendEvent::Status(Status::Connected(_)))
    });
    wait_held(1, "while connected");
    m.disconnect().expect("disconnect");
    wait_held(0, "after disconnect");

    // Ended by itself (the meter was unplugged).
    let mut m = manager(FaultPlan::new().disconnect_after_polls(20));
    let (tx, rx) = mpsc::channel();
    m.connect("demo:load-5v-1a", 1000, move |e| {
        let _ = tx.send(e);
    })
    .expect("connect");
    wait_for(&rx, "connected", |e| {
        matches!(e, BackendEvent::Status(Status::Connected(_)))
    });
    wait_for(&rx, "disconnected", |e| {
        matches!(e, BackendEvent::Status(Status::Disconnected { .. }))
    });
    wait_held(0, "after an unplug");
}
