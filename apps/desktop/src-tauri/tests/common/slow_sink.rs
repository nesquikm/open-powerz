//! FR KQF5GM: the evidence and the budget of the slow-sink test at 1000 SPS.
//!
//! [`RecordingClock`] is the system clock with every sleep logged, so a
//! failed run can say where the poll thread lost its time: the longest gaps
//! between queue polls, the largest sleep overrun and the longest work
//! between a wake and the next sleep.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use powerz_protocol::session::{Clock, SystemClock};

/// One sleep of the poll thread, timed from the clock's creation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sleep {
    /// What the worker asked for.
    pub requested: Duration,
    /// When the sleep began.
    pub started: Duration,
    /// When the thread woke.
    pub woke: Duration,
}

/// The sleep log of one run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PollEvidence {
    sleeps: Vec<Sleep>,
}

impl PollEvidence {
    pub fn new(sleeps: Vec<Sleep>) -> PollEvidence {
        PollEvidence { sleeps }
    }

    pub fn sleeps(&self) -> &[Sleep] {
        &self.sleeps
    }

    /// The `n` longest gaps between consecutive wakes (one queue poll per
    /// wake), longest first.
    pub fn longest_poll_gaps(&self, n: usize) -> Vec<Duration> {
        let mut gaps: Vec<Duration> = self
            .sleeps
            .windows(2)
            .map(|w| w[1].woke.saturating_sub(w[0].woke))
            .collect();
        gaps.sort_unstable_by(|a, b| b.cmp(a));
        gaps.truncate(n);
        gaps
    }

    /// The largest amount by which a wake came after its request.
    pub fn max_oversleep(&self) -> Duration {
        self.sleeps
            .iter()
            .map(|s| s.woke.saturating_sub(s.started + s.requested))
            .max()
            .unwrap_or_default()
    }

    /// The longest time from a wake to the next sleep.
    pub fn max_work(&self) -> Duration {
        self.sleeps
            .windows(2)
            .map(|w| w[1].started.saturating_sub(w[0].woke))
            .max()
            .unwrap_or_default()
    }

    /// The poll-thread evidence in one line.
    pub fn report(&self) -> String {
        let gaps: Vec<String> = self
            .longest_poll_gaps(5)
            .iter()
            .map(|d| format!("{} ms", d.as_millis()))
            .collect();
        format!(
            "poll-thread evidence over {} sleeps: longest poll gaps [{}], max oversleep {} ms, max work between wake and sleep {} ms",
            self.sleeps.len(),
            gaps.join(", "),
            self.max_oversleep().as_millis(),
            self.max_work().as_millis()
        )
    }
}

/// The system clock with every sleep logged. Clones share the log.
#[derive(Debug, Clone)]
pub struct RecordingClock {
    origin: Instant,
    log: Arc<Mutex<Vec<Sleep>>>,
}

impl RecordingClock {
    pub fn new() -> RecordingClock {
        RecordingClock {
            origin: Instant::now(),
            log: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// A snapshot of the sleeps logged so far.
    pub fn evidence(&self) -> PollEvidence {
        let sleeps = self.log.lock().map(|l| l.clone()).unwrap_or_default();
        PollEvidence::new(sleeps)
    }
}

impl Default for RecordingClock {
    fn default() -> Self {
        RecordingClock::new()
    }
}

impl Clock for RecordingClock {
    fn now_ms(&mut self) -> u64 {
        SystemClock.now_ms()
    }

    fn random_u64(&mut self) -> u64 {
        SystemClock.random_u64()
    }

    fn sleep(&mut self, duration: Duration) {
        let started = self.origin.elapsed();
        std::thread::sleep(duration);
        let woke = self.origin.elapsed();
        if let Ok(mut log) = self.log.lock() {
            log.push(Sleep {
                requested: duration,
                started,
                woke,
            });
        }
    }
}

/// What the slow-sink run produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outcome {
    /// Gap events reported.
    pub gaps: usize,
    /// Breaks in the 1 ms sample sequence.
    pub holes: usize,
    /// Samples sent to the sink.
    pub samples: usize,
    /// Queue records the meter delivered.
    pub delivered: usize,
    /// Rows in the session history.
    pub history: usize,
}

/// What the run may get away with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tolerance {
    pub max_gaps: usize,
    pub max_holes: usize,
    /// Every delivered record must reach the sink and the history.
    pub every_record: bool,
    pub min_samples: usize,
}

/// The strict budget: zero gaps, zero holes, every delivered record. The CI
/// allowance lives in [`tolerance_for`].
pub fn tolerance(_ci: Option<&str>) -> Tolerance {
    Tolerance {
        max_gaps: 0,
        max_holes: 0,
        every_record: true,
        min_samples: 9_000,
    }
}

/// How often the worker polls the queue.
pub const POLL_PERIOD: Duration = Duration::from_millis(20);

/// How much of the record stream the meter keeps between polls.
pub const METER_WINDOW: Duration = Duration::from_millis(63);

/// Samples per millisecond at 1000 SPS.
const SAMPLES_PER_MS: u128 = 1;

/// Loop work between a wake and the next sleep outlasted the poll period: a
/// stall in the worker, not the runner.
fn stalled(evidence: &PollEvidence) -> bool {
    evidence.max_work() > POLL_PERIOD
}

/// The budget for the value of `CI` and the run's sleep log (AC-KQF5GM.2).
///
/// Without `CI` it is [`tolerance`]. On CI, each poll gap that runs past the
/// meter's window because the runner overslept buys one gap, one hole and
/// the samples past the window (never more than the oversleep itself). A run
/// whose own loop work outlasts the poll period gets no allowance: that is a
/// stall in the worker, not the runner.
pub fn tolerance_for(ci: Option<&str>, evidence: &PollEvidence) -> Tolerance {
    let strict = tolerance(ci);
    if ci.is_none_or(str::is_empty) || stalled(evidence) {
        return strict;
    }
    let mut gaps = 0;
    let mut lost = 0usize;
    for w in evidence.sleeps().windows(2) {
        let gap = w[1].woke.saturating_sub(w[0].woke);
        let oversleep = w[1].woke.saturating_sub(w[1].started + w[1].requested);
        let explained = gap.saturating_sub(METER_WINDOW).min(oversleep);
        if !explained.is_zero() {
            gaps += 1;
            lost += usize::try_from(explained.as_millis() * SAMPLES_PER_MS).unwrap_or(usize::MAX);
        }
    }
    Tolerance {
        max_gaps: strict.max_gaps + gaps,
        max_holes: strict.max_holes + gaps,
        every_record: true,
        min_samples: strict.min_samples.saturating_sub(lost),
    }
}

/// Check `outcome` against `tolerance`; a failure names every breach plus
/// the poll-thread evidence.
pub fn check(
    outcome: &Outcome,
    tolerance: &Tolerance,
    evidence: &PollEvidence,
) -> Result<(), String> {
    let mut breaches = Vec::new();
    if outcome.gaps > tolerance.max_gaps {
        breaches.push(format!(
            "{} gaps reported (allowed {})",
            outcome.gaps, tolerance.max_gaps
        ));
    }
    if outcome.holes > tolerance.max_holes {
        breaches.push(format!(
            "{} holes (allowed {})",
            outcome.holes, tolerance.max_holes
        ));
    }
    if outcome.samples < tolerance.min_samples {
        breaches.push(format!(
            "only {} samples (need {})",
            outcome.samples, tolerance.min_samples
        ));
    }
    let lossy = outcome.gaps > 0 || outcome.holes > 0 || outcome.samples < outcome.delivered;
    if lossy && stalled(evidence) {
        breaches.push(format!(
            "the poll loop stalled: {} ms of work between a wake and the next sleep (poll period {} ms)",
            evidence.max_work().as_millis(),
            POLL_PERIOD.as_millis()
        ));
    }
    if tolerance.every_record {
        if outcome.samples != outcome.delivered {
            breaches.push(format!(
                "{} samples sent != {} records delivered",
                outcome.samples, outcome.delivered
            ));
        }
        if outcome.history != outcome.delivered {
            breaches.push(format!(
                "{} history rows != {} records delivered",
                outcome.history, outcome.delivered
            ));
        }
    }
    if breaches.is_empty() {
        Ok(())
    } else {
        Err(format!("{}; {}", breaches.join("; "), evidence.report()))
    }
}
