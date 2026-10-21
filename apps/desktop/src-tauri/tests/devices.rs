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

/// Sample times of every batch in `events`.
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

#[test]
fn reconnecting_the_same_serial_continues_the_history_and_the_stats() {
    use open_powerz_desktop_lib::history::Series;
    let (mut m, _opens, _) = manager(vec![FaultPlan::new().disconnect_after_polls(20)]);
    let (rx, sink) = channel_sink();
    m.connect("demo:load-5v-1a", 50, sink).unwrap();
    let first = recv_until(&rx, |e| {
        matches!(e, BackendEvent::Status(Status::Disconnected { .. }))
    });
    let kept = sample_times(&first);
    assert!(!kept.is_empty(), "no samples before the unplug");

    let (rx2, sink2) = channel_sink();
    m.connect("demo:load-5v-1a", 50, sink2).unwrap();
    let second = recv_until(
        &rx2,
        |e| matches!(e, BackendEvent::Batch(b) if !b.samples.is_empty()),
    );
    let seen = sample_times(&second);
    let stats = m.stats().expect("stats");
    let range = m
        .history_range(&[Series::Vbus], 0, u64::MAX, 8_192)
        .expect("history_range");
    m.disconnect().unwrap();

    assert!(
        stats.samples as usize >= kept.len() + seen.len(),
        "stats count {} samples; the two connections sent {} + {}",
        stats.samples,
        kept.len(),
        seen.len()
    );
    // The meter restarted its clock, but the session timeline goes on.
    assert!(
        seen[0] > kept[kept.len() - 1],
        "second connection starts at {} after the first ended at {}",
        seen[0],
        kept[kept.len() - 1]
    );
    let buckets = &range[0].buckets;
    assert_eq!(
        buckets.first().map(|b| b.t_ms),
        Some(u64::from(kept[0])),
        "the history lost the first connection"
    );
}

/// AC-272WSN.3: a user disconnect and reconnect is a break between runs too.
#[test]
fn a_user_disconnect_and_reconnect_leaves_a_history_gap_too() {
    let (mut m, _opens, _) = manager(vec![]);
    let (rx, sink) = channel_sink();
    m.connect("demo:load-5v-1a", 50, sink).unwrap();
    let first = recv_until(
        &rx,
        |e| matches!(e, BackendEvent::Batch(b) if !b.samples.is_empty()),
    );
    m.disconnect().unwrap();
    let kept = sample_times(&first);
    assert!(m.history_gaps(0, u64::MAX).unwrap().is_empty(), "one run");

    let (rx2, sink2) = channel_sink();
    m.connect("demo:load-5v-1a", 50, sink2).unwrap();
    let second = recv_until(
        &rx2,
        |e| matches!(e, BackendEvent::Batch(b) if !b.samples.is_empty()),
    );
    let seen = sample_times(&second);
    // The break is in the history as soon as the second run's first record
    // is; allow the worker a moment on a slow runner before judging.
    let deadline = Instant::now() + Duration::from_secs(2);
    let gaps = loop {
        let gaps = m.history_gaps(0, u64::MAX).expect("history_gaps");
        if !gaps.is_empty() || Instant::now() > deadline {
            break gaps;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let len = m.session_store().map(|s| s.history_len()).unwrap_or(0);
    m.disconnect().unwrap();
    // No outage event says so (the user disconnected), but the chart must
    // still break the line between the two connections.
    assert_eq!(
        gaps.len(),
        1,
        "{gaps:?}; history holds {len} records; first run {} samples ending at {:?}, second starting at {:?}",
        kept.len(),
        kept.last(),
        seen.first()
    );
    assert!(gaps[0].from_ms >= u64::from(kept[kept.len() - 1]));
    assert_eq!(gaps[0].to_ms, u64::from(seen[0]));
}

// ---- the data stays readable after the meter is gone ----

#[test]
fn history_stats_and_export_still_answer_after_a_disconnect() {
    use open_powerz_desktop_lib::history::Series;
    let (mut m, _opens, _) = manager(vec![]);
    let err = m.stats().expect_err("stats before any session");
    assert_eq!(err.message, "No meter is connected.");

    let (rx, sink) = channel_sink();
    m.connect("demo:load-5v-1a", 50, sink).unwrap();
    recv_until(
        &rx,
        |e| matches!(e, BackendEvent::Batch(b) if !b.samples.is_empty()),
    );
    m.disconnect().unwrap();

    let stats = m.stats().expect("stats after disconnect");
    assert!(stats.samples > 0);
    let range = m
        .history_range(&[Series::Vbus], 0, u64::MAX, 100)
        .expect("history_range after disconnect");
    assert!(!range[0].buckets.is_empty());

    let dir = std::env::temp_dir().join(format!("powerz-kept-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("kept.csv");
    let rows = m
        .export_session(&path, |_| {})
        .expect("export after disconnect")
        .join()
        .unwrap()
        .expect("export");
    assert_eq!(rows, stats.samples);
    let lines = std::fs::read_to_string(&path).unwrap().lines().count() as u64;
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(lines, rows + 1);

    m.reset_stats().expect("reset_stats after disconnect");
    assert_eq!(m.stats().unwrap().samples, 0);
    m.clear_history().expect("clear_history after disconnect");
    let range = m.history_range(&[Series::Vbus], 0, u64::MAX, 100).unwrap();
    assert!(range[0].buckets.is_empty());
}

// ---- FR VEDDFD: re-attach a new page to the running session ----

use open_powerz_desktop_lib::devices::AttachedSession;
use open_powerz_desktop_lib::recording::RecordingEvent;

/// The `connected` info the worker sent first.
fn connected_info(events: &[BackendEvent]) -> open_powerz_desktop_lib::worker::ConnectedInfo {
    events
        .iter()
        .find_map(|e| match e {
            BackendEvent::Status(Status::Connected(i)) => Some(i.clone()),
            _ => None,
        })
        .expect("the session never connected")
}

/// Everything already waiting in `rx`.
fn drain(rx: &mpsc::Receiver<BackendEvent>) -> Vec<BackendEvent> {
    let mut got = Vec::new();
    while let Ok(e) = rx.try_recv() {
        got.push(e);
    }
    got
}

fn temp_csv(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("powerz-attach-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("powerz-attach.csv");
    (dir, path)
}

/// AC-VEDDFD.1: with no session running, attach answers nothing.
#[test]
fn attach_without_a_running_session_returns_none() {
    let (mut m, opens, _) = manager(vec![]);
    let (rx, sink) = channel_sink();
    assert_eq!(m.attach(sink), None, "a session before any connect");
    assert_eq!(opens.load(Ordering::SeqCst), 0, "attach opened a device");
    assert!(drain(&rx).is_empty(), "the unused sink got events");

    // Nor after the session ended.
    let (rx1, sink1) = channel_sink();
    m.connect("demo:load-5v-1a", 50, sink1).unwrap();
    recv_until(
        &rx1,
        |e| matches!(e, BackendEvent::Batch(b) if !b.samples.is_empty()),
    );
    m.disconnect().unwrap();
    let (_rx2, sink2) = channel_sink();
    assert_eq!(m.attach(sink2), None, "a session after disconnect");
}

/// AC-VEDDFD.1: a session that ended by itself (unplug) is not running either.
#[test]
fn attach_after_an_unplug_returns_none() {
    let (mut m, _opens, _) = manager(vec![FaultPlan::new().disconnect_after_polls(20)]);
    let (rx, sink) = channel_sink();
    m.connect("demo:load-5v-1a", 50, sink).unwrap();
    recv_until(&rx, |e| {
        matches!(e, BackendEvent::Status(Status::Disconnected { .. }))
    });
    // The worker thread may still be winding down; give it a moment.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let (_rx2, sink2) = channel_sink();
        let got = m.attach(sink2);
        if got.is_none() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "an unplugged session still attaches: {got:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// AC-VEDDFD.2: the snapshot describes the running session as connect reported it.
#[test]
fn attach_returns_the_running_session_snapshot() {
    let (mut m, opens, _) = manager(vec![]);
    let (rx, sink) = channel_sink();
    m.connect("demo:load-5v-1a", 50, sink).unwrap();
    let before = recv_until(
        &rx,
        |e| matches!(e, BackendEvent::Batch(b) if !b.samples.is_empty()),
    );
    let info = connected_info(&before);

    let (_rx2, sink2) = channel_sink();
    let snap: AttachedSession = m.attach(sink2).expect("no snapshot of a running session");
    assert_eq!(opens.load(Ordering::SeqCst), 1, "attach reopened the meter");
    assert_eq!(snap.device_id, "demo:load-5v-1a");
    assert_eq!(snap.model, info.model);
    assert_eq!(snap.firmware_version, info.firmware_version);
    assert_eq!(snap.serial.as_deref(), Some(info.serial.as_str()));
    assert_eq!(snap.rate_sps, Some(50));
    assert!(!snap.rate_locked);
    assert_eq!(snap.session_id, info.session_id);
    assert_eq!(snap.recording, None, "no recording is open");

    let v = serde_json::to_value(&snap).unwrap();
    assert_eq!(v["device_id"], "demo:load-5v-1a");
    assert_eq!(
        v["firmware_version"],
        serde_json::json!(info.firmware_version)
    );
    assert_eq!(v["rate_sps"], 50);
    assert_eq!(v["rate_locked"], false);
    assert_eq!(v["session_id"], serde_json::json!(info.session_id));
    assert!(v["recording"].is_null());
    m.disconnect().unwrap();
}

/// AC-VEDDFD.2 / AC-VEDDFD.5: after attach the stream goes to the new sink only,
/// and the session is not restarted.
#[test]
fn attach_moves_the_stream_to_the_new_sink_only() {
    let (mut m, opens, _) = manager(vec![]);
    let (old_rx, old_sink) = channel_sink();
    m.connect("demo:load-5v-1a", 50, old_sink).unwrap();
    let before = recv_until(
        &old_rx,
        |e| matches!(e, BackendEvent::Batch(b) if !b.samples.is_empty()),
    );
    let (new_rx, new_sink) = channel_sink();
    m.attach(new_sink).expect("attach to the running session");
    // What the old page got before the swap is fine; nothing may follow.
    let mut old_seen = before;
    old_seen.extend(drain(&old_rx));

    let after = recv_until(&new_rx, {
        let mut n = 0;
        move |e| {
            if matches!(e, BackendEvent::Batch(b) if !b.samples.is_empty()) {
                n += 1;
            }
            n >= 5
        }
    });
    assert!(
        !after
            .iter()
            .any(|e| matches!(e, BackendEvent::Status(Status::Connected(_)))),
        "the session reconnected"
    );
    assert_eq!(
        opens.load(Ordering::SeqCst),
        1,
        "the meter was opened again"
    );
    // The stream continues: the new page's samples follow the old page's.
    let old_times = sample_times(&old_seen);
    let new_times = sample_times(&after);
    assert!(
        new_times[0] > *old_times.last().unwrap(),
        "the new sink starts at {} before the old one ended at {}",
        new_times[0],
        old_times.last().unwrap()
    );

    m.disconnect().unwrap();
    recv_until(&new_rx, |e| {
        matches!(e, BackendEvent::Status(Status::Disconnected { .. }))
    });
    let late = drain(&old_rx);
    assert!(
        late.is_empty(),
        "the old sink still got {} events after attach",
        late.len()
    );
}

/// AC-VEDDFD.3: an open recording keeps running and the snapshot carries its rows
/// and elapsed time; its progress goes to the new sink.
#[test]
fn attach_snapshot_carries_the_open_recording() {
    let (dir, path) = temp_csv("recording");
    let (mut m, _opens, _) = manager(vec![]);
    let (old_rx, old_sink) = channel_sink();
    m.connect("demo:load-5v-1a", 50, old_sink).unwrap();
    recv_until(
        &old_rx,
        |e| matches!(e, BackendEvent::Batch(b) if !b.samples.is_empty()),
    );
    m.start_recording(&path).expect("start recording");
    // Wait for a progress event, so rows and time have accumulated.
    let progress = recv_until(&old_rx, |e| {
        matches!(
            e,
            BackendEvent::Recording(RecordingEvent::Progress { rows, .. }) if *rows > 0
        )
    });
    let (rows_before, elapsed_before) = progress
        .iter()
        .rev()
        .find_map(|e| match e {
            BackendEvent::Recording(RecordingEvent::Progress { rows, elapsed_ms }) => {
                Some((*rows, *elapsed_ms))
            }
            _ => None,
        })
        .unwrap();

    let (new_rx, new_sink) = channel_sink();
    let snap = m.attach(new_sink).expect("attach while recording");
    let rec = snap
        .recording
        .expect("the snapshot lost the open recording");
    assert!(
        rec.rows >= rows_before,
        "snapshot rows {} < {rows_before} already reported",
        rec.rows
    );
    assert!(
        rec.elapsed_ms >= elapsed_before && rec.elapsed_ms > 0,
        "snapshot elapsed {} ms < {elapsed_before} ms already reported",
        rec.elapsed_ms
    );
    let v = serde_json::to_value(snap.recording).unwrap();
    assert_eq!(v["rows"], serde_json::json!(rec.rows));
    assert_eq!(v["elapsed_ms"], serde_json::json!(rec.elapsed_ms));

    // The recording goes on, reporting to the new page.
    recv_until(&new_rx, |e| {
        matches!(
            e,
            BackendEvent::Recording(RecordingEvent::Progress { rows, .. }) if *rows > rec.rows
        )
    });
    // AC-VEDDFD.5: Stop works on the re-attached session.
    let summary = m.stop_recording().expect("stop after attach");
    assert!(summary.rows > rec.rows);
    m.disconnect().unwrap();
    let lines = std::fs::read_to_string(&path).unwrap().lines().count() as u64;
    assert_eq!(
        lines,
        summary.rows + 1,
        "the file lost rows across the attach"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// AC-VEDDFD.4: history and statistics still hold everything from before the attach.
#[test]
fn history_and_stats_reach_back_before_the_attach() {
    use open_powerz_desktop_lib::history::Series;
    let (mut m, _opens, _) = manager(vec![]);
    let (old_rx, old_sink) = channel_sink();
    m.connect("demo:load-5v-1a", 50, old_sink).unwrap();
    let before = recv_until(
        &old_rx,
        |e| matches!(e, BackendEvent::Batch(b) if !b.samples.is_empty()),
    );
    let first = sample_times(&before)[0];
    let (new_rx, new_sink) = channel_sink();
    m.attach(new_sink).expect("attach");
    let after = recv_until(
        &new_rx,
        |e| matches!(e, BackendEvent::Batch(b) if !b.samples.is_empty()),
    );
    let stats = m.stats().expect("stats after attach");
    let range = m
        .history_range(&[Series::Vbus], 0, u64::MAX, 8_192)
        .expect("history after attach");
    m.disconnect().unwrap();
    assert_eq!(
        range[0].buckets.first().map(|b| b.t_ms),
        Some(u64::from(first)),
        "the history lost the samples from before the attach"
    );
    assert!(stats.samples as usize >= sample_times(&before).len() + sample_times(&after).len());
}

/// AC-VEDDFD.5: rate changes, Record and Export work on a re-attached session.
#[test]
fn rate_record_and_export_work_after_attach() {
    let (dir, path) = temp_csv("controls");
    let (mut m, _opens, _) = manager(vec![]);
    let (old_rx, old_sink) = channel_sink();
    m.connect("demo:load-5v-1a", 50, old_sink).unwrap();
    recv_until(
        &old_rx,
        |e| matches!(e, BackendEvent::Batch(b) if !b.samples.is_empty()),
    );
    let (new_rx, new_sink) = channel_sink();
    m.attach(new_sink).expect("attach");

    m.set_rate(10).expect("rate change after attach");
    recv_until(&new_rx, |e| {
        matches!(e, BackendEvent::Status(Status::Rate { rate_sps: 10, .. }))
    });
    let (_rx3, sink3) = channel_sink();
    let snap = m.attach(sink3).expect("attach again");
    assert_eq!(
        snap.rate_sps,
        Some(10),
        "the snapshot missed the rate change"
    );
    // The third sink is the live one now; move back to a receiver we hold.
    let (rx4, sink4) = channel_sink();
    m.attach(sink4).expect("attach once more");

    m.start_recording(&path).expect("record after attach");
    recv_until(&rx4, |e| {
        matches!(e, BackendEvent::Recording(RecordingEvent::Started))
    });
    m.stop_recording().expect("stop after attach");
    let export = dir.join("export.csv");
    let rows = m
        .export_session(&export, |_| {})
        .expect("export after attach")
        .join()
        .unwrap()
        .expect("export");
    assert!(rows > 0);
    m.disconnect().unwrap();
    recv_until(&rx4, |e| {
        matches!(e, BackendEvent::Status(Status::Disconnected { .. }))
    });
    let _ = std::fs::remove_dir_all(dir);
}
