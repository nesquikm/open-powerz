//! FR EHY1JW AC-EHY1JW.4 / .5: the desktop worker ends a stall under the web
//! edition's rule. Mid-stall the meter now and then answers one queue poll
//! with records while another program polls it over CDC `[device]`; the
//! stall ends only once polls with records have run for 1 s of polling, with
//! at most 750 ms of polling between two of them, and one
//! `stall { stalled: false }` goes out per stall. The stall still starts after
//! 2 s of unheld empty polls.
//!
//! Over the simulator on its fake clock, behind a pipe whose queue replies can
//! be switched to the bare `41 id 02 00`, with a switch that lets the next
//! queue poll through (a stray reply with records).

mod common;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::*;
use open_powerz_desktop_lib::worker::{BackendEvent, Worker, WorkerConfig};
use powerz_protocol::decode::Rate;
use powerz_protocol::frame::{self, Attribute};
use powerz_protocol::transport::{self, Transport};

/// The simulator's pipe, with a switch that empties every queue reply, one
/// that lets the next queue poll through, and the times of the queue polls
/// that delivered records.
struct Stalling {
    inner: Pipe,
    clock: SimClock,
    stalled: Arc<AtomicBool>,
    stray: Arc<AtomicBool>,
    record_polls: Arc<Mutex<Vec<u64>>>,
}

fn is_queue_poll(req: &[u8]) -> bool {
    frame::get_data_mask(req).is_some_and(|m| m.bits() & Attribute::ADC_QUEUE.bits() != 0)
}

fn record_count(reply: &[u8]) -> usize {
    if reply.len() <= 4 {
        return 0;
    }
    frame::parse_put_data(reply).map_or(0, |sections| {
        sections
            .iter()
            .filter(|s| s.att == Attribute::ADC_QUEUE)
            .map(|s| s.payload.len() / 20)
            .sum()
    })
}

impl Transport for Stalling {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, transport::Error> {
        let queue = is_queue_poll(request);
        if queue && self.stalled.load(Ordering::SeqCst) && !self.stray.swap(false, Ordering::SeqCst)
        {
            // [device] The meter's queue reply while another program polls ADC
            // snapshots over CDC: a bare PUT_DATA at level 1, nothing new.
            return Ok(vec![0x41, request[1], 0x02, 0x00]);
        }
        let reply = self.inner.transact(request, timeout)?;
        if queue && record_count(&reply) > 0 {
            self.record_polls.lock().unwrap().push(self.clock.now_ms());
        }
        Ok(reply)
    }

    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, transport::Error> {
        self.inner.read(timeout)
    }

    fn settings_pipe(&mut self) -> Option<Result<Box<dyn Transport>, transport::Error>> {
        self.inner.settings_pipe()
    }
}

struct Run {
    worker: Worker<Stalling, SimClock>,
    clock: SimClock,
    stalled: Arc<AtomicBool>,
    stray: Arc<AtomicBool>,
    record_polls: Arc<Mutex<Vec<u64>>>,
    events: Events,
}

fn start(rate: Rate) -> Run {
    let clock = clock();
    let stalled = Arc::new(AtomicBool::new(false));
    let stray = Arc::new(AtomicBool::new(false));
    let record_polls = Arc::new(Mutex::new(Vec::new()));
    let pipe = Stalling {
        inner: Pipe::sim(
            Scenario::Load5v1a,
            FaultPlan::new(),
            &clock,
            PipeOpts::default(),
        ),
        clock: clock.clone(),
        stalled: Arc::clone(&stalled),
        stray: Arc::clone(&stray),
        record_polls: Arc::clone(&record_polls),
    };
    let (events, sink) = capture(&clock);
    let worker = Worker::start(pipe, clock.clone(), WorkerConfig::new(rate, 1), sink)
        .unwrap_or_else(|e| panic!("worker did not start: {e:?}"));
    Run {
        worker,
        clock,
        stalled,
        stray,
        record_polls,
        events,
    }
}

impl Run {
    fn run(&mut self, ms: u64) {
        run_for(&mut self.worker, &self.clock, ms);
    }

    fn stall(&self, on: bool) {
        self.stalled.store(on, Ordering::SeqCst);
    }

    /// The stall events so far, in order, with their simulated times.
    fn stall_events(&self) -> Vec<(u64, bool)> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(t, e)| match e {
                BackendEvent::Stall(s) => Some((*t, s.stalled)),
                _ => None,
            })
            .collect()
    }

    fn stalls(&self) -> Vec<bool> {
        self.stall_events().into_iter().map(|(_, s)| s).collect()
    }

    fn record_polls(&self) -> usize {
        self.record_polls.lock().unwrap().len()
    }

    /// Hand over one stray reply with records every `gap_ms`, `count` times.
    fn strays(&mut self, gap_ms: u64, count: usize) {
        for _ in 0..count {
            let before = self.record_polls();
            self.stray.store(true, Ordering::SeqCst);
            self.run(gap_ms);
            assert!(
                self.record_polls() > before,
                "the stray poll delivered no records"
            );
        }
    }

    /// Stream 1 s, then stall for `ms` of empty queue polls; the stall is on.
    fn stalled_run(rate: Rate, ms: u64) -> Run {
        let mut run = start(rate);
        run.run(1_000);
        assert!(
            !samples(&run.events).is_empty(),
            "no samples before the stall"
        );
        run.stall(true);
        run.run(ms);
        assert_eq!(run.stalls(), vec![true]);
        run
    }
}

#[test]
fn ehy1jw_a_stray_poll_with_records_does_not_end_the_stall() {
    let mut run = Run::stalled_run(Rate::Sps50, 3_000);
    run.strays(1_500, 1);
    assert_eq!(run.stalls(), vec![true], "a stray reply ended the stall");
    run.run(4_000);
    assert_eq!(run.stalls(), vec![true], "the stall flickered");
}

#[test]
fn ehy1jw_strays_in_a_long_stall_never_flicker() {
    let mut run = Run::stalled_run(Rate::Sps50, 3_000);
    for _ in 0..3 {
        run.strays(100, 1);
        run.run(3_000);
    }
    run.run(50_000);
    assert_eq!(run.stalls(), vec![true]);
}

#[test]
fn ehy1jw_strays_600_ms_apart_end_the_stall_once_they_span_one_second() {
    let mut run = Run::stalled_run(Rate::Sps50, 3_000);
    run.strays(600, 2);
    assert_eq!(run.stalls(), vec![true], "ended before the run spanned 1 s");
    run.strays(600, 1);
    assert_eq!(run.stalls(), vec![true, false]);
    run.run(1_000);
    assert_eq!(run.stalls(), vec![true, false], "one stall-ended event");
}

#[test]
fn ehy1jw_single_strays_further_apart_than_750_ms_never_end_the_stall() {
    for gap in [900, 1_600] {
        let mut run = Run::stalled_run(Rate::Sps50, 3_000);
        run.strays(gap, 8);
        assert_eq!(run.stalls(), vec![true], "strays {gap} ms apart ended it");
    }
}

#[test]
fn ehy1jw_a_gap_over_750_ms_restarts_the_run() {
    let mut run = Run::stalled_run(Rate::Sps50, 3_000);
    run.strays(600, 1);
    run.strays(1_600, 1);
    run.strays(600, 1);
    run.strays(1_600, 1);
    assert_eq!(run.stalls(), vec![true]);
}

#[test]
fn ehy1jw_a_recovered_stream_ends_the_stall_within_1_2_s_at_every_rate() {
    for rate in [Rate::Sps2, Rate::Sps10, Rate::Sps50, Rate::Sps1000] {
        let mut run = Run::stalled_run(rate, 8_000);
        let polls_before = run.record_polls();
        run.stall(false);
        run.run(3_000);
        let first = run.record_polls.lock().unwrap().get(polls_before).copied();
        let first = first.unwrap_or_else(|| panic!("{rate:?}: no records after the stall"));
        let events = run.stall_events();
        assert_eq!(
            events.iter().map(|(_, s)| *s).collect::<Vec<_>>(),
            vec![true, false],
            "{rate:?}"
        );
        let after = events[1].0 - first;
        assert!(
            (900..=1_200).contains(&after),
            "{rate:?}: the stall ended {after} ms after the first records"
        );
        run.run(5_000);
        assert_eq!(run.stalls(), vec![true, false], "{rate:?}: more events");
    }
}

#[test]
fn ehy1jw_the_stall_still_starts_after_two_seconds_of_empty_polls() {
    let mut run = start(Rate::Sps50);
    run.run(1_000);
    let began = run.clock.now_ms();
    run.stall(true);
    run.run(1_800);
    assert_eq!(run.stalls(), Vec::<bool>::new(), "a stall before 2 s");
    run.run(1_000);
    let events = run.stall_events();
    assert_eq!(
        events.iter().map(|(_, s)| *s).collect::<Vec<_>>(),
        vec![true]
    );
    let at = events[0].0 - began;
    assert!(
        (2_000..=2_300).contains(&at),
        "the stall started at {at} ms"
    );
}

#[test]
fn ehy1jw_strays_that_keep_the_stall_do_not_send_it_again() {
    let mut run = Run::stalled_run(Rate::Sps50, 3_000);
    for _ in 0..3 {
        run.strays(100, 1);
        run.run(2_500);
    }
    assert_eq!(run.stalls(), vec![true]);
}

#[test]
fn ehy1jw_after_a_recovery_a_new_stall_reports_once() {
    let mut run = Run::stalled_run(Rate::Sps50, 3_000);
    run.stall(false);
    run.run(2_000);
    assert_eq!(run.stalls(), vec![true, false]);
    run.stall(true);
    run.run(1_800);
    assert_eq!(run.stalls(), vec![true, false]);
    run.run(3_000);
    assert_eq!(run.stalls(), vec![true, false, true]);
}
