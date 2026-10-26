//! Replug reconnects to the meter (FR TGE839).
//!
//! The simulator stands in for a real meter listed by the OS: a shared
//! "attached" list drives discovery, and a plug flag makes the open pipe fail
//! with `Disconnected` the moment the meter is pulled. The hotplug watcher
//! hands every changed device list to `DeviceManager::devices_changed`, which
//! is what these tests call directly. Every serial here is synthetic.

mod common;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::*;
use open_powerz_desktop_lib::devices::{DeviceListing, DeviceManager, Opener};
use open_powerz_desktop_lib::recording::RecordingEvent;
use open_powerz_desktop_lib::worker::{BackendEvent, GapKind, Status};
use powerz_protocol::transport::{self, Transport, discovery::DeviceEntry, discovery::Model};

const SERIAL_A: &str = "SERIAL-REPLUG-A";
const SERIAL_B: &str = "SERIAL-REPLUG-B";
const METER_A: &str = "usb:1:7";

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

/// The simulator behind a USB plug: once pulled, every request fails.
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

/// Opens a fresh simulator for every listing while the meter is plugged in,
/// and remembers what it was asked to open.
struct PlugOpener {
    clock: SimClock,
    plugged: Arc<AtomicBool>,
    opened: Arc<Mutex<Vec<DeviceListing>>>,
}

impl Opener for PlugOpener {
    type Transport = Plugged;
    fn open(&self, device: &DeviceListing) -> Result<Plugged, transport::Error> {
        self.opened.lock().unwrap().push(device.clone());
        if device.demo_scenario.is_none() && !self.plugged.load(Ordering::SeqCst) {
            return Err(transport::Error::NotFound);
        }
        // A demo entry is always there; it never shares the meter's plug.
        let plugged = if device.demo_scenario.is_some() {
            Arc::new(AtomicBool::new(true))
        } else {
            Arc::clone(&self.plugged)
        };
        Ok(Plugged {
            sim: SimDevice::with_clock(Persona::Bulk, Scenario::Load5v1a, self.clock.clone()),
            plugged,
        })
    }
}

/// A manager over one plug, with the OS device list under the test's control.
struct Rig {
    m: DeviceManager<PlugOpener, SimClock>,
    clock: SimClock,
    attached: Arc<Mutex<Vec<DeviceEntry>>>,
    plugged: Arc<AtomicBool>,
    opened: Arc<Mutex<Vec<DeviceListing>>>,
}

impl Rig {
    fn new() -> Rig {
        let clock = clock();
        let attached = Arc::new(Mutex::new(vec![meter(SERIAL_A, "1", 7)]));
        let plugged = Arc::new(AtomicBool::new(true));
        let opened = Arc::new(Mutex::new(Vec::new()));
        let listed = Arc::clone(&attached);
        let m = DeviceManager::new(
            move || Ok(listed.lock().unwrap().clone()),
            PlugOpener {
                clock: clock.clone(),
                plugged: Arc::clone(&plugged),
                opened: Arc::clone(&opened),
            },
            clock.clone(),
        );
        Rig {
            m,
            clock,
            attached,
            plugged,
            opened,
        }
    }

    /// Pull the cable: the pipe fails at once, the OS stops listing the
    /// meter, and the watcher reports the shorter list.
    fn unplug(&mut self) {
        self.plugged.store(false, Ordering::SeqCst);
        self.attached.lock().unwrap().clear();
    }

    /// What the hotplug watcher would hand the manager now.
    fn relist(&mut self) -> bool {
        let list = self.m.list_devices();
        self.m.devices_changed(&list)
    }

    /// Plug `entries` in and let the watcher report them.
    fn plug(&mut self, entries: Vec<DeviceEntry>) -> bool {
        self.plugged.store(true, Ordering::SeqCst);
        *self.attached.lock().unwrap() = entries;
        self.relist()
    }

    fn opens(&self) -> Vec<DeviceListing> {
        self.opened.lock().unwrap().clone()
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
            Err(e) => panic!("event never arrived ({e:?}); got {got:?}"),
        }
    }
}

/// Events that arrive within `wait` (none expected to matter).
fn collect_for(rx: &mpsc::Receiver<BackendEvent>, wait: Duration) -> Vec<BackendEvent> {
    let deadline = Instant::now() + wait;
    let mut got = Vec::new();
    while let Ok(e) = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        got.push(e);
    }
    got
}

fn sample_times(events: &[BackendEvent]) -> Vec<u32> {
    events
        .iter()
        .filter_map(|e| match e {
            BackendEvent::Batch(b) => Some(b.samples.iter().map(|s| s.t_ms)),
            _ => None,
        })
        .flatten()
        .collect()
}

fn is_samples(e: &BackendEvent) -> bool {
    matches!(e, BackendEvent::Batch(b) if !b.samples.is_empty())
}

fn session_id(events: &[BackendEvent]) -> Option<u64> {
    events.iter().find_map(|e| match e {
        BackendEvent::Status(Status::Connected(i)) => Some(i.session_id),
        _ => None,
    })
}

/// Connect to meter A at `rate_sps` and wait for its first samples.
fn stream_meter_a(
    rig: &mut Rig,
    rate_sps: u32,
) -> (mpsc::Receiver<BackendEvent>, Vec<BackendEvent>) {
    let (rx, sink) = channel_sink();
    rig.m
        .connect(METER_A, rate_sps, sink)
        .expect("connect to the meter failed");
    let first = recv_until(&rx, is_samples);
    (rx, first)
}

/// Unplug and wait until the session has said it is waiting for the meter.
fn unplug_and_wait(rig: &mut Rig, rx: &mpsc::Receiver<BackendEvent>) -> Vec<BackendEvent> {
    rig.unplug();
    let got = recv_until(rx, |e| {
        matches!(e, BackendEvent::Status(Status::Waiting { .. }))
    });
    assert!(!rig.relist(), "a list without the meter started a connect");
    got
}

// ---- AC-TGE839.1: the unplugged meter stays the one the app waits for ----

#[test]
fn an_unplug_leaves_the_session_waiting_for_that_meter_and_opens_no_demo() {
    let mut rig = Rig::new();
    let (rx, _) = stream_meter_a(&mut rig, 50);
    let got = unplug_and_wait(&mut rig, &rx);

    let disconnected = got
        .iter()
        .position(|e| matches!(e, BackendEvent::Status(Status::Disconnected { .. })))
        .expect("the unplug never ended the session");
    let waiting = got
        .iter()
        .position(|e| matches!(e, BackendEvent::Status(Status::Waiting { .. })))
        .unwrap();
    assert!(
        disconnected < waiting,
        "waiting reported before the session ended"
    );
    match &got[waiting] {
        BackendEvent::Status(Status::Waiting { serial }) => assert_eq!(serial, SERIAL_A),
        other => panic!("not a waiting status: {other:?}"),
    }
    assert_eq!(rig.m.waiting_for().as_deref(), Some(SERIAL_A));

    // More lists with only the demo entries: still nothing is opened.
    rig.clock.advance(Duration::from_secs(5));
    assert!(!rig.relist(), "the demo-only list started a connect");
    assert!(
        collect_for(&rx, Duration::from_millis(300))
            .iter()
            .all(|e| !matches!(e, BackendEvent::Status(Status::Connected(_))))
    );
    let opens = rig.opens();
    assert_eq!(
        opens.len(),
        1,
        "something was opened after the unplug: {opens:?}"
    );
    assert!(
        opens.iter().all(|d| d.demo_scenario.is_none()),
        "a demo device was opened on its own: {opens:?}"
    );
    assert_eq!(rig.m.waiting_for().as_deref(), Some(SERIAL_A));
}

#[test]
fn a_disconnect_the_user_asked_for_waits_for_nothing() {
    let mut rig = Rig::new();
    let (rx, _) = stream_meter_a(&mut rig, 50);
    rig.m.disconnect().expect("disconnect failed");
    let got = recv_until(&rx, |e| {
        matches!(e, BackendEvent::Status(Status::Disconnected { .. }))
    });
    assert!(
        !got.iter()
            .any(|e| matches!(e, BackendEvent::Status(Status::Waiting { .. }))),
        "a requested disconnect waits for the meter"
    );
    assert_eq!(rig.m.waiting_for(), None);
    rig.unplug();
    rig.relist();
    assert!(
        !rig.plug(vec![meter(SERIAL_A, "1", 8)]),
        "reconnected after a user disconnect"
    );
    assert_eq!(rig.opens().len(), 1);
}

// ---- AC-TGE839.2: the same serial comes back and the session reconnects ----

#[test]
fn the_same_serial_reconnects_within_3_s_at_the_rate_selected_before() {
    let mut rig = Rig::new();
    let (rx, before) = stream_meter_a(&mut rig, 50);
    let first_id = session_id(&before).expect("never connected");
    // The user picked 10 SPS during the session.
    rig.m.set_rate(10).expect("set_rate failed");
    recv_until(&rx, |e| {
        matches!(e, BackendEvent::Status(Status::Rate { rate_sps: 10, .. }))
    });
    unplug_and_wait(&mut rig, &rx);
    rig.clock.advance(Duration::from_secs(4));

    // The OS lists the meter again, at a new address as after a real replug.
    let replugged = Instant::now();
    assert!(
        rig.plug(vec![meter(SERIAL_A, "1", 9)]),
        "the returning meter was not reconnected"
    );
    let after = recv_until(&rx, |e| {
        matches!(e, BackendEvent::Status(Status::Connected(_)))
    });
    let took = replugged.elapsed();
    assert!(took <= Duration::from_secs(3), "reconnect took {took:?}");
    let reopened = rig.opens().last().cloned().expect("nothing reopened");
    assert_eq!(reopened.serial.as_deref(), Some(SERIAL_A));
    assert_eq!(
        reopened.id, "usb:1:9",
        "the meter's new listing was not used"
    );
    assert_eq!(
        session_id(&after),
        Some(first_id),
        "the reconnect started a new session"
    );
    assert_eq!(
        rig.m.waiting_for(),
        None,
        "still waiting after the reconnect"
    );

    // Streaming resumes at 10 SPS, not at the 50 SPS of the first connect.
    let mut events = after;
    while sample_times(&events).len() < 6 {
        events.extend(recv_until(&rx, is_samples));
    }
    let rates: Vec<u32> = events
        .iter()
        .filter_map(|e| match e {
            BackendEvent::Status(Status::Connected(i)) => i.rate_sps,
            BackendEvent::Status(Status::Rate { rate_sps, .. }) => Some(*rate_sps),
            _ => None,
        })
        .collect();
    assert!(!rates.is_empty(), "no rate reported after the reconnect");
    assert!(
        rates.iter().all(|&r| r == 10),
        "rates after the reconnect: {rates:?}"
    );
    let t = sample_times(&events);
    for w in t.windows(2) {
        let step = w[1].saturating_sub(w[0]);
        assert!(
            (50..=150).contains(&step),
            "samples {step} ms apart after the reconnect, not 10 SPS: {t:?}"
        );
    }
    rig.m.disconnect().unwrap();
}

// ---- AC-TGE839.3: the same timeline, with a No data gap over the unplug ----

#[test]
fn the_reconnect_continues_the_timeline_with_an_outage_gap_in_history_and_export() {
    let mut rig = Rig::new();
    let (rx, mut before) = stream_meter_a(&mut rig, 50);
    before.extend(unplug_and_wait(&mut rig, &rx));
    let kept = sample_times(&before);
    let last_before = u64::from(*kept.last().expect("no samples before the unplug"));
    rig.clock.advance(Duration::from_secs(5));

    assert!(rig.plug(vec![meter(SERIAL_A, "1", 9)]), "no reconnect");
    let after = recv_until(&rx, is_samples);
    let first_after = u64::from(sample_times(&after)[0]);

    let gap = after
        .iter()
        .find_map(|e| match e {
            BackendEvent::Gap(g) if g.kind == GapKind::Outage => Some(g.clone()),
            _ => None,
        })
        .expect("no outage gap after the reconnect");
    let gap_at = after
        .iter()
        .position(|e| matches!(e, BackendEvent::Gap(_)))
        .unwrap();
    let samples_at = after.iter().position(is_samples).unwrap();
    assert!(gap_at < samples_at, "the gap came after the new samples");
    assert_eq!(
        gap.from_ms, last_before,
        "the gap does not start at the last sample"
    );
    assert_eq!(
        gap.to_ms, first_after,
        "the gap does not end at the first new sample"
    );
    assert!(
        gap.to_ms - gap.from_ms >= 4_000,
        "a 5 s unplug became a {} ms gap",
        gap.to_ms - gap.from_ms
    );

    // Whole session: the history spans both runs and reports the same gap.
    let gaps = rig.m.history_gaps(0, u64::MAX).expect("history_gaps");
    assert!(
        gaps.iter()
            .any(|g| g.from_ms == gap.from_ms && g.to_ms == gap.to_ms),
        "the history has no gap over the unplug: {gaps:?}"
    );

    // The CSV export has rows on both sides and none inside the gap.
    let dir = std::env::temp_dir().join(format!("powerz-replug-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("replug.csv");
    rig.m
        .export_session(&path, |_| {})
        .expect("export refused")
        .join()
        .unwrap()
        .expect("export failed");
    rig.m.disconnect().unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    let elapsed: Vec<u64> = text.lines().skip(1).map(elapsed_ms).collect();
    let origin = u64::from(kept[0]);
    let (from, to) = (gap.from_ms - origin, gap.to_ms - origin);
    assert!(
        elapsed.iter().any(|&e| e <= from),
        "no exported rows before the gap"
    );
    assert!(
        elapsed.iter().any(|&e| e >= to),
        "no exported rows after the gap"
    );
    assert!(
        elapsed.iter().all(|&e| e <= from || e >= to),
        "exported rows inside the gap {from}..{to}"
    );
    assert!(
        elapsed.windows(2).any(|w| w[0] == from && w[1] == to),
        "the export does not jump across the gap {from}..{to}"
    );
}

/// `h:mm:ss.mmm` of a CSV row's first column, in ms.
fn elapsed_ms(row: &str) -> u64 {
    let cell = row.split(',').next().unwrap();
    let (hms, ms) = cell.split_once('.').unwrap();
    let parts: Vec<u64> = hms.split(':').map(|p| p.parse().unwrap()).collect();
    ((parts[0] * 60 + parts[1]) * 60 + parts[2]) * 1000 + ms.parse::<u64>().unwrap()
}

// ---- AC-TGE839.4: another serial is listed, not connected ----

#[test]
fn a_meter_with_another_serial_is_not_connected_while_waiting() {
    let mut rig = Rig::new();
    let (rx, _) = stream_meter_a(&mut rig, 50);
    unplug_and_wait(&mut rig, &rx);

    assert!(
        !rig.plug(vec![meter(SERIAL_B, "1", 8)]),
        "a different meter was connected automatically"
    );
    assert!(
        rig.m
            .list_devices()
            .iter()
            .any(|d| d.serial.as_deref() == Some(SERIAL_B))
    );
    assert_eq!(rig.opens().len(), 1, "the other meter was opened");
    assert!(
        collect_for(&rx, Duration::from_millis(300))
            .iter()
            .all(|e| !matches!(e, BackendEvent::Status(Status::Connected(_))))
    );
    assert_eq!(rig.m.waiting_for().as_deref(), Some(SERIAL_A));

    // Meter A joins it: A is the one that reconnects.
    assert!(rig.plug(vec![meter(SERIAL_B, "1", 8), meter(SERIAL_A, "1", 9)]));
    recv_until(&rx, |e| {
        matches!(e, BackendEvent::Status(Status::Connected(_)))
    });
    let reopened = rig.opens().last().cloned().unwrap();
    assert_eq!(reopened.serial.as_deref(), Some(SERIAL_A));
    rig.m.disconnect().unwrap();
}

// ---- AC-TGE839.5: a demo chosen by hand cancels the wait ----

#[test]
fn connecting_a_demo_by_hand_while_waiting_works_and_cancels_the_wait() {
    let mut rig = Rig::new();
    let (rx, _) = stream_meter_a(&mut rig, 50);
    unplug_and_wait(&mut rig, &rx);

    let (demo_rx, demo_sink) = channel_sink();
    rig.m
        .connect("demo:idle", 50, demo_sink)
        .expect("the demo connect was refused while waiting");
    recv_until(&demo_rx, is_samples);
    assert_eq!(rig.m.waiting_for(), None, "the wait for the meter goes on");

    // The meter comes back: the demo session stays, nothing reconnects.
    assert!(
        !rig.plug(vec![meter(SERIAL_A, "1", 9)]),
        "the meter took over the demo"
    );
    let opens = rig.opens();
    assert_eq!(
        opens
            .last()
            .and_then(|d| d.demo_scenario.clone())
            .as_deref(),
        Some("idle")
    );
    assert!(
        collect_for(&rx, Duration::from_millis(300))
            .iter()
            .all(|e| !matches!(e, BackendEvent::Status(Status::Connected(_))))
    );
    recv_until(&demo_rx, is_samples);
    rig.m.disconnect().unwrap();
}

#[test]
fn the_watcher_keeps_reporting_while_waiting_so_a_missed_replug_is_retried() {
    let mut rig = Rig::new();
    let (rx, _) = stream_meter_a(&mut rig, 50);
    unplug_and_wait(&mut rig, &rx);

    // The list does not change, yet every poll reports it while the app
    // waits: a replug the manager missed, or a reconnect that failed to
    // open, gets another try on the next poll.
    let (lists_tx, lists) = mpsc::channel();
    let watch = rig.m.watch_devices(move |list| {
        let _ = lists_tx.send(list);
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut reports = 0;
    while reports < 3 && Instant::now() < deadline {
        if lists.recv_timeout(Duration::from_millis(100)).is_ok() {
            reports += 1;
        }
    }
    drop(watch);
    assert!(reports >= 3, "only {reports} list report(s) while waiting");
}

#[test]
fn a_reconnect_leaves_the_page_stop_flag_down() {
    let mut rig = Rig::new();
    let (rx, sink) = channel_sink();
    let stop = Arc::new(AtomicBool::new(false));
    rig.m
        .connect_with_stop(METER_A, 50, sink, Arc::clone(&stop))
        .expect("connect to the meter failed");
    recv_until(&rx, is_samples);
    unplug_and_wait(&mut rig, &rx);
    assert!(rig.plug(vec![meter(SERIAL_A, "1", 9)]), "no reconnect");
    recv_until(&rx, is_samples);
    assert!(
        !stop.load(Ordering::SeqCst),
        "the reconnect raised the stop flag the page holds"
    );
    rig.m.disconnect().unwrap();
}

#[test]
fn picking_another_device_cancels_the_wait_before_any_connect() {
    let mut rig = Rig::new();
    let (rx, _) = stream_meter_a(&mut rig, 50);
    unplug_and_wait(&mut rig, &rx);

    // The user picks a demo in the picker but has not pressed Connect yet.
    rig.m.cancel_wait();
    assert_eq!(rig.m.waiting_for(), None);

    // The meter comes back meanwhile: nothing reconnects behind the picker.
    assert!(
        !rig.plug(vec![meter(SERIAL_A, "1", 9)]),
        "the meter reconnected after the wait was cancelled"
    );
    assert!(
        collect_for(&rx, Duration::from_millis(300))
            .iter()
            .all(|e| !matches!(e, BackendEvent::Status(Status::Connected(_))))
    );
}

// ---- AC-V9SK52.5: a meter that comes back in bootloader mode ----

/// Meter `serial` listed in bootloader mode (IF0 interface string `BOOT`).
fn boot_meter(serial: &str, bus: &str, address: u8) -> DeviceEntry {
    DeviceEntry {
        interface_string: Some("BOOT".to_owned()),
        ..meter(serial, bus, address)
    }
}

#[test]
fn the_same_serial_back_in_bootloader_mode_is_not_reconnected_and_is_listed_as_such() {
    let mut rig = Rig::new();
    let (rx, _) = stream_meter_a(&mut rig, 50);
    unplug_and_wait(&mut rig, &rx);

    // The waited-for meter re-enumerates with its serial, but in bootloader
    // mode: the bootloader answers no measurement request, so no session
    // may be opened to it.
    assert!(
        !rig.plug(vec![boot_meter(SERIAL_A, "1", 9)]),
        "a bootloader-mode meter was reconnected automatically"
    );
    let opens = rig.opens();
    assert_eq!(
        opens.len(),
        1,
        "the bootloader-mode meter was opened (frames sent): {opens:?}"
    );
    assert!(
        collect_for(&rx, Duration::from_millis(300))
            .iter()
            .all(|e| !matches!(e, BackendEvent::Status(Status::Connected(_)))),
        "a connected status was sent for a bootloader-mode meter"
    );
    // Still waiting for the meter, which is listed as present in
    // bootloader mode.
    assert_eq!(rig.m.waiting_for().as_deref(), Some(SERIAL_A));
    let listed = rig
        .m
        .list_devices()
        .into_iter()
        .find(|d| d.serial.as_deref() == Some(SERIAL_A))
        .expect("the bootloader-mode meter is listed");
    let v = serde_json::to_value(&listed).unwrap();
    assert_eq!(v["mode"], "bootloader", "{v}");

    // Every later watcher report of the same list still opens nothing.
    rig.clock.advance(Duration::from_secs(2));
    assert!(!rig.relist(), "a later report reconnected the bootloader");
    assert_eq!(rig.opens().len(), 1);

    // Back in application mode: the meter reconnects as before.
    assert!(
        rig.plug(vec![meter(SERIAL_A, "1", 10)]),
        "the meter back in application mode was not reconnected"
    );
    recv_until(&rx, |e| {
        matches!(e, BackendEvent::Status(Status::Connected(_)))
    });
    let reopened = rig.opens().last().cloned().expect("nothing reopened");
    assert_eq!(reopened.serial.as_deref(), Some(SERIAL_A));
    assert_eq!(reopened.id, "usb:1:10");
    assert_eq!(rig.m.waiting_for(), None);
    rig.m.disconnect().unwrap();
}

// ---- AC-TGE839.6: a running recording ends at the unplug ----

#[test]
fn a_recording_ends_at_the_unplug_with_a_complete_file_and_does_not_resume() {
    let mut rig = Rig::new();
    let (rx, _) = stream_meter_a(&mut rig, 50);
    let dir = std::env::temp_dir().join(format!("powerz-replug-rec-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("rec.csv");
    rig.m.start_recording(&path).expect("recording refused");
    // AC-2BF5G6.6: count the batches only once the recording has started, so rows are
    // written before the unplug (on a slow runner they could all arrive before the start).
    let mut got = recv_until(&rx, |e| {
        matches!(e, BackendEvent::Recording(RecordingEvent::Started))
    });
    got.extend(recv_until(&rx, is_samples));
    got.extend(recv_until(&rx, is_samples));

    rig.unplug();
    let mut last_sample = Instant::now();
    let mut summary = None;
    let mut ended = None;
    let deadline = Instant::now() + Duration::from_secs(10);
    while summary.is_none() {
        let e = rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("the recording never ended");
        match &e {
            BackendEvent::Batch(b) if !b.samples.is_empty() => last_sample = Instant::now(),
            BackendEvent::Recording(RecordingEvent::Stopped(s)) => {
                summary = Some(s.clone());
                ended = Some(Instant::now());
            }
            _ => {}
        }
    }
    let summary = summary.unwrap();
    let took = ended.unwrap() - last_sample;
    assert!(
        took <= Duration::from_secs(2),
        "the recording ended {took:?} after the last sample"
    );
    assert!(
        summary.disconnected,
        "the notice does not say the meter went away: {summary:?}"
    );

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.ends_with('\n'), "the file ends mid-row");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines.len() as u64,
        summary.rows + 1,
        "rows lost at the unplug"
    );
    assert!(summary.rows > 0);
    let columns = lines[0].split(',').count();
    assert!(
        lines.iter().all(|l| l.split(',').count() == columns),
        "a row is cut short"
    );

    recv_until(&rx, |e| {
        matches!(e, BackendEvent::Status(Status::Waiting { .. }))
    });
    assert!(rig.plug(vec![meter(SERIAL_A, "1", 9)]), "no reconnect");
    let after = recv_until(&rx, is_samples);
    let more = recv_until(&rx, is_samples);
    assert!(
        !after.iter().chain(more.iter()).any(|e| matches!(
            e,
            BackendEvent::Recording(RecordingEvent::Started)
                | BackendEvent::Recording(RecordingEvent::Progress { .. })
        )),
        "the recording resumed after the reconnect"
    );
    assert!(
        rig.m.stop_recording().is_err(),
        "a recording runs after the reconnect"
    );
    let again = std::fs::read_to_string(&path).unwrap();
    assert_eq!(again, text, "the closed file grew after the reconnect");
    let _ = std::fs::remove_dir_all(&dir);
    rig.m.disconnect().unwrap();
}

// ---- AC-DGSQ6S.5: a meter that drops out also ends the trigger session ----

/// Unplugging while a trigger session holds the charger above 5 V must not
/// leave that session open and invisible: the backend returns to 5 V and
/// closes it, as the UI already assumes when the meter disconnects.
#[test]
fn an_unplug_while_triggering_returns_to_5v_and_closes_the_trigger_session() {
    use open_powerz_desktop_lib::trigger::{TriggerProtocol, TriggerRequest, TriggerWorker};
    use powerz_sim::{SimCharger, SimPdm};

    let mut rig = Rig::new();
    let (rx, _) = stream_meter_a(&mut rig, 50);
    let pdm = SimPdm::new(SimCharger::Epr140W, SimClock::new(1_790_000_000_000));
    let w = TriggerWorker::spawn(pdm.open_port(), SimClock::new(1_790_000_000_000))
        .expect("the trigger session opens");
    rig.m.install_trigger(w).expect("install");
    let c = rig.m.trigger_control().expect("a trigger session is open");
    c.list(TriggerProtocol::Pd).expect("list pd");
    c.request(
        TriggerRequest::Pd {
            position: 2,
            voltage_mv: None,
            current_ma: None,
        },
        true,
    )
    .expect("9 V");
    assert!(pdm.vbus_mv() > 8000, "the charger is at 9 V");

    unplug_and_wait(&mut rig, &rx);

    assert!(!rig.m.trigger_open(), "the trigger session is still open");
    assert!(!pdm.pdm_active(), "pdm close was not sent");
    let writes: Vec<String> = pdm.writes().iter().map(|w| w.trim().to_owned()).collect();
    let close = writes
        .iter()
        .rposition(|w| w == "pdm close")
        .expect("pdm close");
    let back = writes
        .iter()
        .rposition(|w| w == "pd req=1" || w == "pd hrst")
        .expect("back to 5 V");
    assert!(back < close, "{writes:?}");
}
