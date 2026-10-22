//! FR KQF5GM: the evidence and the budget of the slow-sink test at 1000 SPS
//! (`real_meter_fixes.rs`), unit-tested on made-up sleep logs.
//!
//! - AC-KQF5GM.1: a failure reports the poll-thread evidence: the longest
//!   gaps between queue polls, the largest sleep overrun and the longest
//!   work between a wake and the next sleep;
//! - AC-KQF5GM.3: without CI set, the test requires zero gaps, zero holes and
//!   every delivered record.
//!
//! The helpers live in `tests/common/slow_sink.rs`.

mod common;

use std::time::Duration;

use common::slow_sink::{Outcome, PollEvidence, RecordingClock, Sleep, check, tolerance};
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
