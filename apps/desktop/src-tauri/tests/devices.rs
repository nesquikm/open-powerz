//! Device listing, hotplug and the device manager (FR VHY378).

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::*;
use open_powerz_desktop_lib::devices::{
    DeviceListing, DeviceManager, Hotplug, Opener, list_devices_from,
};
use open_powerz_desktop_lib::ui_error::ErrorKind;
use open_powerz_desktop_lib::worker::{BackendEvent, GapKind, Status};
use powerz_protocol::transport::{self, discovery::DeviceEntry, discovery::Model};

const PLACEHOLDER_SERIAL: &str = "SERIAL-0001";

fn meter(serial: &str, bus: &str, address: u8) -> DeviceEntry {
    DeviceEntry {
        model: Model::Km003c,
        pid: 0x0063,
        serial: Some(serial.to_owned()),
        bus: bus.to_owned(),
        address,
    }
}

fn demo_labels(list: &[DeviceListing]) -> Vec<String> {
    list.iter()
        .filter(|d| d.demo_scenario.is_some())
        .map(|d| d.label.clone())
        .collect()
}

// ---- AC-VHY378.1: listing ----

#[test]
fn list_has_every_meter_plus_one_demo_entry_per_scenario() {
    let list = list_devices_from(Ok(vec![meter(PLACEHOLDER_SERIAL, "1", 7)]));
    let usb: Vec<&DeviceListing> = list.iter().filter(|d| d.demo_scenario.is_none()).collect();
    assert_eq!(usb.len(), 1);
    assert_eq!(usb[0].model, "KM003C");
    assert_eq!(usb[0].serial.as_deref(), Some(PLACEHOLDER_SERIAL));
    assert_eq!(usb[0].bus.as_deref(), Some("1"));
    assert_eq!(usb[0].address, Some(7));
    assert_eq!(
        demo_labels(&list),
        vec![
            "Demo device — idle",
            "Demo device — load-5v-1a",
            "Demo device — pd-9v-2a",
            "Demo device — noisy-negative",
        ]
    );
    let scenarios: Vec<_> = list
        .iter()
        .filter_map(|d| d.demo_scenario.clone())
        .collect();
    assert_eq!(
        scenarios,
        ["idle", "load-5v-1a", "pd-9v-2a", "noisy-negative"]
    );
    // Ids are unique so the UI can pick one.
    let mut ids: Vec<_> = list.iter().map(|d| d.id.clone()).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), list.len());
}

#[test]
fn list_still_has_the_demo_device_when_usb_discovery_fails() {
    let list = list_devices_from(Err(transport::Error::Io("no usb".into())));
    assert_eq!(list.len(), 4);
    assert_eq!(demo_labels(&list).len(), 4);
}

/// Opener over the simulator that counts calls and records the thread.
struct SimOpener {
    clock: SimClock,
    opens: Arc<AtomicUsize>,
    thread: Arc<Mutex<Option<std::thread::ThreadId>>>,
    /// Fault plans for successive opens (then no faults).
    faults: Mutex<Vec<FaultPlan>>,
}

impl SimOpener {
    fn new(clock: &SimClock, faults: Vec<FaultPlan>) -> SimOpener {
        SimOpener {
            clock: clock.clone(),
            opens: Arc::new(AtomicUsize::new(0)),
            thread: Arc::new(Mutex::new(None)),
            faults: Mutex::new(faults),
        }
    }
}

impl Opener for SimOpener {
    type Transport = SimDevice;
    fn open(&self, _device: &DeviceListing) -> Result<SimDevice, transport::Error> {
        self.opens.fetch_add(1, Ordering::SeqCst);
        *self.thread.lock().unwrap() = Some(std::thread::current().id());
        let mut faults = self.faults.lock().unwrap();
        let plan = if faults.is_empty() {
            FaultPlan::new()
        } else {
            faults.remove(0)
        };
        Ok(
            SimDevice::with_clock(Persona::Bulk, Scenario::Load5v1a, self.clock.clone())
                .with_faults(plan),
        )
    }
}

/// A manager over the simulator, its open counter and the thread that opened.
type ManagerParts = (
    DeviceManager<SimOpener, SimClock>,
    Arc<AtomicUsize>,
    Arc<Mutex<Option<std::thread::ThreadId>>>,
);

fn manager(faults: Vec<FaultPlan>) -> ManagerParts {
    let clock = clock();
    let opener = SimOpener::new(&clock, faults);
    let (opens, thread) = (Arc::clone(&opener.opens), Arc::clone(&opener.thread));
    let m = DeviceManager::new(
        || Ok(vec![meter(PLACEHOLDER_SERIAL, "1", 7)]),
        opener,
        clock,
    );
    (m, opens, thread)
}

fn channel_sink() -> (
    mpsc::Receiver<BackendEvent>,
    impl FnMut(BackendEvent) + Send + 'static,
) {
    let (tx, rx) = mpsc::channel();
    (rx, move |e| {
        let _ = tx.send(e);
    })
}

/// Receive events until `want` matches one; returns everything received.
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

#[test]
fn listing_never_opens_a_device() {
    let (m, opens, _) = manager(vec![]);
    let list = m.list_devices();
    assert_eq!(list.len(), 5);
    assert_eq!(opens.load(Ordering::SeqCst), 0, "listing opened a device");
}

// ---- AC-VHY378.2: connect / disconnect through the manager ----

#[test]
fn connect_runs_on_a_worker_thread_and_a_second_connect_is_refused() {
    let (mut m, opens, thread) = manager(vec![]);
    let (rx, sink) = channel_sink();
    m.connect("demo:load-5v-1a", 50, sink)
        .expect("connect failed");
    recv_until(&rx, |e| {
        matches!(e, BackendEvent::Status(Status::Connected(_)))
    });
    assert_eq!(opens.load(Ordering::SeqCst), 1);
    assert_ne!(
        thread.lock().unwrap().unwrap(),
        std::thread::current().id(),
        "opened on the caller's thread"
    );

    let (_rx2, sink2) = channel_sink();
    let err = m
        .connect("demo:idle", 50, sink2)
        .expect_err("second connect accepted");
    assert_eq!(err.kind, ErrorKind::Usage);
    assert_eq!(
        opens.load(Ordering::SeqCst),
        1,
        "second connect opened a device"
    );

    m.disconnect().expect("disconnect failed");
    recv_until(&rx, |e| {
        matches!(e, BackendEvent::Status(Status::Disconnected { .. }))
    });
}

#[test]
fn connect_refuses_an_unsupported_rate() {
    let (mut m, opens, _) = manager(vec![]);
    let (_rx, sink) = channel_sink();
    let err = m
        .connect("demo:idle", 7, sink)
        .expect_err("rate 7 accepted");
    assert_eq!(err.kind, ErrorKind::Usage);
    assert_eq!(opens.load(Ordering::SeqCst), 0);
}

// ---- AC-VHY378.6: hotplug and reconnect ----

#[test]
fn hotplug_reflects_plug_and_unplug_within_2_s() {
    let attached: Arc<Mutex<Vec<DeviceEntry>>> = Arc::new(Mutex::new(Vec::new()));
    let mut hp = Hotplug::new();
    let discover = |a: &Arc<Mutex<Vec<DeviceEntry>>>| {
        let a = Arc::clone(a);
        move || Ok(a.lock().unwrap().clone())
    };
    let has_meter = |l: &[DeviceListing]| {
        l.iter()
            .any(|d| d.serial.as_deref() == Some(PLACEHOLDER_SERIAL))
    };

    let first = hp
        .tick(0, discover(&attached))
        .expect("first tick must list");
    assert!(!has_meter(&first));

    let plug_at = 1_230;
    let unplug_at = 6_170;
    let mut seen_plug = None;
    let mut seen_unplug = None;
    let mut t = 100;
    while t <= 12_000 {
        if t >= plug_at && t < unplug_at && seen_plug.is_none() {
            *attached.lock().unwrap() = vec![meter(PLACEHOLDER_SERIAL, "1", 7)];
        }
        if t >= unplug_at {
            attached.lock().unwrap().clear();
        }
        if let Some(list) = hp.tick(t, discover(&attached)) {
            if has_meter(&list) && seen_plug.is_none() {
                seen_plug = Some(t);
            }
            if !has_meter(&list) && seen_plug.is_some() && t >= unplug_at {
                seen_unplug = Some(t);
            }
        }
        t += 100;
    }
    let plug = seen_plug.expect("plug never reported");
    let unplug = seen_unplug.expect("unplug never reported");
    // The plug became visible at the first tick at or after plug_at (1300).
    assert!(
        plug - 1_300 <= 2_000,
        "plug reported after {} ms",
        plug - 1_300
    );
    assert!(
        unplug - 6_200 <= 2_000,
        "unplug reported after {} ms",
        unplug - 6_200
    );
}

#[test]
fn hotplug_reports_nothing_when_the_list_is_unchanged() {
    let mut hp = Hotplug::new();
    let discover = || Ok(vec![meter(PLACEHOLDER_SERIAL, "1", 7)]);
    assert!(hp.tick(0, discover).is_some());
    for t in (100..=5_000).step_by(100) {
        assert!(
            hp.tick(t, discover).is_none(),
            "unchanged list reported at {t}"
        );
    }
}

#[test]
fn reconnecting_the_same_serial_appends_to_the_same_session_with_an_outage_gap() {
    let (mut m, _opens, _) = manager(vec![FaultPlan::new().disconnect_after_polls(20)]);
    let (rx, sink) = channel_sink();
    m.connect("demo:load-5v-1a", 50, sink).unwrap();
    let first = recv_until(&rx, |e| {
        matches!(e, BackendEvent::Status(Status::Disconnected { .. }))
    });
    let first_id = first
        .iter()
        .find_map(|e| match e {
            BackendEvent::Status(Status::Connected(i)) => Some(i.session_id),
            _ => None,
        })
        .expect("first session never connected");
    let kept: usize = first
        .iter()
        .map(|e| match e {
            BackendEvent::Batch(b) => b.samples.len(),
            _ => 0,
        })
        .sum();
    assert!(kept > 0, "no samples before the unplug");

    let (rx2, sink2) = channel_sink();
    m.connect("demo:load-5v-1a", 50, sink2)
        .expect("reconnect after unplug refused");
    let second = recv_until(
        &rx2,
        |e| matches!(e, BackendEvent::Batch(b) if !b.samples.is_empty()),
    );
    m.disconnect().unwrap();

    let second_id = second
        .iter()
        .find_map(|e| match e {
            BackendEvent::Status(Status::Connected(i)) => Some(i.session_id),
            _ => None,
        })
        .expect("second session never connected");
    assert_eq!(second_id, first_id, "same serial got a new session");
    let outage = second
        .iter()
        .position(|e| matches!(e, BackendEvent::Gap(g) if g.kind == GapKind::Outage))
        .expect("no outage gap on reconnect");
    let first_samples = second
        .iter()
        .position(|e| matches!(e, BackendEvent::Batch(b) if !b.samples.is_empty()))
        .unwrap();
    assert!(outage < first_samples, "outage gap after the new samples");
}
