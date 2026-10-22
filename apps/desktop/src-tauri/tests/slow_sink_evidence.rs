//! FR KQF5GM: the evidence and the budget of the slow-sink test at 1000 SPS
//! (`real_meter_fixes.rs`), unit-tested on made-up sleep logs.
//!
//! - AC-KQF5GM.1: a failure reports the poll-thread evidence: the longest
//!   gaps between queue polls, the largest sleep overrun and the longest
//!   work between a wake and the next sleep;
//! - AC-KQF5GM.2: with CI set, gaps, holes and lost samples are tolerated
//!   only as far as the measured oversleep explains them (the meter keeps
//!   63 ms of records, the worker polls every 20 ms); every delivered record
//!   must still reach the sink and the history, and loop work longer than the
//!   poll period still fails as a stall; every poll-loop cycle carries its
//!   wall time, the poll thread's CPU time and the time blocked inside the
//!   worker's own calls, and a long cycle is runner descheduling only when
//!   both of those are short (otherwise, or when CPU time is unknown, it is
//!   a stall);
//! - AC-KQF5GM.3: without CI set, the test requires zero gaps, zero holes and
//!   every delivered record.
//!
//! The helpers live in `tests/common/slow_sink.rs`.

mod common;

use std::time::Duration;

use common::slow_sink::{
    Cycle, CycleCost, Outcome, PollEvidence, RecordingClock, Sleep, check, tolerance, tolerance_for,
};
use open_powerz_desktop_lib::worker::BlockedMeter;
use powerz_protocol::session::Clock;

fn ms(v: u64) -> Duration {
    Duration::from_millis(v)
}

/// A sleep of `requested` ms that started at `started` ms and woke at `woke` ms.
fn sleep(requested: u64, started: u64, woke: u64) -> Sleep {
    Sleep {
        requested: ms(requested),
        started: ms(started),
        woke: ms(woke),
    }
}

/// A 20 ms poll loop: 2 ms of work per poll, one 137 ms oversleep and one
/// 61 ms stall in the loop's own work.
fn sample_evidence() -> PollEvidence {
    PollEvidence::new(vec![
        sleep(20, 0, 20),    // woke at 20
        sleep(20, 22, 42),   // work 2, gap 22
        sleep(20, 44, 201),  // oversleep 137, gap 159
        sleep(20, 203, 223), // work 2, gap 22
        sleep(20, 284, 304), // work 61, gap 81
        sleep(20, 306, 326), // work 2, gap 22
    ])
}

/// A clean run: every sample, no gap, no hole.
fn clean_outcome() -> Outcome {
    Outcome {
        gaps: 0,
        holes: 0,
        samples: 10_000,
        delivered: 10_000,
        history: 10_000,
    }
}

// ---- AC-KQF5GM.1: the poll-thread evidence ----

#[test]
fn the_longest_poll_gaps_come_first() {
    let gaps = sample_evidence().longest_poll_gaps(3);
    assert_eq!(gaps, vec![ms(159), ms(81), ms(22)]);
}

#[test]
fn the_largest_oversleep_is_the_wake_past_the_request() {
    assert_eq!(sample_evidence().max_oversleep(), ms(137));
}

#[test]
fn the_longest_work_is_from_a_wake_to_the_next_sleep() {
    assert_eq!(sample_evidence().max_work(), ms(61));
}

#[test]
fn an_early_wake_is_no_oversleep() {
    let e = PollEvidence::new(vec![sleep(20, 0, 15), sleep(20, 16, 36)]);
    assert_eq!(e.max_oversleep(), ms(0));
    assert_eq!(e.max_work(), ms(1));
}

#[test]
fn the_report_names_every_piece_of_evidence() {
    let report = sample_evidence().report();
    let lower = report.to_lowercase();
    assert!(lower.contains("poll gap"), "no poll gaps in: {report}");
    assert!(lower.contains("oversleep"), "no oversleep in: {report}");
    assert!(lower.contains("work"), "no work time in: {report}");
    for figure in ["159", "81", "137", "61"] {
        assert!(
            report.contains(figure),
            "{figure} ms missing from: {report}"
        );
    }
}

#[test]
fn a_failed_check_carries_the_evidence() {
    let mut outcome = clean_outcome();
    outcome.gaps = 75;
    let message = check(&outcome, &tolerance(None), &sample_evidence())
        .expect_err("75 gaps must fail the strict check");
    assert!(
        message.contains("75"),
        "the gap count is missing: {message}"
    );
    assert!(
        message.contains(&sample_evidence().report()),
        "the poll-thread evidence is missing: {message}"
    );
}

#[test]
fn a_hole_fails_with_the_evidence_too() {
    let mut outcome = clean_outcome();
    outcome.holes = 3;
    let message = check(&outcome, &tolerance(None), &sample_evidence())
        .expect_err("3 holes must fail the strict check");
    assert!(
        message.contains(&sample_evidence().report()),
        "the poll-thread evidence is missing: {message}"
    );
}

#[test]
fn the_recording_clock_logs_each_sleep_against_its_request() {
    let mut clock = RecordingClock::new();
    clock.sleep(ms(5));
    clock.sleep(ms(5));
    let sleeps = clock.evidence().sleeps().to_vec();
    assert_eq!(sleeps.len(), 2, "two sleeps logged: {sleeps:?}");
    for s in &sleeps {
        assert_eq!(s.requested, ms(5));
        assert!(
            s.woke >= s.started + ms(5),
            "woke before the request: {s:?}"
        );
    }
    assert!(
        sleeps[1].started >= sleeps[0].woke,
        "out of order: {sleeps:?}"
    );
    // A clone shares the log: the worker's copy fills the test's evidence.
    let mut copy = clock.clone();
    copy.sleep(ms(1));
    assert_eq!(clock.evidence().sleeps().len(), 3);
    // Wall-clock time is the system's.
    let now = clock.now_ms();
    assert!(now > 1_700_000_000_000, "not the system time: {now}");
}

// ---- AC-KQF5GM.3: without CI, the strict budget ----

#[test]
fn without_ci_the_budget_is_strict() {
    let t = tolerance(None);
    assert_eq!(t.max_gaps, 0, "gaps allowed without CI");
    assert_eq!(t.max_holes, 0, "holes allowed without CI");
    assert!(t.every_record, "lost records allowed without CI");
    assert_eq!(t.min_samples, 9_000, "fewer than 9000 samples in 10 s");
}

#[test]
fn an_empty_ci_counts_as_unset() {
    let t = tolerance(Some(""));
    assert_eq!(t.max_gaps, 0);
    assert_eq!(t.max_holes, 0);
    assert!(t.every_record);
}

#[test]
fn a_clean_run_passes_the_strict_check() {
    assert_eq!(
        check(&clean_outcome(), &tolerance(None), &sample_evidence()),
        Ok(())
    );
}

#[test]
fn the_strict_check_fails_one_gap() {
    let mut outcome = clean_outcome();
    outcome.gaps = 1;
    assert!(check(&outcome, &tolerance(None), &sample_evidence()).is_err());
}

#[test]
fn the_strict_check_fails_one_hole() {
    let mut outcome = clean_outcome();
    outcome.holes = 1;
    assert!(check(&outcome, &tolerance(None), &sample_evidence()).is_err());
}

#[test]
fn the_strict_check_fails_a_lost_record() {
    let mut outcome = clean_outcome();
    outcome.samples = 9_999;
    let message = check(&outcome, &tolerance(None), &sample_evidence())
        .expect_err("a sample short of the delivered records must fail");
    assert!(
        message.contains("9999"),
        "the sample count is missing: {message}"
    );

    let mut outcome = clean_outcome();
    outcome.history = 9_998;
    assert!(
        check(&outcome, &tolerance(None), &sample_evidence()).is_err(),
        "history short of the delivered records must fail"
    );
}

#[test]
fn the_strict_check_fails_a_short_run() {
    let outcome = Outcome {
        gaps: 0,
        holes: 0,
        samples: 8_999,
        delivered: 8_999,
        history: 8_999,
    };
    assert!(check(&outcome, &tolerance(None), &sample_evidence()).is_err());
}

// ---- AC-KQF5GM.2: on CI, a tolerance derived from the measured oversleep ----

/// A 20 ms poll loop on a runner that oversleeps twice: 2 ms of work per
/// poll, a 137 ms oversleep (poll gap 159 ms, 96 ms past the meter's 63 ms of
/// records) and a 102 ms oversleep (poll gap 124 ms, 61 ms past it).
fn oversleep_evidence() -> PollEvidence {
    PollEvidence::new(vec![
        sleep(20, 0, 20),    // woke at 20
        sleep(20, 22, 42),   // gap 22
        sleep(20, 44, 201),  // oversleep 137, gap 159
        sleep(20, 203, 223), // gap 22
        sleep(20, 225, 347), // oversleep 102, gap 124
        sleep(20, 349, 369), // gap 22
    ])
}

/// The same oversleeps, plus one 30 ms stretch of loop work: longer than the
/// 20 ms poll period.
fn stalled_evidence() -> PollEvidence {
    PollEvidence::new(vec![
        sleep(20, 0, 20),
        sleep(20, 22, 42),
        sleep(20, 44, 201),  // oversleep 137, gap 159
        sleep(20, 231, 251), // work 30: a stall
        sleep(20, 253, 375), // oversleep 102, gap 124
        sleep(20, 377, 397),
    ])
}

/// Samples the two oversleeps cost: 96 + 61 ms at 1000 SPS.
const OVERSLEEP_LOSS: usize = 96 + 61;

/// A CI run that lost exactly what the two oversleeps explain: two gaps, two
/// holes, and every delivered record in the sink and the history.
fn oversleep_outcome() -> Outcome {
    let n = 9_000 - OVERSLEEP_LOSS;
    Outcome {
        gaps: 2,
        holes: 2,
        samples: n,
        delivered: n,
        history: n,
    }
}

#[test]
fn on_ci_the_budget_comes_from_the_measured_oversleep() {
    let t = tolerance_for(Some("true"), &oversleep_evidence());
    assert_eq!(t.max_gaps, 2, "one gap per poll gap the oversleep explains");
    assert_eq!(
        t.max_holes, 2,
        "one hole per poll gap the oversleep explains"
    );
    assert!(
        t.every_record,
        "CI must still require every delivered record"
    );
    assert_eq!(
        t.min_samples,
        9_000 - OVERSLEEP_LOSS,
        "the sample floor drops by the records the oversleeps cost"
    );
}

#[test]
fn a_larger_oversleep_buys_a_proportionally_larger_allowance() {
    // One oversleep of 237 ms: poll gap 259 ms, 196 ms past the window.
    let e = PollEvidence::new(vec![
        sleep(20, 0, 20),
        sleep(20, 22, 279), // oversleep 237, gap 259
        sleep(20, 281, 301),
    ]);
    let t = tolerance_for(Some("1"), &e);
    assert_eq!(t.max_gaps, 1);
    assert_eq!(t.max_holes, 1);
    assert_eq!(t.min_samples, 9_000 - 196);
    assert!(t.every_record);
}

#[test]
fn on_ci_an_oversleep_inside_the_window_buys_nothing() {
    // Oversleeps of 30 ms: poll gaps of 52 ms, inside the 63 ms of records.
    let e = PollEvidence::new(vec![
        sleep(20, 0, 20),
        sleep(20, 22, 72),   // oversleep 30, gap 52
        sleep(20, 74, 124),  // oversleep 30, gap 52
        sleep(20, 126, 146), // gap 22
    ]);
    assert_eq!(tolerance_for(Some("true"), &e), tolerance(None));
}

#[test]
fn on_ci_without_a_sleep_log_the_budget_stays_strict() {
    assert_eq!(
        tolerance_for(Some("true"), &PollEvidence::default()),
        tolerance(None)
    );
    assert_eq!(tolerance(Some("true")), tolerance(None));
}

#[test]
fn without_ci_the_oversleep_buys_nothing() {
    assert_eq!(tolerance_for(None, &oversleep_evidence()), tolerance(None));
    assert_eq!(
        tolerance_for(Some(""), &oversleep_evidence()),
        tolerance(None),
        "an empty CI counts as unset"
    );
    let e = oversleep_evidence();
    assert!(
        check(&oversleep_outcome(), &tolerance_for(None, &e), &e).is_err(),
        "without CI two gaps must fail, whatever the oversleep"
    );
}

#[test]
fn on_ci_a_run_the_oversleep_explains_passes() {
    let e = oversleep_evidence();
    assert_eq!(
        check(&oversleep_outcome(), &tolerance_for(Some("true"), &e), &e),
        Ok(())
    );
}

#[test]
fn on_ci_a_gap_with_no_matching_oversleep_fails() {
    let e = oversleep_evidence();
    let mut outcome = oversleep_outcome();
    outcome.gaps = 3;
    let message = check(&outcome, &tolerance_for(Some("true"), &e), &e)
        .expect_err("a third gap has no oversleep behind it");
    assert!(
        message.contains(&e.report()),
        "the poll-thread evidence is missing: {message}"
    );

    let mut outcome = oversleep_outcome();
    outcome.holes = 3;
    assert!(
        check(&outcome, &tolerance_for(Some("true"), &e), &e).is_err(),
        "a third hole has no oversleep behind it"
    );
}

#[test]
fn on_ci_a_loss_beyond_the_oversleep_fails() {
    let e = oversleep_evidence();
    let n = 9_000 - OVERSLEEP_LOSS - 1;
    let outcome = Outcome {
        samples: n,
        delivered: n,
        history: n,
        ..oversleep_outcome()
    };
    let message = check(&outcome, &tolerance_for(Some("true"), &e), &e)
        .expect_err("one sample more lost than the oversleeps explain");
    assert!(
        message.contains(&n.to_string()),
        "the sample count is missing: {message}"
    );
}

#[test]
fn on_ci_every_delivered_record_must_still_reach_the_sink_and_the_history() {
    let e = oversleep_evidence();
    let t = tolerance_for(Some("true"), &e);

    let mut outcome = oversleep_outcome();
    outcome.samples -= 1;
    let message = check(&outcome, &t, &e).expect_err("a delivered record missing from the sink");
    assert!(
        message.contains(&outcome.samples.to_string()),
        "the sample count is missing: {message}"
    );

    let mut outcome = oversleep_outcome();
    outcome.history -= 1;
    assert!(
        check(&outcome, &t, &e).is_err(),
        "a delivered record missing from the history"
    );
}

#[test]
fn on_ci_loop_work_longer_than_the_poll_period_fails_as_a_stall() {
    let e = stalled_evidence();
    assert_eq!(e.max_work(), ms(30), "the fixture's stall");
    assert_eq!(e.max_oversleep(), ms(137), "the fixture's oversleep");
    let message = check(&oversleep_outcome(), &tolerance_for(Some("true"), &e), &e)
        .expect_err("gaps in a run whose loop stalled must fail");
    assert!(
        message.to_lowercase().contains("stall"),
        "the failure does not name the stall: {message}"
    );
    assert!(
        message.contains(&e.report()),
        "the poll-thread evidence is missing: {message}"
    );
}

// ---- AC-KQF5GM.2: per-cycle wall, CPU and blocked time ----

/// What one cycle cost: `cpu` ms of poll-thread CPU time (`None`: unknown)
/// and `blocked` ms inside the worker's own calls.
fn cost(cpu: Option<u64>, blocked: u64) -> CycleCost {
    CycleCost {
        cpu: cpu.map(ms),
        blocked: ms(blocked),
    }
}

/// A 20 ms poll loop with one 61 ms cycle (wake at 42, next sleep at 103,
/// poll gap 81 ms, 18 ms past the meter's 63 ms of records) and no
/// oversleep, the shape of CI run 39759034254. `long` is that cycle's cost;
/// every other cycle costs 1 ms of CPU and nothing blocked.
fn long_cycle_evidence(long: CycleCost) -> PollEvidence {
    PollEvidence::with_costs(
        vec![
            sleep(20, 0, 20),
            sleep(20, 22, 42),   // work 2, gap 22
            sleep(20, 103, 123), // work 61, gap 81
            sleep(20, 125, 145), // work 2, gap 22
        ],
        vec![cost(Some(1), 0), long, cost(Some(1), 0)],
    )
}

/// The long cycle with 2 ms of CPU and 1 ms blocked: the runner took the
/// thread off the CPU for the other 58 ms.
fn descheduled_evidence() -> PollEvidence {
    long_cycle_evidence(cost(Some(2), 1))
}

/// The long cycle spent 55 ms on the CPU: the worker's own work.
fn cpu_stall_evidence() -> PollEvidence {
    long_cycle_evidence(cost(Some(55), 6))
}

/// The long cycle spent 53 ms blocked in the worker's own calls.
fn blocked_stall_evidence() -> PollEvidence {
    long_cycle_evidence(cost(Some(4), 53))
}

/// The long cycle on a platform where CPU time cannot be read.
fn unknown_cpu_evidence() -> PollEvidence {
    long_cycle_evidence(cost(None, 1))
}

/// Samples the descheduled cycle cost: its poll gap ran 18 ms past the window.
const DESCHEDULED_LOSS: usize = 81 - 63;

/// A CI run that lost exactly what the descheduling explains.
fn descheduled_outcome() -> Outcome {
    let n = 9_000 - DESCHEDULED_LOSS;
    Outcome {
        gaps: 1,
        holes: 1,
        samples: n,
        delivered: n,
        history: n,
    }
}

#[test]
fn every_cycle_holds_its_wall_cpu_and_blocked_time() {
    let cycles = cpu_stall_evidence().cycles();
    assert_eq!(
        cycles,
        vec![
            Cycle {
                wall: ms(2),
                cpu: Some(ms(1)),
                blocked: ms(0),
            },
            Cycle {
                wall: ms(61),
                cpu: Some(ms(55)),
                blocked: ms(6),
            },
            Cycle {
                wall: ms(2),
                cpu: Some(ms(1)),
                blocked: ms(0),
            },
        ],
        "one cycle per wake-to-sleep stretch, with its wall time from the sleep log"
    );
}

#[test]
fn a_sleep_log_alone_has_unknown_cpu_and_nothing_blocked() {
    let cycles = sample_evidence().cycles();
    let walls: Vec<Duration> = cycles.iter().map(|c| c.wall).collect();
    assert_eq!(walls, vec![ms(2), ms(2), ms(2), ms(61), ms(2)]);
    assert!(
        cycles
            .iter()
            .all(|c| c.cpu.is_none() && c.blocked.is_zero()),
        "no CPU or blocked time was measured: {cycles:?}"
    );
}

#[test]
fn the_report_names_the_long_cycles_wall_cpu_and_blocked_time() {
    let report = blocked_stall_evidence().report();
    let lower = report.to_lowercase();
    assert!(lower.contains("cpu"), "no CPU time in: {report}");
    assert!(lower.contains("blocked"), "no blocked time in: {report}");
    for figure in ["61 ms", "4 ms", "53 ms"] {
        assert!(report.contains(figure), "{figure} missing from: {report}");
    }
    let report = cpu_stall_evidence().report();
    for figure in ["61 ms", "55 ms", "6 ms"] {
        assert!(report.contains(figure), "{figure} missing from: {report}");
    }
}

#[test]
fn the_report_says_when_cpu_time_is_unknown() {
    let report = unknown_cpu_evidence().report();
    let lower = report.to_lowercase();
    assert!(lower.contains("cpu"), "no CPU time in: {report}");
    assert!(
        lower.contains("unknown"),
        "an unread CPU time must read as unknown: {report}"
    );
}

#[test]
fn a_long_cycle_with_short_cpu_and_short_blocked_time_is_descheduling() {
    assert_eq!(
        descheduled_evidence().stalls(),
        Vec::<Cycle>::new(),
        "the runner, not the worker, lost those 58 ms"
    );
}

#[test]
fn a_short_cycle_is_never_a_stall() {
    // 15 ms of CPU inside the 20 ms poll period: busy, not stalled.
    let e = PollEvidence::with_costs(
        vec![sleep(20, 0, 20), sleep(20, 35, 55), sleep(20, 57, 77)],
        vec![cost(Some(15), 0), cost(None, 0)],
    );
    assert_eq!(e.stalls(), Vec::<Cycle>::new());
}

#[test]
fn a_long_cycle_with_long_cpu_time_is_a_stall() {
    assert_eq!(
        cpu_stall_evidence().stalls(),
        vec![Cycle {
            wall: ms(61),
            cpu: Some(ms(55)),
            blocked: ms(6),
        }]
    );
}

#[test]
fn a_long_cycle_with_long_blocked_time_is_a_stall() {
    assert_eq!(
        blocked_stall_evidence().stalls(),
        vec![Cycle {
            wall: ms(61),
            cpu: Some(ms(4)),
            blocked: ms(53),
        }]
    );
}

#[test]
fn a_long_cycle_with_unknown_cpu_time_is_a_stall() {
    assert_eq!(
        unknown_cpu_evidence().stalls(),
        vec![Cycle {
            wall: ms(61),
            cpu: None,
            blocked: ms(1),
        }]
    );
}

#[test]
fn on_ci_descheduling_buys_what_it_explains() {
    let e = descheduled_evidence();
    let t = tolerance_for(Some("true"), &e);
    assert_eq!(t.max_gaps, 1, "one gap for the descheduled poll gap");
    assert_eq!(t.max_holes, 1, "one hole for the descheduled poll gap");
    assert!(
        t.every_record,
        "CI must still require every delivered record"
    );
    assert_eq!(
        t.min_samples,
        9_000 - DESCHEDULED_LOSS,
        "the floor drops by the 18 ms the poll gap ran past the window"
    );
    assert_eq!(check(&descheduled_outcome(), &t, &e), Ok(()));
}

#[test]
fn without_ci_descheduling_buys_nothing() {
    let e = descheduled_evidence();
    assert_eq!(tolerance_for(None, &e), tolerance(None));
    assert!(check(&descheduled_outcome(), &tolerance_for(None, &e), &e).is_err());
}

#[test]
fn on_ci_a_loss_beyond_the_descheduling_fails() {
    let e = descheduled_evidence();
    let t = tolerance_for(Some("true"), &e);
    let mut outcome = descheduled_outcome();
    outcome.gaps = 2;
    assert!(
        check(&outcome, &t, &e).is_err(),
        "a second gap is unexplained"
    );
    let n = 9_000 - DESCHEDULED_LOSS - 1;
    let outcome = Outcome {
        samples: n,
        delivered: n,
        history: n,
        ..descheduled_outcome()
    };
    assert!(check(&outcome, &t, &e).is_err(), "one sample too many lost");
}

/// On CI, a run with `e`'s stall gets the strict budget, and its lossy
/// outcome fails naming the stall and the cycle's wall, CPU and blocked time.
fn assert_fails_as_a_stall(e: &PollEvidence, figures: &[&str]) {
    let t = tolerance_for(Some("true"), e);
    assert_eq!(t, tolerance(None), "a stall buys no allowance");
    let message =
        check(&descheduled_outcome(), &t, e).expect_err("a lossy run whose loop stalled must fail");
    let lower = message.to_lowercase();
    assert!(
        lower.contains("stall"),
        "the failure does not name the stall: {message}"
    );
    assert!(lower.contains("cpu"), "no CPU time in: {message}");
    assert!(lower.contains("blocked"), "no blocked time in: {message}");
    for figure in figures {
        assert!(message.contains(figure), "{figure} missing from: {message}");
    }
}

#[test]
fn on_ci_a_cpu_stall_fails_with_all_three_numbers() {
    assert_fails_as_a_stall(&cpu_stall_evidence(), &["61 ms", "55 ms", "6 ms"]);
}

#[test]
fn on_ci_a_blocked_stall_fails_with_all_three_numbers() {
    assert_fails_as_a_stall(&blocked_stall_evidence(), &["61 ms", "4 ms", "53 ms"]);
}

#[test]
fn on_ci_a_long_cycle_with_unknown_cpu_fails_as_a_stall() {
    let e = unknown_cpu_evidence();
    assert_fails_as_a_stall(&e, &["61 ms"]);
    let message = check(&descheduled_outcome(), &tolerance_for(Some("true"), &e), &e)
        .expect_err("unknown CPU time cannot clear a long cycle");
    assert!(message.to_lowercase().contains("unknown"), "{message}");
}

/// Spin on the CPU for `d`.
fn burn(d: Duration) {
    let start = std::time::Instant::now();
    let mut x = 0u64;
    while start.elapsed() < d {
        x = std::hint::black_box(x.wrapping_add(1));
    }
}

#[test]
fn the_recording_clock_samples_cpu_and_blocked_time_per_cycle() {
    let meter = BlockedMeter::new();
    let mut clock = RecordingClock::with_meter(meter.clone());
    clock.sleep(ms(2));
    // Cycle 1: 40 ms on the CPU, plus 15 ms the worker spent blocked.
    burn(ms(40));
    meter.add(ms(15));
    clock.sleep(ms(2));
    // Cycle 2: 40 ms off the CPU, nothing blocked (a descheduled thread).
    std::thread::sleep(ms(40));
    clock.sleep(ms(2));

    let cycles = clock.evidence().cycles();
    assert_eq!(cycles.len(), 2, "two wake-to-sleep cycles: {cycles:?}");
    assert!(cycles[0].wall >= ms(40), "{cycles:?}");
    assert_eq!(cycles[0].blocked, ms(15), "the meter's growth in cycle 1");
    assert!(cycles[1].wall >= ms(40), "{cycles:?}");
    assert_eq!(cycles[1].blocked, ms(0), "nothing blocked in cycle 2");
    if cfg!(target_os = "macos") {
        let busy = cycles[0].cpu.expect("thread CPU time is readable on macOS");
        let idle = cycles[1].cpu.expect("thread CPU time is readable on macOS");
        assert!(busy >= ms(30), "40 ms of spinning read as {busy:?} of CPU");
        assert!(idle < ms(10), "40 ms of sleeping read as {idle:?} of CPU");
    }
}

#[test]
fn a_blocked_meter_is_shared_by_its_clones() {
    let meter = BlockedMeter::new();
    assert_eq!(meter.total(), ms(0));
    let copy = meter.clone();
    copy.add(ms(7));
    meter.add(ms(5));
    assert_eq!(meter.total(), ms(12));
    assert_eq!(copy.total(), ms(12));
    assert_eq!(BlockedMeter::default().total(), ms(0));
}
