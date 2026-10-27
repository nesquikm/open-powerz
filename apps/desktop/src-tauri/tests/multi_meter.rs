//! Several meters at once (FR 1HHT03).
//!
//! Two simulated meters stand in for two real ones listed by the OS: each
//! has its own synthetic serial (the simulator's serial override), its own
//! scenario (5 V / 1 A and 9 V / 2 A, so their streams are told apart by
//! value) and its own USB plug. The manager keeps one session per device id;
//! every per-meter call names the device it is for. Every serial here is
//! synthetic.

mod common;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::*;
use open_powerz_desktop_lib::devices::{AppOpener, DeviceListing, DeviceManager, Opener};
use open_powerz_desktop_lib::recording::RecordingEvent;
use open_powerz_desktop_lib::worker::{BackendEvent, Status};
use powerz_protocol::session::SystemClock;
use powerz_protocol::transport::{self, Transport, discovery::DeviceEntry, discovery::Model};
use powerz_sim::PdScenario;

const SERIAL_A: &str = "SERIAL-MULTI-A";
const SERIAL_B: &str = "SERIAL-MULTI-B";
const METER_A: &str = "usb:1:7";
const METER_B: &str = "usb:1:8";

fn meter(serial: &str, bus: &str, address: u8) -> DeviceEntry {
    DeviceEntry {
        model: Model::Km003c,
        pid: 0x0063,
        serial: Some(serial.to_owned()),
        product: Some("POWER-Z KM003C".to_owned()),
        interface_string: Some("APP".to_owned()),
        bus: bus.to_owned(),
        address,
    }
}

/// The simulator behind one USB plug: once pulled, every request fails.
struct Plugged {
    sim: SimDevice,
    plugged: Arc<AtomicBool>,
}

impl Transport for Plugged {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, transport::Error> {
        if !self.plugged.load(Ordering::SeqCst) {
            return Err(transport::Error::Disconnected);
        }
        self.sim.transact(request, timeout)
    }

    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, transport::Error> {
        if !self.plugged.load(Ordering::SeqCst) {
            return Err(transport::Error::Disconnected);
        }
        self.sim.read(timeout)
    }
}

/// Opens the simulated meter a listing names (by serial): meter A plays
/// 5 V / 1 A, meter B 9 V / 2 A, each reporting its own serial and each
/// behind its own plug.
struct TwoMeterOpener {
    clock: SimClock,
    plug_a: Arc<AtomicBool>,
    plug_b: Arc<AtomicBool>,
    opened: Arc<Mutex<Vec<DeviceListing>>>,
}

impl Opener for TwoMeterOpener {
    type Transport = Plugged;
    fn open(&self, device: &DeviceListing) -> Result<Plugged, transport::Error> {
        self.opened.lock().unwrap().push(device.clone());
        let (scenario, plug, serial) = match device.serial.as_deref() {
            Some(SERIAL_A) => (Scenario::Load5v1a, &self.plug_a, SERIAL_A),
            Some(SERIAL_B) => (Scenario::Pd9v2a, &self.plug_b, SERIAL_B),
            _ => return Err(transport::Error::NotFound),
        };
        if !plug.load(Ordering::SeqCst) {
            return Err(transport::Error::NotFound);
        }
        Ok(Plugged {
            sim: SimDevice::with_clock(Persona::Bulk, scenario, self.clock.clone())
                .with_serial(serial)
                .with_pd_scenario(PdScenario::Negotiation),
            plugged: Arc::clone(plug),
        })
    }
}

/// A manager over two plugs, with the OS device list under the test's control.
struct Rig {
    m: DeviceManager<TwoMeterOpener, SimClock>,
    attached: Arc<Mutex<Vec<DeviceEntry>>>,
    plug_a: Arc<AtomicBool>,
    opened: Arc<Mutex<Vec<DeviceListing>>>,
}

impl Rig {
    fn new() -> Rig {
        let clock = clock();
        let attached = Arc::new(Mutex::new(vec![
            meter(SERIAL_A, "1", 7),
            meter(SERIAL_B, "1", 8),
        ]));
        let plug_a = Arc::new(AtomicBool::new(true));
        let plug_b = Arc::new(AtomicBool::new(true));
        let opened = Arc::new(Mutex::new(Vec::new()));
        let listed = Arc::clone(&attached);
        let m = DeviceManager::new(
            move || Ok(listed.lock().unwrap().clone()),
            TwoMeterOpener {
                clock: clock.clone(),
                plug_a: Arc::clone(&plug_a),
                plug_b,
                opened: Arc::clone(&opened),
            },
            clock,
        );
        Rig {
            m,
            attached,
            plug_a,
            opened,
        }
    }

    /// Pull meter A's cable: its pipe fails at once and the OS stops listing
    /// it; meter B stays.
    fn unplug_a(&mut self) {
        self.plug_a.store(false, Ordering::SeqCst);
        self.attached
            .lock()
            .unwrap()
            .retain(|e| e.serial.as_deref() != Some(SERIAL_A));
    }

    /// Plug meter A back in at a new address and let the watcher report it.
    fn replug_a(&mut self) -> bool {
        self.plug_a.store(true, Ordering::SeqCst);
        self.attached.lock().unwrap().push(meter(SERIAL_A, "1", 9));
        let list = self.m.list_devices();
        self.m.devices_changed(&list)
    }

    fn opens_of(&self, serial: &str) -> usize {
        self.opened
            .lock()
            .unwrap()
            .iter()
            .filter(|d| d.serial.as_deref() == Some(serial))
            .count()
    }
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

/// Everything already queued, without waiting.
fn drain(rx: &mpsc::Receiver<BackendEvent>) -> Vec<BackendEvent> {
    rx.try_iter().collect()
}

fn is_samples(e: &BackendEvent) -> bool {
    matches!(e, BackendEvent::Batch(b) if !b.samples.is_empty())
}

fn is_connected(e: &BackendEvent) -> bool {
    matches!(e, BackendEvent::Status(Status::Connected(_)))
}

fn connected_info(events: &[BackendEvent]) -> (String, u64) {
    events
        .iter()
        .find_map(|e| match e {
            BackendEvent::Status(Status::Connected(i)) => Some((i.serial.clone(), i.session_id)),
            _ => None,
        })
        .expect("never connected")
}

fn mean_vbus(events: &[BackendEvent]) -> f64 {
    let v: Vec<f64> = events
        .iter()
        .filter_map(|e| match e {
            BackendEvent::Batch(b) => Some(b.samples.iter().map(|s| s.vbus_v)),
            _ => None,
        })
        .flatten()
        .collect();
    assert!(!v.is_empty(), "no samples");
    v.iter().sum::<f64>() / v.len() as f64
}

/// Connect meter `id` at 50 SPS and wait for its first samples.
fn stream(rig: &mut Rig, id: &str) -> (mpsc::Receiver<BackendEvent>, Vec<BackendEvent>) {
    let (rx, sink) = channel_sink();
    rig.m
        .connect(id, 50, sink)
        .unwrap_or_else(|e| panic!("connect {id} failed: {}", e.message));
    let first = recv_until(&rx, is_samples);
    (rx, first)
}

/// Both meters streaming: (A's events, B's events).
fn stream_both(rig: &mut Rig) -> (mpsc::Receiver<BackendEvent>, mpsc::Receiver<BackendEvent>) {
    let (rx_a, _) = stream(rig, METER_A);
    let (rx_b, _) = stream(rig, METER_B);
    (rx_a, rx_b)
}

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("powerz-multi-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

// ---- AC-1HHT03.1: a second meter, each with its own session and stream ----

#[test]
fn a_second_meter_connects_while_the_first_streams_each_with_its_own_session() {
    let mut rig = Rig::new();
    let (rx_a, first_a) = stream(&mut rig, METER_A);
    let (rx_b, first_b) = stream(&mut rig, METER_B);

    let (serial_a, session_a) = connected_info(&first_a);
    let (serial_b, session_b) = connected_info(&first_b);
    assert_eq!(serial_a, SERIAL_A);
    assert_eq!(serial_b, SERIAL_B);
    assert_ne!(session_a, session_b, "two meters share one session id");
    assert_eq!(
        rig.m.connected_ids(),
        vec![METER_A.to_owned(), METER_B.to_owned()]
    );
    assert_eq!(rig.opens_of(SERIAL_A), 1);
    assert_eq!(rig.opens_of(SERIAL_B), 1);

    // Each stream carries its own meter's values, never the other's.
    let more_a = recv_until(&rx_a, is_samples);
    let more_b = recv_until(&rx_b, is_samples);
    assert!(
        (mean_vbus(&more_a) - 5.0).abs() < 0.2,
        "A streams {}",
        mean_vbus(&more_a)
    );
    assert!(
        (mean_vbus(&more_b) - 9.0).abs() < 0.2,
        "B streams {}",
        mean_vbus(&more_b)
    );

    // Each meter keeps its own statistics.
    let avg = |id: &str| {
        rig.m
            .stats_for(id)
            .unwrap_or_else(|e| panic!("stats_for {id}: {}", e.message))
            .vbus
            .expect("no VBUS statistics")
            .avg
    };
    assert!(
        (avg(METER_A) - 5.0).abs() < 0.2,
        "A's stats {}",
        avg(METER_A)
    );
    assert!(
        (avg(METER_B) - 9.0).abs() < 0.2,
        "B's stats {}",
        avg(METER_B)
    );

    rig.m.disconnect_for(METER_A).unwrap();
    rig.m.disconnect_for(METER_B).unwrap();
}

#[test]
fn the_same_meter_cannot_be_connected_twice_but_a_third_meter_can_be_added() {
    let mut rig = Rig::new();
    rig.attached
        .lock()
        .unwrap()
        .push(meter("SERIAL-MULTI-C", "1", 10));
    let (_rx_a, _rx_b) = stream_both(&mut rig);

    let (_rx, sink) = channel_sink();
    let err = rig
        .m
        .connect(METER_A, 50, sink)
        .expect_err("meter A connected twice");
    assert_eq!(
        err.kind,
        open_powerz_desktop_lib::ui_error::ErrorKind::Usage
    );
    assert_eq!(rig.opens_of(SERIAL_A), 1, "a second connect opened meter A");

    // A third listed meter can be added too (the opener does not know it, so
    // the session ends at once, but the manager let the connect through).
    let (rx_c, sink_c) = channel_sink();
    rig.m
        .connect("usb:1:10", 50, sink_c)
        .expect("a third meter was refused");
    recv_until(&rx_c, |e| {
        matches!(e, BackendEvent::Status(Status::Disconnected { .. }))
    });
    rig.m.disconnect_for(METER_A).unwrap();
    rig.m.disconnect_for(METER_B).unwrap();
}

/// The demo device can stand in for two meters at once: the second demo
/// session gets a serial of its own, so the two never share a session.
#[test]
fn two_demo_entries_stream_together_with_distinct_serials_and_sessions() {
    let mut m = DeviceManager::new(|| Ok(vec![]), AppOpener, SystemClock);
    let (rx_a, sink_a) = channel_sink();
    let (rx_b, sink_b) = channel_sink();
    m.connect("demo:load-5v-1a", 50, sink_a)
        .expect("first demo connect");
    let first_a = recv_until(&rx_a, is_samples);
    m.connect("demo:pd-9v-2a", 50, sink_b)
        .expect("second demo connect");
    let first_b = recv_until(&rx_b, is_samples);

    let (serial_a, session_a) = connected_info(&first_a);
    let (serial_b, session_b) = connected_info(&first_b);
    assert_eq!(
        serial_a, SIM_SERIAL,
        "the first demo meter keeps its serial"
    );
    assert_ne!(serial_a, serial_b, "two demo meters report one serial");
    assert_ne!(session_a, session_b, "two demo meters share a session");

    let more_a = recv_until(&rx_a, is_samples);
    let more_b = recv_until(&rx_b, is_samples);
    assert!((mean_vbus(&more_a) - 5.0).abs() < 0.2);
    assert!((mean_vbus(&more_b) - 9.0).abs() < 0.2);
    m.disconnect_for("demo:load-5v-1a").unwrap();
    m.disconnect_for("demo:pd-9v-2a").unwrap();
}

// ---- AC-1HHT03.3: actions stay with the meter they name ----

#[test]
fn recording_one_meter_records_only_that_meter() {
    let mut rig = Rig::new();
    let (rx_a, rx_b) = stream_both(&mut rig);
    let dir = temp_dir("rec");
    let path = dir.join("b.csv");
    let _ = std::fs::remove_file(&path);

    rig.m
        .start_recording_for(METER_B, &path)
        .expect("recording meter B");
    recv_until(&rx_b, |e| {
        matches!(e, BackendEvent::Recording(RecordingEvent::Started))
    });
    recv_until(&rx_b, is_samples);
    recv_until(&rx_b, is_samples);
    recv_until(&rx_a, is_samples);
    assert!(
        !drain(&rx_a)
            .iter()
            .any(|e| matches!(e, BackendEvent::Recording(_))),
        "meter A got a recording event"
    );
    assert!(
        rig.m.stop_recording_for(METER_A).is_err(),
        "meter A records too"
    );
    let summary = rig.m.stop_recording_for(METER_B).expect("stop B");
    assert!(summary.rows > 0, "meter B's recording wrote nothing");

    // Every row (VBUS is the third column) is meter B's 9 V, none is
    // meter A's 5 V.
    let text = std::fs::read_to_string(&path).unwrap();
    let rows: Vec<&str> = text.lines().skip(1).filter(|l| !l.is_empty()).collect();
    assert!(!rows.is_empty(), "no data rows in {text}");
    for row in rows {
        let vbus: f64 = row
            .split(',')
            .nth(2)
            .and_then(|f| f.trim().parse().ok())
            .unwrap_or_else(|| panic!("no VBUS in {row}"));
        assert!((vbus - 9.0).abs() < 0.2, "row is not meter B's: {row}");
    }
    let _ = std::fs::remove_dir_all(&dir);
    rig.m.disconnect_for(METER_A).unwrap();
    rig.m.disconnect_for(METER_B).unwrap();
}

#[test]
fn a_rate_change_and_the_pd_capture_go_only_to_the_meter_they_name() {
    let mut rig = Rig::new();
    let (rx_a, rx_b) = stream_both(&mut rig);

    rig.m.set_rate_for(METER_B, 10).expect("rate of meter B");
    recv_until(&rx_b, |e| {
        matches!(e, BackendEvent::Status(Status::Rate { rate_sps: 10, .. }))
    });

    rig.m
        .control_for(METER_A)
        .expect("meter A's control")
        .start_pd()
        .expect("PD capture on meter A");
    recv_until(
        &rx_a,
        |e| matches!(e, BackendEvent::PdState(s) if s.capturing),
    );
    recv_until(&rx_b, is_samples);
    let b = drain(&rx_b);
    assert!(
        !b.iter()
            .any(|e| matches!(e, BackendEvent::PdState(_) | BackendEvent::PdRows(_))),
        "meter B got meter A's PD capture"
    );
    recv_until(&rx_a, is_samples);
    assert!(
        !drain(&rx_a)
            .iter()
            .any(|e| matches!(e, BackendEvent::Status(Status::Rate { rate_sps: 10, .. }))),
        "meter A took meter B's rate"
    );
    rig.m.disconnect_for(METER_A).unwrap();
    rig.m.disconnect_for(METER_B).unwrap();
}

#[test]
fn a_trigger_session_belongs_to_one_meter() {
    use open_powerz_desktop_lib::trigger::TriggerWorker;
    use powerz_sim::{SimCharger, SimPdm};

    let mut rig = Rig::new();
    let (_rx_a, rx_b) = stream_both(&mut rig);
    let pdm = SimPdm::new(SimCharger::Epr140W, SimClock::new(T0));
    let w = TriggerWorker::spawn(pdm.open_port(), SimClock::new(T0)).expect("trigger opens");
    rig.m
        .install_trigger_for(METER_B, w)
        .expect("trigger on meter B");
    assert!(rig.m.trigger_open_for(METER_B));
    assert!(
        !rig.m.trigger_open_for(METER_A),
        "meter A got meter B's trigger"
    );

    // Closing meter A leaves meter B's trigger session open.
    rig.m.disconnect_for(METER_A).unwrap();
    assert!(
        rig.m.trigger_open_for(METER_B),
        "meter A's disconnect closed B's trigger"
    );
    assert!(pdm.pdm_active(), "pdm close was sent for meter A");
    recv_until(&rx_b, is_samples);

    rig.m.disconnect_for(METER_B).unwrap();
    assert!(!rig.m.trigger_open_for(METER_B));
    assert!(!pdm.pdm_active(), "meter B's trigger outlived its session");
}

#[test]
fn a_firmware_update_takes_only_the_meter_it_names() {
    let mut rig = Rig::new();
    let (rx_a, rx_b) = stream_both(&mut rig);

    let lease = rig.m.begin_flash(METER_A).expect("flash meter A");
    recv_until(&rx_a, |e| {
        matches!(e, BackendEvent::Status(Status::Disconnected { .. }))
    });
    // Meter B streams on and still takes requests.
    let _ = drain(&rx_b);
    recv_until(&rx_b, is_samples);
    rig.m
        .set_rate_for(METER_B, 10)
        .expect("meter B refused while meter A flashes");
    assert_eq!(rig.m.connected_ids(), vec![METER_B.to_owned()]);
    drop(lease);
    rig.m.disconnect_for(METER_B).unwrap();
}

// ---- AC-1HHT03.3 / AC-1HHT03.4: unplugging one meter leaves the others ----

#[test]
fn unplugging_one_meter_leaves_the_other_streaming_and_replug_recovers_only_it() {
    let mut rig = Rig::new();
    let (rx_a, first_a) = stream(&mut rig, METER_A);
    let (rx_b, first_b) = stream(&mut rig, METER_B);
    let (_, session_a) = connected_info(&first_a);
    let (_, session_b) = connected_info(&first_b);

    rig.unplug_a();
    let got = recv_until(&rx_a, |e| {
        matches!(e, BackendEvent::Status(Status::Waiting { .. }))
    });
    assert!(
        got.iter().any(|e| matches!(
            e,
            BackendEvent::Status(Status::Waiting { serial }) if serial == SERIAL_A
        )),
        "meter A does not wait for its serial"
    );
    assert_eq!(rig.m.waiting_serials(), vec![SERIAL_A.to_owned()]);

    // Meter B never noticed: no status change, and its samples keep coming.
    let _ = drain(&rx_b);
    let later_b = recv_until(&rx_b, is_samples);
    let later_b = [later_b, recv_until(&rx_b, is_samples)].concat();
    assert!(
        !later_b.iter().any(|e| matches!(e, BackendEvent::Status(_))),
        "meter B's session changed: {later_b:?}"
    );
    assert!((mean_vbus(&later_b) - 9.0).abs() < 0.2);
    assert!(
        rig.m.control_for(METER_B).is_ok(),
        "meter B lost its session"
    );

    // Meter A comes back: only meter A reconnects, on its own session.
    assert!(rig.replug_a(), "the returning meter A was not reconnected");
    let back = recv_until(&rx_a, is_connected);
    let (serial, session) = connected_info(&back);
    assert_eq!(serial, SERIAL_A);
    assert_eq!(session, session_a, "meter A's session did not continue");
    assert_eq!(rig.opens_of(SERIAL_B), 1, "meter B was reopened");
    recv_until(&rx_b, is_samples);
    assert!(
        !drain(&rx_b).iter().any(is_connected),
        "meter B's page saw a reconnect"
    );
    assert!(rig.m.waiting_serials().is_empty());
    assert_ne!(session_a, session_b);
    rig.m.disconnect_for(METER_A).unwrap();
    rig.m.disconnect_for(METER_B).unwrap();
}

#[test]
fn disconnecting_one_meter_leaves_the_other_running_and_keeps_its_data() {
    let mut rig = Rig::new();
    let (rx_a, rx_b) = stream_both(&mut rig);

    rig.m.disconnect_for(METER_A).expect("disconnect meter A");
    recv_until(&rx_a, |e| {
        matches!(e, BackendEvent::Status(Status::Disconnected { .. }))
    });
    assert_eq!(rig.m.connected_ids(), vec![METER_B.to_owned()]);
    assert!(
        rig.m.waiting_serials().is_empty(),
        "a disconnect asked for waits"
    );

    let _ = drain(&rx_b);
    let later_b = recv_until(&rx_b, is_samples);
    assert!((mean_vbus(&later_b) - 9.0).abs() < 0.2);
    // Meter A's data stays readable after its session ended.
    let stats_a = rig.m.stats_for(METER_A).expect("meter A's data is gone");
    assert!(stats_a.samples > 0);
    assert!(
        rig.m.disconnect_for(METER_A).is_err(),
        "meter A disconnected twice"
    );
    rig.m.disconnect_for(METER_B).unwrap();
}

/// The commands without a device id act on a running meter: once the meter
/// connected last is disconnected, they move to the one still streaming
/// instead of the ended session (audit finding).
#[test]
fn after_the_last_meter_leaves_the_plain_commands_act_on_the_one_still_running() {
    let mut rig = Rig::new();
    let (rx_a, _rx_b) = stream_both(&mut rig);
    rig.m.disconnect_for(METER_B).unwrap();

    rig.m
        .set_rate(10)
        .expect("the plain rate change reaches meter A");
    recv_until(&rx_a, |e| {
        matches!(e, BackendEvent::Status(Status::Rate { rate_sps: 10, .. }))
    });
    assert!(rig.m.attacher().is_some(), "a reload must find meter A");
    // The plain stats keep growing: they read A's live store, not B's ended one.
    let before = rig.m.stats().expect("plain stats").samples;
    let deadline = Instant::now() + Duration::from_secs(5);
    while rig.m.stats().expect("plain stats").samples <= before {
        assert!(
            Instant::now() < deadline,
            "plain stats froze at {before} samples"
        );
        drain(&rx_a);
        std::thread::sleep(Duration::from_millis(20));
    }
    rig.m
        .disconnect()
        .expect("the plain disconnect ends meter A");
    recv_until(&rx_a, |e| {
        matches!(e, BackendEvent::Status(Status::Disconnected { .. }))
    });
}

/// With no meter running, the plain commands keep the last session, so a
/// single meter's data stays readable after its disconnect, as before.
#[test]
fn with_no_meter_running_the_plain_commands_keep_the_last_session() {
    let mut rig = Rig::new();
    let (_rx_a, _rx_b) = stream_both(&mut rig);
    rig.m.disconnect_for(METER_A).unwrap();
    rig.m.disconnect_for(METER_B).unwrap();
    assert_eq!(
        rig.m.stats().expect("plain stats after both ended"),
        rig.m.stats_for(METER_B).expect("B's stats"),
    );
    assert!(rig.m.attacher().is_none());
}

// ---- Review findings: waits, reloads and demo serials with several meters ----

/// Adding a meter while another one is unplugged and waited for keeps that
/// wait: the unplugged meter still reconnects when it comes back.
#[test]
fn adding_a_meter_while_another_waits_for_its_replug_keeps_that_wait() {
    let mut rig = Rig::new();
    let (rx_a, first_a) = stream(&mut rig, METER_A);
    let (_rx_b, _) = stream(&mut rig, METER_B);
    let (_, session_a) = connected_info(&first_a);

    rig.unplug_a();
    recv_until(&rx_a, |e| {
        matches!(e, BackendEvent::Status(Status::Waiting { .. }))
    });

    // A third meter is added while meter B streams (the opener does not
    // know it, so its session ends at once; the connect went through).
    let (rx_c, sink_c) = channel_sink();
    rig.m
        .connect("demo:idle", 50, sink_c)
        .expect("adding a meter was refused");
    recv_until(&rx_c, |e| {
        matches!(e, BackendEvent::Status(Status::Disconnected { .. }))
    });
    assert_eq!(
        rig.m.waiting_serials(),
        vec![SERIAL_A.to_owned()],
        "adding a meter cancelled meter A's wait"
    );

    // Meter A comes back and resumes its own session.
    assert!(rig.replug_a(), "the returning meter A was not reconnected");
    let back = recv_until(&rx_a, is_connected);
    assert_eq!(connected_info(&back).1, session_a);
    recv_until(&rx_a, is_samples);
    // A reload names meter A by its new address, as its attach snapshot does.
    let snap = rig
        .m
        .attacher_for(METER_A)
        .expect("meter A runs")
        .attach(|_: BackendEvent| {})
        .expect("meter A's snapshot");
    assert_eq!(
        rig.m.running_ids(),
        vec![snap.device_id.clone(), METER_B.to_owned()]
    );
    assert_eq!(snap.device_id, "usb:1:9");
    rig.m.disconnect_for(METER_A).unwrap();
    rig.m.disconnect_for(METER_B).unwrap();
}

/// A reload hands every running meter to the new page, not only the
/// current one: each keeps streaming to its new sink.
#[test]
fn a_reload_reattaches_every_running_meter() {
    let mut rig = Rig::new();
    let (_rx_a, _rx_b) = stream_both(&mut rig);
    assert_eq!(
        rig.m.connected_ids(),
        vec![METER_A.to_owned(), METER_B.to_owned()]
    );

    let (new_a, sink_a) = channel_sink();
    let (new_b, sink_b) = channel_sink();
    let snap_a = rig
        .m
        .attacher_for(METER_A)
        .expect("meter A runs")
        .attach(sink_a)
        .expect("meter A's snapshot");
    let snap_b = rig
        .m
        .attacher_for(METER_B)
        .expect("meter B runs")
        .attach(sink_b)
        .expect("meter B's snapshot");
    assert_eq!(snap_a.device_id, METER_A);
    assert_eq!(snap_b.device_id, METER_B);
    assert_eq!(snap_a.serial.as_deref(), Some(SERIAL_A));
    assert_eq!(snap_b.serial.as_deref(), Some(SERIAL_B));

    let later_a = recv_until(&new_a, is_samples);
    let later_b = recv_until(&new_b, is_samples);
    assert!((mean_vbus(&later_a) - 5.0).abs() < 0.2);
    assert!((mean_vbus(&later_b) - 9.0).abs() < 0.2);
    // The plain attach still picks the current meter.
    assert!(rig.m.attacher().is_some());
    assert!(rig.m.attacher_for("usb:9:9").is_none());
    rig.m.disconnect_for(METER_A).unwrap();
    rig.m.disconnect_for(METER_B).unwrap();
}

/// A new demo meter never gets the serial (and so the session and data) of
/// another demo meter that ended; a demo meter connected again gets its own.
#[test]
fn a_new_demo_meter_never_takes_over_an_ended_demo_meters_serial() {
    let mut m = DeviceManager::new(|| Ok(vec![]), AppOpener, SystemClock);
    let (rx_load, sink_load) = channel_sink();
    m.connect("demo:load-5v-1a", 50, sink_load)
        .expect("first demo connect");
    recv_until(&rx_load, is_samples);
    let (rx_pd, sink_pd) = channel_sink();
    m.connect("demo:pd-9v-2a", 50, sink_pd)
        .expect("second demo connect");
    let (serial_pd, session_pd) = connected_info(&recv_until(&rx_pd, is_samples));
    m.disconnect_for("demo:pd-9v-2a").unwrap();
    recv_until(&rx_pd, |e| {
        matches!(e, BackendEvent::Status(Status::Disconnected { .. }))
    });

    let (rx_idle, sink_idle) = channel_sink();
    m.connect("demo:idle", 50, sink_idle)
        .expect("third demo connect");
    let (serial_idle, session_idle) = connected_info(&recv_until(&rx_idle, is_samples));
    assert_ne!(
        serial_idle, serial_pd,
        "the new demo took the ended one's serial"
    );
    assert_ne!(
        session_idle, session_pd,
        "the new demo took the ended one's session"
    );
    m.disconnect_for("demo:idle").unwrap();

    let (rx_again, sink_again) = channel_sink();
    m.connect("demo:pd-9v-2a", 50, sink_again)
        .expect("the 9 V demo connects again");
    let (serial_again, session_again) = connected_info(&recv_until(&rx_again, is_samples));
    assert_eq!(serial_again, serial_pd, "the 9 V demo lost its serial");
    assert_eq!(session_again, session_pd);
    m.disconnect_for("demo:pd-9v-2a").unwrap();
    m.disconnect_for("demo:load-5v-1a").unwrap();
}

/// A demo meter that got a serial of its own keeps it when it comes back,
/// even after the first demo meter ended and the shared serial is free.
#[test]
fn a_demo_meter_comes_back_with_its_own_serial_once_the_first_one_ended() {
    let mut m = DeviceManager::new(|| Ok(vec![]), AppOpener, SystemClock);
    let ended = |rx: &mpsc::Receiver<BackendEvent>| {
        recv_until(rx, |e| {
            matches!(e, BackendEvent::Status(Status::Disconnected { .. }))
        });
    };
    let (rx_load, sink_load) = channel_sink();
    m.connect("demo:load-5v-1a", 50, sink_load)
        .expect("first demo connect");
    recv_until(&rx_load, is_samples);
    let (rx_pd, sink_pd) = channel_sink();
    m.connect("demo:pd-9v-2a", 50, sink_pd)
        .expect("second demo connect");
    let (serial_pd, session_pd) = connected_info(&recv_until(&rx_pd, is_samples));
    assert_ne!(serial_pd, SIM_SERIAL);
    m.disconnect_for("demo:load-5v-1a").unwrap();
    ended(&rx_load);
    m.disconnect_for("demo:pd-9v-2a").unwrap();
    ended(&rx_pd);

    let (rx_again, sink_again) = channel_sink();
    m.connect("demo:pd-9v-2a", 50, sink_again)
        .expect("the second demo again");
    let (serial_again, session_again) = connected_info(&recv_until(&rx_again, is_samples));
    assert_eq!(serial_again, serial_pd, "it came back with another serial");
    assert_eq!(session_again, session_pd, "it lost its own session");
    m.disconnect_for("demo:pd-9v-2a").unwrap();
}
