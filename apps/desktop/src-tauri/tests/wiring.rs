//! Wiring of the device backend to the UI layer (FR VHY378): rate changes on
//! a running session, the hotplug watcher, the app's opener and worker
//! panics.

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::*;
use open_powerz_desktop_lib::devices::{AppOpener, DeviceListing, DeviceManager, Opener};
use open_powerz_desktop_lib::ui_error::ErrorKind;
use open_powerz_desktop_lib::worker::{self, BackendEvent, Status, WorkerConfig};
use powerz_protocol::decode::Rate;
use powerz_protocol::firmware::DeviceMode;
use powerz_protocol::frame;
use powerz_protocol::transport::{self, Transport, discovery::DeviceEntry, discovery::Model};
use powerz_sim::{Persona, Scenario, SimClock, SimDevice};

struct DemoOpener(SimClock);

impl Opener for DemoOpener {
    type Transport = SimDevice;
    fn open(&self, _device: &DeviceListing) -> Result<SimDevice, transport::Error> {
        Ok(SimDevice::with_clock(
            Persona::Bulk,
            Scenario::Load5v1a,
            self.0.clone(),
        ))
    }
}

fn demo_manager() -> DeviceManager<DemoOpener, SimClock> {
    let clock = clock();
    DeviceManager::new(|| Ok(Vec::new()), DemoOpener(clock.clone()), clock)
}

fn events() -> (
    mpsc::Receiver<BackendEvent>,
    impl FnMut(BackendEvent) + Send + 'static,
) {
    let (tx, rx) = mpsc::channel();
    (rx, move |e| {
        let _ = tx.send(e);
    })
}

fn recv_until(
    rx: &mpsc::Receiver<BackendEvent>,
    mut want: impl FnMut(&BackendEvent) -> bool,
) -> Vec<BackendEvent> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut got = Vec::new();
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(e) => {
                let hit = want(&e);
                got.push(e);
                if hit {
                    return got;
                }
            }
            Err(e) => panic!("event never arrived ({e:?}); got {} events", got.len()),
        }
    }
}

// ---- AC-VHY378.3: set_rate reaches the running worker ----

#[test]
fn set_rate_on_the_manager_restarts_the_running_queue() {
    let mut m = demo_manager();
    let (rx, sink) = events();
    m.connect("demo:load-5v-1a", 50, sink).expect("connect");
    recv_until(&rx, |e| {
        matches!(e, BackendEvent::Status(Status::Connected(_)))
    });

    m.set_rate(1000).expect("set_rate refused");
    recv_until(&rx, |e| {
        matches!(
            e,
            BackendEvent::Status(Status::Rate {
                rate_sps: 1000,
                rate_locked: false
            })
        )
    });

    let err = m.set_rate(7).expect_err("rate 7 accepted");
    assert_eq!(err.kind, ErrorKind::Usage);
    m.disconnect().expect("disconnect");
}

#[test]
fn set_rate_without_a_session_is_a_usage_error() {
    let mut m = demo_manager();
    let err = m
        .set_rate(50)
        .expect_err("set_rate accepted with no session");
    assert_eq!(err.kind, ErrorKind::Usage);
}

// ---- AC-VHY378.6: the hotplug watcher drives the device list ----

#[test]
fn watcher_reports_plug_events_to_the_ui() {
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&calls);
    let clock = clock();
    let m = DeviceManager::new(
        move || {
            // Nothing attached at first, then one meter.
            if seen.fetch_add(1, Ordering::SeqCst) == 0 {
                Ok(Vec::new())
            } else {
                Ok(vec![DeviceEntry {
                    model: Model::Km003c,
                    pid: 0x0063,
                    serial: Some("SERIAL-0002".to_owned()),
                    product: Some("POWER-Z KM003C".to_owned()),
                    interface_string: Some("APP".to_owned()),
                    bus: "2".to_owned(),
                    address: 4,
                }])
            }
        },
        DemoOpener(clock.clone()),
        clock,
    );
    let (tx, rx) = mpsc::channel::<Vec<DeviceListing>>();
    let watch = m.watch_devices(move |list| {
        let _ = tx.send(list);
    });
    let first = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("first list");
    let second = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("changed list");
    watch.stop();
    assert_eq!(first.len(), 4, "only the demo entries at first");
    assert_eq!(second.len(), 5, "the plugged meter is listed");
    assert!(
        second
            .iter()
            .any(|d| d.serial.as_deref() == Some("SERIAL-0002"))
    );
}

// ---- AC-VHY378.2: the app's opener ----

#[test]
fn app_opener_opens_the_demo_device_for_a_demo_entry() {
    let listing = open_powerz_desktop_lib::devices::list_devices_from(Ok(Vec::new()))
        .into_iter()
        .find(|d| d.id == "demo:pd-9v-2a")
        .expect("demo entry");
    let mut t = AppOpener.open(&listing).expect("demo open failed");
    let reply = t
        .transact(&frame::connect(1), Duration::from_secs(1))
        .expect("CONNECT over the demo device failed");
    assert!(!reply.is_empty());
}

#[test]
fn app_opener_reports_a_vanished_meter_as_not_found() {
    let gone = DeviceListing {
        id: "usb:99:250".to_owned(),
        label: "KM003C — gone".to_owned(),
        model: "KM003C".to_owned(),
        serial: Some("NOPE".to_owned()),
        bus: Some("99".to_owned()),
        address: Some(250),
        demo_scenario: None,
        mode: DeviceMode::Application,
    };
    match AppOpener.open(&gone) {
        Err(transport::Error::NotFound) => {}
        Err(other) => panic!("expected NotFound, got {other:?}"),
        Ok(_) => panic!("opened a meter that is not attached"),
    }
}

// ---- AC-VHY378.7: a worker panic reaches the UI as a plain error ----

struct PanickingPipe;

impl Transport for PanickingPipe {
    fn transact(
        &mut self,
        _request: &[u8],
        _timeout: Duration,
    ) -> Result<Vec<u8>, transport::Error> {
        panic!("boom: internal detail");
    }
    fn read(&mut self, _timeout: Duration) -> Result<Vec<u8>, transport::Error> {
        panic!("boom: internal detail");
    }
}

#[test]
fn a_worker_panic_ends_the_session_with_a_plain_internal_error() {
    let (tx, rx) = mpsc::channel();
    let got: Arc<Mutex<Vec<BackendEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let keep = Arc::clone(&got);
    let handle = worker::spawn(
        || Ok(PanickingPipe),
        clock(),
        WorkerConfig::new(Rate::Sps50, 1),
        move |e: BackendEvent| {
            keep.lock().unwrap().push(e.clone());
            let _ = tx.send(e);
        },
    );
    let events = recv_until(&rx, |e| {
        matches!(e, BackendEvent::Status(Status::Disconnected { .. }))
    });
    handle.disconnect();
    let Some(BackendEvent::Status(Status::Disconnected { error: Some(error) })) = events.last()
    else {
        panic!("no disconnected status with an error: {events:?}");
    };
    assert_eq!(error.kind, ErrorKind::Internal);
    assert!(
        !error.message.contains("boom"),
        "panic text leaked: {error:?}"
    );
}

// ---- Review round 1: lifecycle ----

fn demo_spawn(
    stop: Option<Arc<std::sync::atomic::AtomicBool>>,
) -> (mpsc::Receiver<BackendEvent>, worker::WorkerHandle) {
    let clock = clock();
    let (rx, sink) = events();
    let open = {
        let clock = clock.clone();
        move || Ok(SimDevice::with_clock(Persona::Bulk, Scenario::Idle, clock))
    };
    let config = WorkerConfig::new(Rate::Sps50, 1);
    let handle = match stop {
        Some(stop) => worker::spawn_with_stop(open, clock, config, sink, stop),
        None => worker::spawn(open, clock, config, sink),
    };
    (rx, handle)
}

#[test]
fn dropping_the_worker_handle_stops_the_worker() {
    let (rx, handle) = demo_spawn(None);
    recv_until(&rx, |e| {
        matches!(e, BackendEvent::Status(Status::Connected(_)))
    });
    drop(handle);
    recv_until(&rx, |e| {
        matches!(
            e,
            BackendEvent::Status(Status::Disconnected { error: None })
        )
    });
}

#[test]
fn an_external_stop_flag_stops_the_worker() {
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (rx, _handle) = demo_spawn(Some(Arc::clone(&stop)));
    recv_until(&rx, |e| {
        matches!(e, BackendEvent::Status(Status::Connected(_)))
    });
    stop.store(true, Ordering::SeqCst);
    recv_until(&rx, |e| {
        matches!(
            e,
            BackendEvent::Status(Status::Disconnected { error: None })
        )
    });
}

#[test]
fn a_session_can_be_taken_out_and_closed_outside_the_manager() {
    let mut m = demo_manager();
    let (rx, sink) = events();
    m.connect("demo:idle", 50, sink).expect("connect");
    recv_until(&rx, |e| {
        matches!(e, BackendEvent::Status(Status::Connected(_)))
    });

    let (control, rate) = m.rate_control(10).expect("rate control");
    assert_eq!(rate, Rate::Sps10);
    control
        .set_rate(rate)
        .expect("set_rate outside the manager");

    let handle = m.take_session().expect("take_session");
    assert_eq!(
        m.take_session().expect_err("second take").kind,
        ErrorKind::Usage,
        "the session stayed in the manager"
    );
    handle.disconnect();
    recv_until(&rx, |e| {
        matches!(e, BackendEvent::Status(Status::Disconnected { .. }))
    });
}
