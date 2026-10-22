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
//!   poll period still fails as a stall;
//! - AC-KQF5GM.3: without CI set, the test requires zero gaps, zero holes and
//!   every delivered record.
//!
//! The helpers live in `tests/common/slow_sink.rs`.

mod common;

use std::time::Duration;

use common::slow_sink::{
    Outcome, PollEvidence, RecordingClock, Sleep, check, tolerance, tolerance_for,
};
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
