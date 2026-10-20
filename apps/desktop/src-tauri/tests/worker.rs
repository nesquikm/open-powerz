//! Session worker against the simulator on its fake clock (FR VHY378).

mod common;

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::*;
use open_powerz_desktop_lib::worker::{
    self, BackendEvent, ConnectedInfo, GapKind, Status, Step, Worker, WorkerConfig,
};
use powerz_protocol::crypto::{self, KEYS};
use powerz_protocol::decode::Rate;
use powerz_protocol::frame::Attribute;
use powerz_protocol::transport::guard::FORBIDDEN_TYPES;

fn config(rate: Rate) -> WorkerConfig {
    WorkerConfig {
        rate,
        session_id: 1,
    }
}

/// Start a worker on a fresh simulator; returns the worker, its clock, the
/// pipe log and the captured events.
fn start(
    rate: Rate,
    scenario: Scenario,
    faults: FaultPlan,
    opts: PipeOpts,
) -> (Worker<Pipe, SimClock>, SimClock, Arc<Mutex<Log>>, Events) {
    let clock = clock();
    let pipe = Pipe::sim(scenario, faults, &clock, opts);
    let log = Arc::clone(&pipe.log);
    let (events, sink) = capture(&clock);
    let worker = Worker::start(pipe, clock.clone(), config(rate), sink)
        .unwrap_or_else(|e| panic!("worker did not start: {e:?}"));
    (worker, clock, log, events)
}

fn connected(events: &Events) -> ConnectedInfo {
    statuses(events)
        .into_iter()
        .find_map(|s| match s {
            Status::Connected(info) => Some(info),
            _ => None,
        })
        .expect("no connected status")
}

fn batches(events: &Events) -> Vec<(u64, open_powerz_desktop_lib::batch::Batch)> {
    events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|(t, e)| match e {
            BackendEvent::Batch(b) => Some((*t, b.clone())),
            _ => None,
        })
        .collect()
}

// ---- AC-VHY378.2: connect, auth level 1, queue at the requested rate ----

#[test]
fn connect_sends_connect_auth_and_starts_the_queue_at_the_requested_rate() {
    let (_worker, _clock, log, events) = start(
        Rate::Sps1000,
        Scenario::Load5v1a,
        FaultPlan::new(),
        PipeOpts::default(),
    );
    let log = log.lock().unwrap();
    let types: Vec<u8> = log.frames.iter().map(|(_, f)| f[0] & 0x7F).collect();
    assert_eq!(
        types.first(),
        Some(&0x02),
        "CONNECT goes first: {types:02x?}"
    );
    let auth = types
        .iter()
        .position(|&t| t == 0x4C)
        .expect("no auth request");
    let start_q = log.of_type(0x0E);
    assert_eq!(start_q.len(), 1, "one queue start");
    assert_eq!(
        start_q[0].1[2] >> 1,
        Rate::Sps1000.code(),
        "queue rate code"
    );
    let start_pos = types.iter().position(|&t| t == 0x0E).unwrap();
    assert!(auth < start_pos, "auth before the queue start");

    let info = connected(&events);
    assert_eq!(info.model, "KM003C");
    assert_eq!(info.firmware_version, "2.0.6");
    assert_eq!(info.serial, SIM_SERIAL);
    assert_eq!(info.rate_sps, Some(1000));
    assert!(!info.rate_locked);
    assert_eq!(info.session_id, 1);
}

#[test]
fn connect_honours_each_supported_rate() {
    for rate in Rate::ALL {
        let (mut worker, clock, log, events) = start(
            rate,
            Scenario::Load5v1a,
            FaultPlan::new(),
            PipeOpts::default(),
        );
        assert_eq!(connected(&events).rate_sps, Some(rate.sps()), "{rate:?}");
        run_for(&mut worker, &clock, 3_000);
        let t = samples(&events);
        assert!(t.len() >= 3, "{rate:?}: too few samples {}", t.len());
        let step = 1000 / rate.sps();
        assert!(
            t.windows(2).all(|w| w[1] - w[0] == step),
            "{rate:?}: samples not {step} ms apart"
        );
        assert_eq!(log.lock().unwrap().of_type(0x0E)[0].1[2] >> 1, rate.code());
    }
}

#[test]
fn stop_stops_the_queue_and_closes_the_pipe() {
    let (mut worker, clock, log, events) = start(
        Rate::Sps50,
        Scenario::Load5v1a,
        FaultPlan::new(),
        PipeOpts::default(),
    );
    run_for(&mut worker, &clock, 1_000);
    worker.stop();
    let log = log.lock().unwrap();
    assert!(log.closed, "the pipe was not closed");
    let types: Vec<u8> = log.frames.iter().map(|(_, f)| f[0] & 0x7F).collect();
    let stop = types
        .iter()
        .rposition(|&t| t == 0x0F)
        .expect("no queue stop");
    assert!(
        types[stop + 1..].iter().all(|&t| t != 0x0C),
        "polled after stopping the queue: {types:02x?}"
    );
    assert!(matches!(
        statuses(&events).last(),
        Some(Status::Disconnected { .. })
    ));
}

#[test]
fn spawn_opens_the_pipe_on_a_dedicated_thread_and_disconnects() {
    let clock = clock();
    let opened_on = Arc::new(Mutex::new(None));
    let log_slot: Arc<Mutex<Option<Arc<Mutex<Log>>>>> = Arc::new(Mutex::new(None));
    let (o, l, c) = (Arc::clone(&opened_on), Arc::clone(&log_slot), clock.clone());
    let open = move || {
        *o.lock().unwrap() = Some(std::thread::current().id());
        let pipe = Pipe::sim(
            Scenario::Load5v1a,
            FaultPlan::new(),
            &c,
            PipeOpts::default(),
        );
        *l.lock().unwrap() = Some(Arc::clone(&pipe.log));
        Ok(pipe)
    };
    let (tx, rx) = mpsc::channel();
    let sink = move |e: BackendEvent| {
        let _ = tx.send(e);
    };
    let handle = worker::spawn(open, clock.clone(), config(Rate::Sps50), sink);
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        match rx.recv_timeout(left) {
            Ok(BackendEvent::Status(Status::Connected(info))) => {
                assert_eq!(info.serial, SIM_SERIAL);
                break;
            }
            Ok(_) => {}
            Err(e) => panic!("no connected status: {e:?}"),
        }
    }
    let thread = opened_on.lock().unwrap().expect("pipe never opened");
    assert_ne!(
        thread,
        std::thread::current().id(),
        "opened on the caller's thread"
    );
    handle.disconnect();
    let log = log_slot.lock().unwrap().clone().unwrap();
    let log = log.lock().unwrap();
    assert!(log.closed, "disconnect did not close the pipe");
    assert!(
        !log.of_type(0x0F).is_empty(),
        "disconnect did not stop the queue"
    );
}

// ---- AC-VHY378.3: rate fallback when auth or the queue start is rejected ----

fn assert_rate_locked_at_meter_rate(opts: PipeOpts) {
    let (mut worker, clock, log, events) =
        start(Rate::Sps1000, Scenario::Load5v1a, FaultPlan::new(), opts);
    assert!(
        connected(&events).rate_locked,
        "connected without rate_locked"
    );
    run_for(&mut worker, &clock, 3_000);
    // The simulator's own rate is 50 SPS (records 20 ms apart).
    let measured = statuses(&events).into_iter().any(|s| {
        matches!(
            s,
            Status::Rate {
                rate_sps: 50,
                rate_locked: true
            }
        )
    });
    assert!(measured, "no rate_locked status with the measured 50 SPS");
    let t = samples(&events);
    assert!(t.len() >= 100, "queue not polled: {} samples", t.len());
    assert!(t.windows(2).all(|w| w[1] - w[0] == 20));

    let sent = log.lock().unwrap().frames.len();
    let err = worker
        .set_rate(Rate::Sps1000)
        .expect_err("set_rate succeeded while the rate is locked");
    assert!(
        err.message
            .to_lowercase()
            .contains("changing the rate needs authentication"),
        "message: {}",
        err.message
    );
    assert_eq!(
        log.lock().unwrap().frames.len(),
        sent,
        "set_rate sent a frame"
    );
}

#[test]
fn rejected_auth_falls_back_to_the_meters_own_rate() {
    assert_rate_locked_at_meter_rate(PipeOpts {
        reject_auth: true,
        ..PipeOpts::default()
    });
}

#[test]
fn rejected_queue_start_falls_back_to_the_meters_own_rate() {
    assert_rate_locked_at_meter_rate(PipeOpts {
        reject_queue_start: true,
        ..PipeOpts::default()
    });
}

#[test]
fn set_rate_changes_the_queue_rate_when_authenticated() {
    let (mut worker, clock, log, events) = start(
        Rate::Sps1000,
        Scenario::Load5v1a,
        FaultPlan::new(),
        PipeOpts::default(),
    );
    run_for(&mut worker, &clock, 1_000);
    worker.set_rate(Rate::Sps50).expect("set_rate failed");
    let n = samples(&events).len();
    run_for(&mut worker, &clock, 2_000);
    let after = &samples(&events)[n..];
    assert!(after.len() >= 50, "too few samples after set_rate");
    // Skip the first few records around the switch.
    assert!(after[5..].windows(2).all(|w| w[1] - w[0] == 20));
    let starts = log.lock().unwrap().of_type(0x0E);
    assert_eq!(starts.last().unwrap().1[2] >> 1, Rate::Sps50.code());
}

// ---- AC-VHY378.4: snapshot polling and temperature ----

#[test]
fn snapshot_is_requested_at_least_once_per_second_between_queue_polls() {
    let (mut worker, clock, log, _events) = start(
        Rate::Sps1000,
        Scenario::Load5v1a,
        FaultPlan::new(),
        PipeOpts::default(),
    );
    let t_start = clock.now_ms();
    run_for(&mut worker, &clock, 10_000);
    let log = log.lock().unwrap();
    assert!(log.max_in_flight <= 1, "two requests in flight");
    let gets: Vec<(u64, u16)> = log
        .of_type(0x0C)
        .into_iter()
        .map(|(t, f)| (t, u16::from_le_bytes([f[2], f[3]]) >> 1))
        .filter(|(t, _)| *t >= t_start)
        .collect();
    let snaps: Vec<u64> = gets
        .iter()
        .filter(|(_, m)| *m == Attribute::ADC.bits())
        .map(|(t, _)| *t)
        .collect();
    assert!(snaps.len() >= 10, "only {} snapshots in 10 s", snaps.len());
    assert!(snaps[0] - t_start <= 1_000, "first snapshot too late");
    assert!(
        snaps.windows(2).all(|w| w[1] - w[0] <= 1_000),
        "snapshots more than 1 s apart: {snaps:?}"
    );
    // One snapshot between queue polls: never two in a row.
    let is_snap: Vec<bool> = gets
        .iter()
        .filter(|(_, m)| *m == Attribute::ADC.bits() || *m == Attribute::ADC_QUEUE.bits())
        .map(|(_, m)| *m == Attribute::ADC.bits())
        .collect();
    assert!(
        !is_snap.windows(2).any(|w| w[0] && w[1]),
        "two snapshots without a queue poll between them"
    );
}

#[test]
fn every_batch_carries_the_latest_temperature() {
    let (mut worker, clock, _log, events) = start(
        Rate::Sps1000,
        Scenario::Load5v1a,
        FaultPlan::new(),
        PipeOpts::default(),
    );
    run_for(&mut worker, &clock, 3_000);
    let b = batches(&events);
    assert!(!b.is_empty());
    for (_, batch) in &b {
        let t = batch.temp_c.expect("batch without temperature");
        // The simulator's chip temperature is 30 ± 2 °C.
        assert!((27.0..=33.0).contains(&t), "temperature {t}");
    }
}

#[test]
fn invalid_temperature_is_null() {
    let opts = PipeOpts {
        invalid_temp: true,
        ..PipeOpts::default()
    };
    let (mut worker, clock, _log, events) =
        start(Rate::Sps1000, Scenario::Load5v1a, FaultPlan::new(), opts);
    run_for(&mut worker, &clock, 3_000);
    let b = batches(&events);
    assert!(!b.is_empty());
    assert!(b.iter().all(|(_, b)| b.temp_c.is_none()));
}

// ---- AC-VHY378.5: batching over the channel ----

#[test]
fn ten_seconds_at_1000_sps_is_at_most_600_batches_with_every_record_once() {
    let (mut worker, clock, log, events) = start(
        Rate::Sps1000,
        Scenario::Load5v1a,
        FaultPlan::new(),
        PipeOpts::default(),
    );
    let t_start = clock.now_ms();
    run_for(&mut worker, &clock, 10_000);
    worker.stop();
    let b = batches(&events);
    assert!(b.len() <= 600, "{} batches", b.len());
    let in_window = b.iter().filter(|(t, _)| *t >= t_start).count();
    assert!(in_window >= 200, "only {in_window} batches: below 20 Hz");
    let times: Vec<u64> = b
        .iter()
        .map(|(t, _)| *t)
        .filter(|t| *t >= t_start)
        .collect();
    assert!(
        times.windows(2).all(|w| w[1] - w[0] <= 50),
        "more than 50 ms between batches"
    );
    let t = samples(&events);
    assert_eq!(
        t.len(),
        log.lock().unwrap().records_delivered,
        "records lost or duplicated"
    );
    assert!(t.len() >= 9_900, "only {} samples", t.len());
    assert!(
        t.windows(2).all(|w| w[1] > w[0]),
        "not in device-time order"
    );
    let gaps = events
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, e)| matches!(e, BackendEvent::Gap(_)))
        .count();
    assert_eq!(gaps, 0);
}

#[test]
fn batches_flow_at_20_to_60_hz_at_a_slow_rate() {
    let (mut worker, clock, _log, events) = start(
        Rate::Sps10,
        Scenario::Load5v1a,
        FaultPlan::new(),
        PipeOpts::default(),
    );
    let t_start = clock.now_ms();
    run_for(&mut worker, &clock, 10_000);
    let n = batches(&events)
        .iter()
        .filter(|(t, _)| *t >= t_start && *t < t_start + 10_000)
        .count();
    assert!((200..=600).contains(&n), "{n} batches in 10 s");
}

#[test]
fn stream_gaps_are_separate_gap_events_in_device_time_order() {
    let opts = PipeOpts {
        stall_at_poll: Some((50, 500)),
        ..PipeOpts::default()
    };
    let (mut worker, clock, log, events) =
        start(Rate::Sps1000, Scenario::Load5v1a, FaultPlan::new(), opts);
    run_for(&mut worker, &clock, 3_000);
    worker.stop();
    let evs = events.lock().unwrap().clone();
    let gap_at = evs
        .iter()
        .position(|(_, e)| matches!(e, BackendEvent::Gap(g) if g.kind == GapKind::Stream))
        .expect("no stream gap event");
    let BackendEvent::Gap(gap) = &evs[gap_at].1 else {
        unreachable!()
    };
    assert!(gap.to_ms > gap.from_ms);
    for (i, (_, e)) in evs.iter().enumerate() {
        if let BackendEvent::Batch(b) = e {
            for s in &b.samples {
                let t = u64::from(s.t_ms);
                if i < gap_at {
                    assert!(t <= gap.from_ms, "sample {t} before the gap event");
                } else {
                    assert!(t >= gap.to_ms, "sample {t} after the gap event");
                }
            }
        }
    }
    let t = samples(&Arc::new(Mutex::new(evs)));
    assert_eq!(t.len(), log.lock().unwrap().records_delivered);
    assert!(t.windows(2).all(|w| w[1] > w[0]));
}

// ---- AC-VHY378.6: unplug mid-stream ----

#[test]
fn unplug_ends_the_session_within_2_s_and_keeps_every_sample() {
    let (mut worker, clock, log, events) = start(
        Rate::Sps1000,
        Scenario::Load5v1a,
        FaultPlan::new().disconnect_after_polls(100),
        PipeOpts::default(),
    );
    assert_eq!(run_for(&mut worker, &clock, 30_000), Step::Ended);
    let unplugged_at = log.lock().unwrap().last_good_poll_ms.unwrap();
    let evs = events.lock().unwrap().clone();
    let (t_disc, _) = evs
        .iter()
        .find(|(_, e)| matches!(e, BackendEvent::Status(Status::Disconnected { .. })))
        .expect("no disconnected status");
    assert!(
        t_disc - unplugged_at <= 2_000,
        "disconnected after {} ms",
        t_disc - unplugged_at
    );
    let t = samples(&events);
    let delivered = log.lock().unwrap().records_delivered;
    assert!(delivered > 0);
    assert_eq!(t.len(), delivered, "samples lost at unplug");
}

// ---- AC-VHY378.8: only safe frames ----

#[test]
fn every_frame_sent_is_in_the_safe_command_set() {
    let (mut worker, clock, log, _events) = start(
        Rate::Sps1000,
        Scenario::Pd9v2a,
        FaultPlan::new(),
        PipeOpts::default(),
    );
    run_for(&mut worker, &clock, 3_000);
    worker.set_rate(Rate::Sps50).unwrap();
    run_for(&mut worker, &clock, 3_000);
    worker.stop();
    let log = log.lock().unwrap();
    assert!(!log.frames.is_empty());
    let allowed_mask =
        Attribute::ADC.bits() | Attribute::ADC_QUEUE.bits() | Attribute::SETTINGS.bits();
    for (_, f) in &log.frames {
        let ty = f[0] & 0x7F;
        assert!(!FORBIDDEN_TYPES.contains(&ty), "forbidden frame {f:02x?}");
        assert!(
            SAFE_TYPES.contains(&ty),
            "frame outside the safe set: {f:02x?}"
        );
        match ty {
            0x0C => {
                let mask = u16::from_le_bytes([f[2], f[3]]) >> 1;
                assert_eq!(mask & !allowed_mask, 0, "GET_DATA mask {mask:#x}");
            }
            0x44 => {
                let plain = crypto::decrypt(&KEYS[0], &f[4..36]);
                let addr = u32::from_le_bytes(plain[0..4].try_into().unwrap());
                let len = u32::from_le_bytes(plain[4..8].try_into().unwrap());
                assert!(
                    ALLOWED_READS.contains(&(addr, len)),
                    "memory read {addr:#x}+{len}"
                );
            }
            _ => {}
        }
    }
    // The rate change and the stop went out too.
    assert!(log.of_type(0x0E).len() >= 2);
    assert!(!log.of_type(0x0F).is_empty());
}

// ---- Review round 1: a failed snapshot is not fatal ----

#[test]
fn a_failed_snapshot_keeps_the_session_and_every_polled_record() {
    let (mut w, clock, log, events) = start(
        Rate::Sps50,
        Scenario::Load5v1a,
        FaultPlan::new(),
        PipeOpts {
            fail_snapshot: Some(2),
            ..PipeOpts::default()
        },
    );
    assert_eq!(
        run_for(&mut w, &clock, 5_000),
        Step::Continue,
        "session ended"
    );
    w.stop();
    assert!(
        !statuses(&events)
            .iter()
            .any(|s| matches!(s, Status::Disconnected { error: Some(_) })),
        "a snapshot hiccup ended the session"
    );
    let delivered = log.lock().unwrap().records_delivered;
    assert_eq!(samples(&events).len(), delivered, "records were dropped");
}
