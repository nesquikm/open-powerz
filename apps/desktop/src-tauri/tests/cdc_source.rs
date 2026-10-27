//! The ADCNEW stream as a measurement source in the desktop backend
//! (FR 1HHT02, AC .4 and .5): a CDC source choice on connect, fed into the
//! same Batch / Gap events, history, statistics and recorder as the bulk
//! queue; the demo meter offers both; the bulk path never uses 0x01 / 0x03
//! and sends 0x02 only as CONNECT.

mod common;

use std::io::{self, Write};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::*;
use open_powerz_desktop_lib::devices::{AppOpener, DeviceManager};
use open_powerz_desktop_lib::worker::{
    BackendEvent, GapKind, MeasurementSource, Status, Worker, WorkerConfig,
};
use powerz_protocol::decode::Rate;
use powerz_protocol::session::SystemClock;

/// A recording file kept in memory.
#[derive(Clone, Default)]
struct MemFile(Arc<Mutex<Vec<u8>>>);

impl Write for MemFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl MemFile {
    fn lines(&self) -> Vec<String> {
        String::from_utf8_lossy(&self.0.lock().unwrap())
            .lines()
            .map(str::to_owned)
            .collect()
    }
}

fn cdc_config(rate: Rate) -> WorkerConfig {
    WorkerConfig {
        source: MeasurementSource::Cdc,
        ..WorkerConfig::new(rate, 1)
    }
}

fn cdc_worker(
    scenario: Scenario,
    faults: FaultPlan,
) -> (Worker<SimDevice, SimClock>, SimClock, Events) {
    let clock = clock();
    let dev = SimDevice::with_clock(Persona::Cdc, scenario, clock.clone()).with_faults(faults);
    let (events, sink) = capture(&clock);
    let worker = Worker::start(dev, clock.clone(), cdc_config(Rate::Sps50), sink)
        .unwrap_or_else(|e| panic!("CDC worker did not start: {e:?}"));
    (worker, clock, events)
}

/// (VBUS, IBUS) of every batched sample, in order.
fn values(events: &Events) -> Vec<(f64, f64)> {
    events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|(_, e)| match e {
            BackendEvent::Batch(b) => Some(
                b.samples
                    .iter()
                    .map(|s| (s.vbus_v, s.ibus_a))
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
        .flatten()
        .collect()
}

fn gaps(events: &Events) -> Vec<(GapKind, u64, u64)> {
    events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|(_, e)| match e {
            BackendEvent::Gap(g) => Some((g.kind, g.from_ms, g.to_ms)),
            _ => None,
        })
        .collect()
}

// ---- AC-1HHT02.4: the source choice ----

#[test]
fn measurement_source_names_on_the_wire() {
    assert_eq!(
        serde_json::to_value(MeasurementSource::Queue).unwrap(),
        serde_json::json!("queue")
    );
    assert_eq!(
        serde_json::to_value(MeasurementSource::Cdc).unwrap(),
        serde_json::json!("cdc")
    );
    let cdc: MeasurementSource = serde_json::from_str("\"cdc\"").unwrap();
    assert_eq!(cdc, MeasurementSource::Cdc);
    let queue: MeasurementSource = serde_json::from_str("\"queue\"").unwrap();
    assert_eq!(queue, MeasurementSource::Queue);
    assert!(serde_json::from_str::<MeasurementSource>("\"hid\"").is_err());
    assert_eq!(
        WorkerConfig::new(Rate::Sps50, 1).source,
        MeasurementSource::Queue,
        "polling the queue stays the default"
    );
}

// ---- AC-1HHT02.4: CDC source feeds the same events ----

#[test]
fn cdc_source_streams_batches_at_the_requested_rate() {
    let (mut worker, clock, events) = cdc_worker(Scenario::Load5v1a, FaultPlan::new());
    assert!(
        statuses(&events)
            .iter()
            .any(|s| matches!(s, Status::Connected(_))),
        "no connected status"
    );
    run_for(&mut worker, &clock, 2_000);
    let t = samples(&events);
    assert!(
        (85..=105).contains(&t.len()),
        "{} samples in 2 s at 50/s",
        t.len()
    );
    for pair in t.windows(2) {
        assert_eq!(pair[1] - pair[0], 20, "{t:?}");
    }
    for (v, i) in values(&events) {
        assert!((v - 5.0).abs() < 0.1, "VBUS {v}");
        assert!((i - 1.0).abs() < 0.05, "IBUS {i}");
    }
    assert!(gaps(&events).is_empty(), "{:?}", gaps(&events));
    worker.stop();
    assert!(
        statuses(&events)
            .iter()
            .any(|s| matches!(s, Status::Disconnected { error: None })),
        "stop ends the session cleanly"
    );
}

#[test]
fn cdc_source_records_like_the_queue_source() {
    let (mut worker, clock, _events) = cdc_worker(Scenario::Load5v1a, FaultPlan::new());
    run_for(&mut worker, &clock, 500);
    let file = MemFile::default();
    worker
        .start_recording(Box::new(file.clone()))
        .expect("start recording");
    run_for(&mut worker, &clock, 1_000);
    let summary = worker.stop_recording().expect("stop recording");
    let lines = file.lines();
    assert!(
        summary.rows >= 45,
        "only {} rows in 1 s at 50/s",
        summary.rows
    );
    assert_eq!(
        lines.len() as u64,
        summary.rows + 1,
        "header plus one row per record"
    );
    assert!(summary.gaps.is_empty(), "{:?}", summary.gaps);
    // Same columns as a queue recording: the first data row starts the
    // recording's clock.
    assert!(lines[1].starts_with("0:00:00.000,"), "{}", lines[1]);
    worker.stop();
}

#[test]
fn a_dropped_cdc_frame_becomes_a_stream_gap() {
    let (mut worker, clock, events) =
        cdc_worker(Scenario::Load5v1a, FaultPlan::new().corrupt_adcnew_at(10));
    run_for(&mut worker, &clock, 1_000);
    let g = gaps(&events);
    assert_eq!(g.len(), 1, "{g:?}");
    assert_eq!(g[0].0, GapKind::Stream);
    assert!(g[0].2 > g[0].1, "{g:?}");
    worker.stop();
}

#[test]
fn the_cdc_source_streams_past_the_meters_hour() {
    let clock = clock();
    let dev = SimDevice::with_clock(Persona::Cdc, Scenario::Load5v1a, clock.clone());
    let (events, sink) = capture(&clock);
    let mut worker = Worker::start(dev, clock.clone(), cdc_config(Rate::Sps2), sink)
        .unwrap_or_else(|e| panic!("CDC worker did not start: {e:?}"));
    run_for(&mut worker, &clock, 70 * 60 * 1_000);
    let t = samples(&events);
    let last = *t.last().expect("samples");
    assert!(
        u64::from(last) >= 69 * 60 * 1_000,
        "the stream ended at {last} ms: no keep-alive"
    );
    worker.stop();
}

// ---- AC-1HHT02.5: the bulk queue path never uses the ADCNEW types ----

#[test]
fn the_queue_source_sends_0x02_only_as_connect_and_never_0x01_or_0x03() {
    let clock = clock();
    let pipe = Pipe::sim(
        Scenario::Load5v1a,
        FaultPlan::new(),
        &clock,
        PipeOpts::default(),
    );
    let log = Arc::clone(&pipe.log);
    let (_events, sink) = capture(&clock);
    let mut worker = Worker::start(pipe, clock.clone(), WorkerConfig::new(Rate::Sps50, 1), sink)
        .unwrap_or_else(|e| panic!("worker did not start: {e:?}"));
    run_for(&mut worker, &clock, 2_000);
    worker.stop();
    let log = log.lock().unwrap();
    assert!(log.of_type(0x01).is_empty(), "{:?}", log.of_type(0x01));
    assert!(log.of_type(0x03).is_empty(), "{:?}", log.of_type(0x03));
    let connects = log.of_type(0x02);
    assert!(!connects.is_empty(), "no CONNECT on bulk");
    for (_, f) in connects {
        assert_eq!(f.len(), 4, "0x02 only as CONNECT: {f:02x?}");
        assert_eq!(&f[2..], &[0x00, 0x00], "CONNECT: {f:02x?}");
    }
}

// ---- AC-1HHT02.4: the demo meter offers the CDC source ----

#[test]
fn the_demo_meter_streams_through_the_cdc_source() {
    let mut m = DeviceManager::new(|| Ok(vec![]), AppOpener, SystemClock);
    let (tx, rx) = mpsc::channel();
    m.connect_source(
        "demo:load-5v-1a",
        50,
        MeasurementSource::Cdc,
        move |e: BackendEvent| {
            let _ = tx.send(e);
        },
    )
    .expect("connect with the CDC source");
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut vbus = Vec::new();
    while vbus.len() < 20 {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(BackendEvent::Batch(b)) => vbus.extend(b.samples.iter().map(|s| s.vbus_v)),
            Ok(BackendEvent::Status(Status::Disconnected { error })) => {
                panic!("session ended: {error:?}")
            }
            Ok(_) => {}
            Err(e) => panic!("no samples from the demo CDC source ({e:?})"),
        }
    }
    assert!(vbus.iter().all(|v| (v - 5.0).abs() < 0.1), "{vbus:?}");
    m.disconnect().expect("disconnect");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(BackendEvent::Status(Status::Disconnected { .. })) => break,
            Ok(_) => {}
            Err(e) => panic!("no disconnected status ({e:?})"),
        }
    }
}

#[test]
fn the_cdc_source_refuses_records_and_the_pd_sniffer() {
    // Records need memory reads and auth, the sniffer needs 0x10; none of
    // them is checked on the CDC pipe, so a CDC session refuses them.
    let (mut worker, _clock, _events) = cdc_worker(Scenario::Load5v1a, FaultPlan::new());
    for (what, refused) in [
        ("records list", worker.list_records().err()),
        ("record download", worker.download_record(1).err()),
        ("PD sniffer", worker.start_pd().err()),
    ] {
        let e = refused.unwrap_or_else(|| panic!("{what} must be refused on the CDC source"));
        assert!(e.message.contains("USB queue"), "{what}: {}", e.message);
    }
    worker.stop();
}

#[test]
fn a_trigger_is_refused_while_the_cdc_source_holds_the_port() {
    let mut m = DeviceManager::new(|| Ok(vec![]), AppOpener, SystemClock);
    m.connect_source("demo:load-5v-1a", 50, MeasurementSource::Cdc, |_| {})
        .expect("connect with the CDC source");
    let e = m
        .trigger_opener()
        .expect_err("a trigger must not open the port the stream holds");
    assert!(e.message.contains("USB queue"), "{}", e.message);
    m.disconnect().expect("disconnect");
}
