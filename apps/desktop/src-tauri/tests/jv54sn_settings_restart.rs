//! FR JV54SN: the chart stops drawing after a meter settings write.
//!
//! `[inferred]` A settings write sends every key, the rate 0x15 included, at
//! its read value, and the running queue goes back to the meter's own rate.
//! The simulator's `settings-write-resets-queue-rate` fault plays that
//! meter: its own rate is 10 SPS while the app streams at 50 SPS.
//!
//! AC .1: after the write the chart keeps drawing (at most one closed gap
//! for the write's pause, no gap per sample). AC .2: a successful write on a
//! queue stream is followed by a stop and a start at the session's rate,
//! also while recording; a failed or refused write is not. AC .3: the write
//! still sends key 0x15 at the value read from the meter, and the meter's
//! stored rate is never changed.

mod common;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use common::*;
use open_powerz_desktop_lib::worker::{BackendEvent, Worker, WorkerConfig};
use powerz_protocol::decode::Rate;
use powerz_protocol::decode::settings::WritableField;
use powerz_protocol::session::Session;

type Log = Arc<Mutex<common::Log>>;

struct Rig {
    worker: Worker<Pipe, SimClock>,
    clock: SimClock,
    events: Events,
    bulk: Log,
    cdc: Log,
    /// The same meter's CDC persona, kept to read its stored settings.
    probe: SimDevice,
}

/// A worker streaming at 50 SPS from a simulated meter whose own rate is
/// 10 SPS, under `faults`, after one second of streaming.
fn rig(faults: FaultPlan) -> Rig {
    let clock = clock();
    let dev = SimDevice::with_clock(Persona::Bulk, Scenario::Load5v1a, clock.clone());
    // The meter's own rate: 10 SPS, set while nothing streams, before the
    // faults are in place.
    let mut twin = Session::with_clock(dev.cdc_twin(), clock.clone());
    let set = twin
        .write_settings(&[(WritableField::SampleRate, 1)])
        .expect("set the meter's own rate");
    assert_eq!(set.sample_rate.value, Rate::Sps10);
    let dev = dev.with_faults(faults);
    let probe = dev.cdc_twin();
    let pipe = Pipe::new(dev, &clock, PipeOpts::default());
    let (bulk, cdc) = (Arc::clone(&pipe.log), Arc::clone(&pipe.cdc_log));
    let (events, sink) = capture(&clock);
    let mut worker = Worker::start(pipe, clock.clone(), WorkerConfig::new(Rate::Sps50, 1), sink)
        .unwrap_or_else(|e| panic!("worker did not start: {e:?}"));
    run_for(&mut worker, &clock, 1_000);
    Rig {
        worker,
        clock,
        events,
        bulk,
        cdc,
        probe,
    }
}

fn the_fault() -> FaultPlan {
    FaultPlan::new().settings_write_resets_queue_rate()
}

fn changes(pairs: &[(&str, i32)]) -> BTreeMap<String, i32> {
    pairs.iter().map(|&(k, v)| (k.to_string(), v)).collect()
}

/// Bulk frames sent from index `from` on, as (type, byte 2).
fn bulk_frames_from(rig: &Rig, from: usize) -> Vec<(u8, u8)> {
    rig.bulk.lock().unwrap().frames[from..]
        .iter()
        .map(|(_, f)| (f[0] & 0x7F, f.get(2).copied().unwrap_or(0)))
        .collect()
}

fn bulk_len(rig: &Rig) -> usize {
    rig.bulk.lock().unwrap().frames.len()
}

fn events_len(rig: &Rig) -> usize {
    rig.events.lock().unwrap().len()
}

/// Gap events and sample times emitted from event index `from` on.
fn gaps_and_samples_from(rig: &Rig, from: usize) -> (usize, Vec<u32>) {
    let events = rig.events.lock().unwrap();
    let mut gaps = 0;
    let mut samples = Vec::new();
    for (_, e) in &events[from..] {
        match e {
            BackendEvent::Gap(_) => gaps += 1,
            BackendEvent::Batch(b) => samples.extend(b.samples.iter().map(|s| s.t_ms)),
            _ => {}
        }
    }
    (gaps, samples)
}

/// Whether `frames` hold a queue stop (0x0F) followed by a queue start
/// (0x0E) at `rate`.
fn has_restart_at(frames: &[(u8, u8)], rate: Rate) -> bool {
    let Some(stop) = frames.iter().position(|&(ty, _)| ty == 0x0F) else {
        return false;
    };
    frames[stop..]
        .iter()
        .any(|&(ty, b2)| ty == 0x0E && b2 == rate.code() << 1)
}

/// `(key, value)` entries of a type-0x0A frame.
fn entries(frame: &[u8]) -> Vec<(u16, i32)> {
    frame[4..]
        .chunks(8)
        .map(|e| {
            let key = u32::from_le_bytes([e[0], e[1], e[2], e[3]]) & 0x7FFF;
            (key as u16, i32::from_le_bytes([e[4], e[5], e[6], e[7]]))
        })
        .collect()
}

// ---- AC-JV54SN.1 ----

#[test]
fn jv54sn_the_chart_keeps_drawing_after_a_settings_write() {
    let mut rig = rig(the_fault());
    let (gaps_before, before) = gaps_and_samples_from(&rig, 0);
    assert_eq!(gaps_before, 0, "no gap while streaming normally");
    assert!(
        before.len() >= 40,
        "{} samples in the first second",
        before.len()
    );

    let mark = events_len(&rig);
    rig.worker
        .write_settings(&changes(&[("brightness", 80)]))
        .expect("write");
    run_for(&mut rig.worker, &rig.clock, 3_000);

    let (gaps, samples) = gaps_and_samples_from(&rig, mark);
    assert!(
        gaps <= 1,
        "at most one gap for the write's pause, got {gaps} for {} samples",
        samples.len()
    );
    assert!(
        samples.len() >= 120,
        "50 SPS again after the write: {} samples in 3 s",
        samples.len()
    );
    rig.worker.stop();
}

#[test]
fn jv54sn_two_writes_in_a_row_still_leave_the_chart_drawing() {
    let mut rig = rig(the_fault());
    let mark = events_len(&rig);
    for value in [70, 90] {
        rig.worker
            .write_settings(&changes(&[("brightness", value)]))
            .expect("write");
        run_for(&mut rig.worker, &rig.clock, 1_500);
    }
    let (gaps, samples) = gaps_and_samples_from(&rig, mark);
    assert!(gaps <= 2, "one closed gap per write at most, got {gaps}");
    assert!(samples.len() >= 120, "{} samples in 3 s", samples.len());
    rig.worker.stop();
}

// ---- AC-JV54SN.2 ----

#[test]
fn jv54sn_a_successful_write_restarts_the_stream_at_the_session_rate() {
    let mut rig = rig(the_fault());
    let from = bulk_len(&rig);
    rig.worker
        .write_settings(&changes(&[("brightness", 80)]))
        .expect("write");
    let frames = bulk_frames_from(&rig, from);
    assert!(
        has_restart_at(&frames, Rate::Sps50),
        "a stop and a start at 50 SPS after the write: {frames:02x?}"
    );
    rig.worker.stop();
}

#[test]
fn jv54sn_the_restart_happens_while_recording_too_and_the_recording_goes_on() {
    let mut rig = rig(the_fault());
    rig.worker
        .start_recording(Box::new(std::io::sink()))
        .expect("recording starts");
    let from = bulk_len(&rig);
    rig.worker
        .write_settings(&changes(&[("brightness", 80)]))
        .expect("write");
    let frames = bulk_frames_from(&rig, from);
    assert!(
        has_restart_at(&frames, Rate::Sps50),
        "the meter changed rate anyway, so the stream restarts: {frames:02x?}"
    );
    run_for(&mut rig.worker, &rig.clock, 500);
    assert!(rig.worker.is_recording(), "the recording keeps running");
    rig.worker.stop();
}

#[test]
fn jv54sn_a_failed_write_does_not_restart_the_stream() {
    let mut rig = rig(the_fault().ignore_settings_write());
    let from = bulk_len(&rig);
    assert!(
        rig.worker
            .write_settings(&changes(&[("brightness", 80)]))
            .is_err(),
        "the re-read does not match"
    );
    let frames = bulk_frames_from(&rig, from);
    assert!(
        !frames.iter().any(|&(ty, _)| ty == 0x0F || ty == 0x0E),
        "no restart after a failed write: {frames:02x?}"
    );
    rig.worker.stop();
}

#[test]
fn jv54sn_a_refused_write_does_not_restart_the_stream() {
    let mut rig = rig(the_fault());
    let from = bulk_len(&rig);
    assert!(
        rig.worker
            .write_settings(&changes(&[("brightness", 1_000)]))
            .is_err(),
        "out of range"
    );
    assert!(
        rig.worker
            .write_settings(&changes(&[("voltage_cal", 1)]))
            .is_err(),
        "not writable"
    );
    let frames = bulk_frames_from(&rig, from);
    assert!(
        !frames.iter().any(|&(ty, _)| ty == 0x0F || ty == 0x0E),
        "no restart after a refused write: {frames:02x?}"
    );
    rig.worker.stop();
}

// ---- AC-JV54SN.3 ----

#[test]
fn jv54sn_the_write_sends_the_rate_key_at_its_read_value_and_the_stored_rate_stays() {
    let mut rig = rig(the_fault());
    rig.worker
        .write_settings(&changes(&[("brightness", 80)]))
        .expect("write");
    run_for(&mut rig.worker, &rig.clock, 500);
    let writes: Vec<Vec<u8>> = [&rig.bulk, &rig.cdc]
        .iter()
        .flat_map(|l| l.lock().unwrap().of_type(0x0A))
        .map(|(_, f)| f)
        .collect();
    assert_eq!(writes.len(), 1, "one settings write: {writes:02x?}");
    let e = entries(&writes[0]);
    assert!(
        e.contains(&(0x15, 1)),
        "key 0x15 at the read value (10 SPS = code 1), not the stream's 50 SPS: {e:?}"
    );
    assert!(e.contains(&(0x03, 80)), "{e:?}");
    // The meter's own rate is still 10 SPS.
    let mut probe = Session::open(rig.probe.cdc_twin());
    let stored = probe.read_meter_settings().expect("read");
    assert_eq!(stored.sample_rate.value, Rate::Sps10);
    rig.worker.stop();
}
