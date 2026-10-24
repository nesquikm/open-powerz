//! Hardware-in-the-loop backend suite (FR AH2AEN, AC .8 / .9): the app's
//! own device manager and session worker (FR VHY378) against the attached
//! KM003C over bulk.
//!
//! Opt-in: every `hil_*` test returns at once unless `POWERZ_HIL=1`; CI
//! never sets it. Run with nothing plugged into the meter's ports:
//!
//! ```text
//! POWERZ_HIL=1 cargo test -p open-powerz-desktop --locked --test hil -- --nocapture
//! ```
//!
//! The worker sends only the safe command set of `docs/protocol.md`. Nothing
//! here prints or asserts the meter's serial number or UID: listings and
//! `connected` statuses are never formatted into output.
//!
//! `csv_round_trip_dry_run_against_the_demo_device` runs the record / export
//! parity flow in the normal gate against the demo device, so the CSV
//! re-rendering cannot rot between hardware sessions.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use open_powerz_desktop_lib::devices::{self, AppOpener, DeviceListing, DeviceManager};
use open_powerz_desktop_lib::worker::{BackendEvent, GapKind, Status};
use powerz_protocol::decode::{QueueRecord, Rate};
use powerz_protocol::session::SystemClock;
use powerz_protocol::transport::discovery;
use powerz_record::{CSV_HEADER, CsvRecorder};

type Manager = DeviceManager<AppOpener, SystemClock>;

fn hil_enabled() -> bool {
    std::env::var("POWERZ_HIL").as_deref() == Ok("1")
}

/// Skip unless `POWERZ_HIL=1`; returns `true` when the test should run.
fn hil_or_skip(name: &str) -> bool {
    if hil_enabled() {
        true
    } else {
        eprintln!("{name}: skipped (set POWERZ_HIL=1 with a meter attached)");
        false
    }
}

/// One meter, one session at a time: the tests take turns.
fn one_at_a_time() -> MutexGuard<'static, ()> {
    static METER: Mutex<()> = Mutex::new(());
    METER.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The app's manager: OS discovery, the app's opener, the system clock.
fn manager() -> Manager {
    DeviceManager::new(discovery::list_devices, AppOpener, SystemClock)
}

/// The id of the first attached meter (never the demo device). Panics
/// without naming the meter, so no serial reaches the output.
fn meter_id(m: &Manager) -> String {
    let meters: Vec<DeviceListing> = m
        .list_devices()
        .into_iter()
        .filter(|d| d.demo_scenario.is_none())
        .collect();
    let n = meters.len();
    meters
        .into_iter()
        .find(|d| d.model == "KM003C")
        .map(|d| d.id)
        .unwrap_or_else(|| panic!("no KM003C attached ({n} other POWER-Z meters listed)"))
}

fn connect(m: &mut Manager, id: &str, rate_sps: u32) -> mpsc::Receiver<BackendEvent> {
    let (tx, rx) = mpsc::channel();
    m.connect(id, rate_sps, move |e| {
        let _ = tx.send(e);
    })
    .unwrap_or_else(|e| panic!("connect failed: {}", e.message));
    rx
}

/// What a stretch of the stream delivered.
#[derive(Debug, Default)]
struct Window {
    samples: Vec<open_powerz_desktop_lib::batch::Sample>,
    stream_gaps: usize,
    errors: Vec<String>,
    disconnected: bool,
    rate_sps: Option<u32>,
    rate_locked: bool,
}

fn absorb(w: &mut Window, event: BackendEvent) {
    match event {
        BackendEvent::Batch(b) => w.samples.extend(b.samples),
        BackendEvent::Gap(g) if g.kind == GapKind::Stream => w.stream_gaps += 1,
        BackendEvent::Gap(_)
        | BackendEvent::Recording(_)
        | BackendEvent::PdState(_)
        | BackendEvent::PdRows(_)
        | BackendEvent::PdStatus(_)
        | BackendEvent::Status(Status::Waiting { .. }) => {}
        BackendEvent::Error(e) => w.errors.push(e.message),
        // Only the fields needed: a connected status carries the serial.
        BackendEvent::Status(Status::Connected(info)) => {
            w.rate_sps = info.rate_sps.or(w.rate_sps);
            w.rate_locked = info.rate_locked;
        }
        BackendEvent::Status(Status::Rate {
            rate_sps,
            rate_locked,
        }) => {
            w.rate_sps = Some(rate_sps);
            w.rate_locked = rate_locked;
        }
        BackendEvent::Status(Status::Disconnected { error }) => {
            w.disconnected = true;
            if let Some(e) = error {
                w.errors.push(e.message);
            }
        }
    }
}

/// Everything the session sends for `d` of real time.
fn collect_for(rx: &mpsc::Receiver<BackendEvent>, d: Duration) -> Window {
    let mut w = Window::default();
    let end = Instant::now() + d;
    loop {
        let left = end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return w;
        }
        match rx.recv_timeout(left) {
            Ok(e) => absorb(&mut w, e),
            Err(mpsc::RecvTimeoutError::Timeout) => return w,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                w.disconnected = true;
                return w;
            }
        }
    }
}

/// Wait (real time, at most 10 s) for the first batch with samples.
fn first_samples(rx: &mpsc::Receiver<BackendEvent>) -> Window {
    let mut w = Window::default();
    let end = Instant::now() + Duration::from_secs(10);
    while w.samples.is_empty() {
        let left = end.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(e) => absorb(&mut w, e),
            Err(e) => panic!("no samples within 10 s ({e:?}); errors: {:?}", w.errors),
        }
        assert!(!w.disconnected, "the session ended: {:?}", w.errors);
    }
    w
}

/// Stream `secs` at `rate_sps` (after `settle` of warm-up) and check: no
/// gaps, no errors, and a sample count within 2% of rate × duration.
fn assert_clean_stream(rx: &mpsc::Receiver<BackendEvent>, rate_sps: u32, secs: u64) {
    let _ = collect_for(rx, Duration::from_secs(1));
    let w = collect_for(rx, Duration::from_secs(secs));
    assert!(
        !w.disconnected,
        "the session ended at {rate_sps} SPS: {:?}",
        w.errors
    );
    assert!(
        w.errors.is_empty(),
        "errors at {rate_sps} SPS: {:?}",
        w.errors
    );
    assert!(
        !w.rate_locked,
        "the meter refused authentication: the rate is locked"
    );
    assert_eq!(w.stream_gaps, 0, "stream gaps at {rate_sps} SPS");
    // Device time never skips more than a few sample periods (at least 5 ms:
    // three 1 ms periods at 1000 SPS leave no margin for timestamp jitter).
    let period_ms = 1000 / rate_sps;
    let worst_step = w
        .samples
        .windows(2)
        .map(|p| p[1].t_ms.wrapping_sub(p[0].t_ms))
        .max()
        .unwrap_or(0);
    assert!(
        worst_step <= (3 * period_ms.max(1)).max(5),
        "device time jumped {worst_step} ms at {rate_sps} SPS (period {period_ms} ms)"
    );
    let want = f64::from(rate_sps) * secs as f64;
    let got = w.samples.len() as f64;
    assert!(
        (got - want).abs() <= want * 0.02,
        "{got} samples in {secs} s at {rate_sps} SPS, want {want} ± 2%"
    );
}

// ---- CSV round trip: parse a recorder file, render it again ----

fn parse_elapsed_ms(cell: &str) -> u64 {
    let (hm, sec) = cell.rsplit_once(':').expect("h:mm:ss.mmm");
    let (h, m) = hm.split_once(':').expect("h:mm");
    let (s, ms) = sec.split_once('.').expect("ss.mmm");
    let n = |x: &str| x.parse::<u64>().expect("a number in ElapsedTime");
    ((n(h) * 60 + n(m)) * 60 + n(s)) * 1000 + n(ms)
}

fn micro(cell: &str) -> i32 {
    let v: f64 = cell.parse().expect("a decimal");
    (v * 1e6).round() as i32
}

/// Raw CC / D± code for volts at `rate` (0.1 mV at 2 SPS, 1 mV otherwise).
fn line_code(cell: &str, rate: Rate) -> u16 {
    let v: f64 = cell.parse().expect("a decimal");
    let per_v = if rate == Rate::Sps2 {
        10_000.0
    } else {
        1_000.0
    };
    (v * per_v).round() as u16
}

/// Every row of a recorder CSV as a queue record with its TEMP (`None` when
/// the cell is empty), plus the wall clock of the first row in ms. The
/// columns are `ElapsedTime,Unix,VBUS,IBUS,DP,DM,CC1,CC2,TEMP,CHARGE,ENERGY,PWR`.
fn parse_csv(text: &str, rate: Rate) -> (Vec<(QueueRecord, Option<f64>)>, u64) {
    let mut lines = text.lines();
    assert_eq!(lines.next(), Some(CSV_HEADER), "not the vendor header");
    let mut unix0 = None;
    let records = lines
        .map(|line| {
            let c: Vec<&str> = line.split(',').collect();
            assert_eq!(c.len(), 12, "row with {} columns", c.len());
            let unix: f64 = c[1].parse().expect("Unix seconds");
            unix0.get_or_insert((unix * 1000.0).round() as u64);
            let t = u32::try_from(parse_elapsed_ms(c[0])).expect("elapsed fits u32");
            let temp = (!c[8].is_empty()).then(|| c[8].parse::<f64>().expect("TEMP in °C"));
            let record = QueueRecord::from_raw(
                t,
                micro(c[2]),
                micro(c[3]),
                [
                    line_code(c[6], rate),
                    line_code(c[7], rate),
                    line_code(c[4], rate),
                    line_code(c[5], rate),
                ],
                rate,
            );
            (record, temp)
        })
        .collect();
    (records, unix0.unwrap_or(0))
}

/// Re-render `file` through `CsvRecorder` (the writer `powerz stream` uses)
/// and require it byte for byte. Returns its row count.
fn assert_rerenders_byte_for_byte(file: &Path, rate: Rate) -> usize {
    let text = std::fs::read_to_string(file).expect("the CSV exists");
    assert!(
        text.starts_with(&format!("{CSV_HEADER}\n")),
        "vendor header"
    );
    let (records, unix0) = parse_csv(&text, rate);
    assert!(!records.is_empty(), "{} has no rows", file.display());
    let mut csv = CsvRecorder::new(Vec::new()).unwrap();
    for (r, temp) in &records {
        csv.push_with_temp(r, unix0, *temp).unwrap();
    }
    let again = String::from_utf8(csv.finish().unwrap()).unwrap();
    assert!(
        again == text,
        "{} differs from its re-rendering (first differing line: {:?})",
        file.display(),
        text.lines().zip(again.lines()).find(|(a, b)| a != b)
    );
    records.len()
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("open-powerz-hil-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Connect to `id` at `rate_sps`, record `record_secs` to a file, stop,
/// export the session, disconnect; check both files and return their row
/// counts `(recording, export)`.
fn record_and_export(m: &mut Manager, id: &str, rate_sps: u32, record_secs: u64) -> (usize, usize) {
    let rate = Rate::from_sps(rate_sps).expect("a supported rate");
    let dir = temp_dir(&format!("record-{rate_sps}"));
    let (rec_path, export_path) = (dir.join("recording.csv"), dir.join("export.csv"));
    let rx = connect(m, id, rate_sps);
    let _ = first_samples(&rx);
    m.start_recording(&rec_path)
        .unwrap_or_else(|e| panic!("start recording: {}", e.message));
    let w = collect_for(&rx, Duration::from_secs(record_secs));
    assert!(
        w.errors.is_empty(),
        "errors while recording: {:?}",
        w.errors
    );
    let summary = m
        .stop_recording()
        .unwrap_or_else(|e| panic!("stop recording: {}", e.message));
    assert!(summary.gaps.is_empty(), "gaps while recording");
    let job = m
        .export_session(&export_path, |_| {})
        .unwrap_or_else(|e| panic!("export: {}", e.message));
    let exported_rows = job
        .join()
        .expect("the export thread ran")
        .unwrap_or_else(|e| panic!("export failed: {}", e.message));
    m.disconnect().unwrap();

    let rec_rows = assert_rerenders_byte_for_byte(&rec_path, rate);
    let export_rows = assert_rerenders_byte_for_byte(&export_path, rate);
    assert_eq!(rec_rows as u64, summary.rows, "recording rows vs summary");
    assert_eq!(export_rows as u64, exported_rows, "export rows vs result");
    assert!(
        export_rows >= rec_rows,
        "the export holds the whole session"
    );
    let _ = std::fs::remove_dir_all(dir);
    (rec_rows, export_rows)
}

// ---- AC-AH2AEN.8: connect, sane idle values, clean streams ----

#[test]
fn hil_first_batch_is_sane_with_nothing_plugged_in() {
    if !hil_or_skip("hil_first_batch_is_sane_with_nothing_plugged_in") {
        return;
    }
    let _meter = one_at_a_time();
    let mut m = manager();
    let id = meter_id(&m);
    let rx = connect(&mut m, &id, 50);
    let w = first_samples(&rx);
    m.disconnect().unwrap();
    for s in &w.samples {
        assert!(
            s.vbus_v < 0.5,
            "VBUS {} V with nothing plugged in",
            s.vbus_v
        );
        for (name, v) in [
            ("D+", s.dp_v),
            ("D−", s.dm_v),
            ("CC1", s.cc1_v),
            ("CC2", s.cc2_v),
        ] {
            assert!((0.0..=5.0).contains(&v), "{name} {v} V outside 0..5 V");
        }
    }
}

#[test]
fn hil_streams_30_s_at_50_and_1000_sps_without_gaps() {
    if !hil_or_skip("hil_streams_30_s_at_50_and_1000_sps_without_gaps") {
        return;
    }
    let _meter = one_at_a_time();
    let mut m = manager();
    let id = meter_id(&m);
    let rx = connect(&mut m, &id, 50);
    let _ = first_samples(&rx);
    assert_clean_stream(&rx, 50, 30);
    m.set_rate(1000)
        .unwrap_or_else(|e| panic!("set rate 1000: {}", e.message));
    assert_clean_stream(&rx, 1000, 30);
    m.disconnect().unwrap();
}

// ---- AC-TGE839.7: a real unplug and replug at 1000 SPS ----

/// Opt-in twice: needs a person to pull and replug the meter's data cable, so
/// it runs only with `POWERZ_HIL=1` and `POWERZ_HIL_REPLUG=1`. The test polls
/// the device list like the app's hotplug watcher and hands every list to
/// `devices_changed`. It prints timings only, never the serial.
#[test]
fn hil_replug_reconnects_within_3_s_at_1000_sps_with_an_outage_gap() {
    if !hil_or_skip("hil_replug_reconnects_within_3_s_at_1000_sps_with_an_outage_gap") {
        return;
    }
    if std::env::var("POWERZ_HIL_REPLUG").as_deref() != Ok("1") {
        eprintln!("hil_replug: skipped (set POWERZ_HIL_REPLUG=1 with a person at the cable)");
        return;
    }
    let _meter = one_at_a_time();
    let mut m = manager();
    let id = meter_id(&m);
    let serial = m
        .list_devices()
        .into_iter()
        .find(|d| d.id == id)
        .and_then(|d| d.serial)
        .unwrap_or_else(|| panic!("the meter reported no serial"));
    let rx = connect(&mut m, &id, 1000);
    let _ = first_samples(&rx);
    let poll = Duration::from_millis(devices::HOTPLUG_PERIOD_MS);

    eprintln!("hil_replug: streaming at 1000 SPS. UNPLUG the meter's data cable now.");
    let started = Instant::now();
    let deadline = started + Duration::from_secs(180);
    let mut waiting = false;
    let mut samples = 0usize;
    let mut listed = true;
    let mut last_batch = Instant::now();
    let mut quiet_reported = false;
    while !waiting {
        assert!(Instant::now() < deadline, "no unplug seen within 180 s");
        // What the OS lists, as the hotplug watcher would see it.
        let now_listed = m
            .list_devices()
            .iter()
            .any(|d| d.demo_scenario.is_none() && d.serial.as_deref() == Some(serial.as_str()));
        if now_listed != listed {
            listed = now_listed;
            eprintln!(
                "hil_replug: +{} ms the OS {} the meter",
                started.elapsed().as_millis(),
                if listed { "lists" } else { "no longer lists" }
            );
        }
        if !quiet_reported && last_batch.elapsed() > Duration::from_secs(1) {
            quiet_reported = true;
            eprintln!(
                "hil_replug: +{} ms no samples for over 1 s",
                started.elapsed().as_millis()
            );
        }
        match rx.recv_timeout(poll) {
            // Status kinds and error texts only: a connected status carries the serial.
            Ok(BackendEvent::Status(Status::Disconnected { error })) => eprintln!(
                "hil_replug: +{} ms disconnected ({})",
                started.elapsed().as_millis(),
                error.as_ref().map_or("no error", |e| e.message.as_str())
            ),
            Ok(BackendEvent::Error(e)) => eprintln!(
                "hil_replug: +{} ms error: {}",
                started.elapsed().as_millis(),
                e.message
            ),
            Ok(BackendEvent::Batch(b)) => {
                last_batch = Instant::now();
                quiet_reported = false;
                samples += b.samples.len();
                if samples >= 10_000 {
                    eprintln!(
                        "hil_replug: +{} ms still streaming",
                        started.elapsed().as_millis()
                    );
                    samples = 0;
                }
            }
            Ok(BackendEvent::Status(Status::Waiting { .. })) => {
                eprintln!("hil_replug: +{} ms waiting", started.elapsed().as_millis());
                waiting = true;
            }
            Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => panic!("the session sink closed"),
        }
        let _ = m.devices_changed(&m.list_devices());
    }
    assert!(m.waiting_for().is_some(), "the manager is not waiting");
    eprintln!("hil_replug: waiting for the meter. PLUG the data cable back in now.");

    // Poll like the watcher; time from the first list that shows the meter
    // again to the first samples of the reconnected session.
    let mut listed_at = None;
    let mut outage = None;
    let mut first_after = None;
    while first_after.is_none() {
        assert!(Instant::now() < deadline, "no reconnect within 180 s");
        let list = m.list_devices();
        if listed_at.is_none()
            && list
                .iter()
                .any(|d| d.demo_scenario.is_none() && d.serial.as_deref() == Some(serial.as_str()))
        {
            listed_at = Some(Instant::now());
        }
        let _ = m.devices_changed(&list);
        let end = Instant::now() + poll;
        while let Ok(e) = rx.recv_timeout(end.saturating_duration_since(Instant::now())) {
            match e {
                BackendEvent::Gap(g) if g.kind == GapKind::Outage => outage = Some(g),
                BackendEvent::Batch(b) if listed_at.is_some() && !b.samples.is_empty() => {
                    first_after = Some(Instant::now());
                    break;
                }
                _ => {}
            }
        }
    }
    let took = first_after.unwrap() - listed_at.unwrap();
    eprintln!(
        "hil_replug: listed again -> first samples in {} ms",
        took.as_millis()
    );
    assert!(took <= Duration::from_secs(3), "reconnect took {took:?}");
    let gap = outage.unwrap_or_else(|| panic!("no outage gap after the reconnect"));
    eprintln!("hil_replug: outage gap {} ms", gap.to_ms - gap.from_ms);
    assert!(gap.to_ms > gap.from_ms);
    let w = collect_for(&rx, Duration::from_secs(3));
    assert!(
        w.samples.len() >= 2_000,
        "only {} samples in 3 s at 1000 SPS",
        w.samples.len()
    );
    m.disconnect().unwrap();
}

// ---- AC-AH2AEN.9: 10 s recording + export, byte parity with the writer ----

#[test]
fn hil_recording_and_export_rerender_byte_for_byte() {
    if !hil_or_skip("hil_recording_and_export_rerender_byte_for_byte") {
        return;
    }
    let _meter = one_at_a_time();
    let mut m = manager();
    let id = meter_id(&m);
    let (rec_rows, _) = record_and_export(&mut m, &id, 50, 10);
    let want = 50.0 * 10.0;
    assert!(
        (rec_rows as f64 - want).abs() <= want * 0.05,
        "{rec_rows} recording rows, want {want} ± 5%"
    );
}

// ---- the record / export parity flow, in the normal gate, on the demo device ----

#[test]
fn csv_round_trip_dry_run_against_the_demo_device() {
    let mut m = DeviceManager::new(|| Ok(Vec::new()), AppOpener, SystemClock);
    let (rec_rows, export_rows) = record_and_export(&mut m, "demo:load-5v-1a", 50, 2);
    assert!(rec_rows > 0 && export_rows >= rec_rows);
}

// ---- AC-DGSQ6S.7: the Trigger panel's backend on the real meter ----

/// The desktop Trigger panel's backend driven the way the panel drives it,
/// on the real meter with the 140 W charger and NOTHING on the output: the
/// bulk session first, then the trigger session; detection; the PD
/// section; an unconfirmed request refused; confirmed 9 V (and 28 V when
/// offered) seen in the meter stream; Back to 5 V; disconnect closes all.
/// Needs `POWERZ_HIL=1` and `POWERZ_HIL_TRIGGER=1`.
#[test]
fn hil_trigger_panel_backend_drive() {
    use open_powerz_desktop_lib::trigger::{TriggerProtocol, TriggerRequest};
    if !hil_or_skip("hil_trigger_panel_backend_drive")
        || std::env::var("POWERZ_HIL_TRIGGER").as_deref() != Ok("1")
    {
        return;
    }
    let _meter = one_at_a_time();
    let mut m = manager();
    let id = meter_id(&m);
    let rx = connect(&mut m, &id, 50);
    first_samples(&rx);
    m.open_trigger()
        .unwrap_or_else(|e| panic!("open_trigger: {}", e.message));
    let c = m.trigger_control().expect("a trigger session is open");
    let pd = |position: u8| TriggerRequest::Pd {
        position,
        voltage_mv: None,
        current_ma: None,
    };
    let peak = |rx: &mpsc::Receiver<BackendEvent>, secs: u64| -> f64 {
        let w = collect_for(rx, Duration::from_secs(secs));
        assert!(w.errors.is_empty() && !w.disconnected, "{:?}", w.errors);
        let n = w.samples.len();
        let mut v: Vec<f64> = w.samples.iter().map(|s| s.vbus_v).collect();
        v.sort_by(f64::total_cmp);
        v.get(n.saturating_sub(n / 4 + 1)).copied().unwrap_or(0.0)
    };

    let det = c
        .detect()
        .unwrap_or_else(|e| panic!("detect: {}", e.message));
    eprintln!(
        "detect: bc12={} apple={} entries={} complete={}",
        det.bc12,
        det.apple,
        det.entries.len(),
        det.complete
    );
    let section = c
        .list(TriggerProtocol::Pd)
        .unwrap_or_else(|e| panic!("list pd: {}", e.message));
    assert!(section.supported, "PD is not offered");
    let positions: Vec<u8> = section.offers.iter().map(|o| o.position).collect();
    eprintln!("pd offers at positions {positions:?}");

    // The backend is the final gate: unconfirmed above 5 V is refused.
    assert!(c.request(pd(2), false).is_err(), "unconfirmed 9 V was sent");
    let v = peak(&rx, 2);
    assert!(v < 5.6, "VBUS {v} V after a refused request");

    let mut ladder = vec![(2u8, 9.0)];
    if positions.contains(&8) {
        ladder.push((8, 28.0));
    }
    for (position, want) in ladder {
        let reply = c
            .request(pd(position), true)
            .unwrap_or_else(|e| panic!("request {position}: {}", e.message));
        let v = peak(&rx, 3);
        eprintln!(
            "pd req={position}: ready {:?} mV, stream VBUS {v:.3} V",
            reply.delivered_mv
        );
        assert!((v - want).abs() <= want * 0.05, "{v} V for {want} V");
        c.back_to_5v()
            .unwrap_or_else(|e| panic!("back to 5 V: {}", e.message));
        let v = peak(&rx, 2);
        eprintln!("back to 5 V: stream VBUS {v:.3} V");
        assert!(v < 5.6, "VBUS {v} V after Back to 5 V");
    }

    m.disconnect().expect("disconnect");
    assert!(!m.trigger_open(), "the trigger session is still open");
}
