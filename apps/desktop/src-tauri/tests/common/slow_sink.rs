//! FR KQF5GM: the evidence and the budget of the slow-sink test at 1000 SPS.
//!
//! [`RecordingClock`] is the system clock with every sleep logged, so a
//! failed run can say where the poll thread lost its time: the longest gaps
//! between queue polls, the largest sleep overrun and the longest work
//! between a wake and the next sleep. Built with a [`BlockedMeter`], it
//! also samples each poll-loop cycle's thread CPU time and the time the
//! worker spent blocked in its own calls, so a long cycle can be told apart
//! as runner descheduling or a stall in the worker (AC-KQF5GM.2).

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use open_powerz_desktop_lib::worker::BlockedMeter;
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

/// What one poll-loop cycle (a wake to the next sleep) cost the worker.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CycleCost {
    /// The poll thread's CPU time; `None` when it cannot be read.
    pub cpu: Option<Duration>,
    /// Wall time blocked inside the worker's own calls.
    pub blocked: Duration,
}

/// One poll-loop cycle: its wall time and what it cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cycle {
    /// From the wake to the next sleep.
    pub wall: Duration,
    /// The poll thread's CPU time; `None` when unknown.
    pub cpu: Option<Duration>,
    /// Wall time blocked inside the worker's own calls.
    pub blocked: Duration,
}

impl Cycle {
    /// The cycle outlasted the poll period and the worker's own time (CPU
    /// plus blocked) does not explain it away: unknown CPU time, or CPU and
    /// blocked time that together outlast the poll period.
    fn is_stall(&self) -> bool {
        self.wall > POLL_PERIOD
            && self
                .cpu
                .is_none_or(|cpu| cpu.saturating_add(self.blocked) > POLL_PERIOD)
    }

    /// Wall time the thread was off the CPU and not blocked in the worker's
    /// own calls: the runner descheduled it. Zero when CPU time is unknown.
    fn descheduled(&self) -> Duration {
        self.cpu.map_or(Duration::ZERO, |cpu| {
            self.wall.saturating_sub(cpu.saturating_add(self.blocked))
        })
    }

    fn describe(&self) -> String {
        let cpu = self
            .cpu
            .map_or_else(|| "unknown".to_owned(), |d| format!("{} ms", d.as_millis()));
        format!(
            "wall {} ms, cpu {cpu}, blocked {} ms",
            self.wall.as_millis(),
            self.blocked.as_millis()
        )
    }
}

/// The sleep log of one run, with each cycle's cost when it was measured.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PollEvidence {
    sleeps: Vec<Sleep>,
    /// `costs[i]` is the cycle from wake `i` to sleep `i + 1`.
    costs: Vec<CycleCost>,
}

impl PollEvidence {
    pub fn new(sleeps: Vec<Sleep>) -> PollEvidence {
        PollEvidence::with_costs(sleeps, Vec::new())
    }

    /// A sleep log with the cost of each cycle between its sleeps; a cycle
    /// without one has unknown CPU time and nothing blocked.
    pub fn with_costs(sleeps: Vec<Sleep>, costs: Vec<CycleCost>) -> PollEvidence {
        PollEvidence { sleeps, costs }
    }

    /// Every wake-to-sleep cycle with its wall, CPU and blocked time.
    pub fn cycles(&self) -> Vec<Cycle> {
        self.sleeps
            .windows(2)
            .enumerate()
            .map(|(i, w)| {
                let cost = self.costs.get(i).copied().unwrap_or_default();
                Cycle {
                    wall: w[1].started.saturating_sub(w[0].woke),
                    cpu: cost.cpu,
                    blocked: cost.blocked,
                }
            })
            .collect()
    }

    /// The cycles that outlasted the poll period with the worker's own CPU
    /// or blocked time long (or CPU time unknown): stalls, not descheduling.
    pub fn stalls(&self) -> Vec<Cycle> {
        self.cycles().into_iter().filter(Cycle::is_stall).collect()
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

    /// The longest time the runner kept the thread off the CPU in a cycle.
    pub fn max_descheduled(&self) -> Duration {
        self.cycles()
            .iter()
            .map(Cycle::descheduled)
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
        let longest = self
            .cycles()
            .into_iter()
            .max_by_key(|c| c.wall)
            .map_or_else(|| "none".to_owned(), |c| c.describe());
        format!(
            "poll-thread evidence over {} sleeps: longest poll gaps [{}], max oversleep {} ms, max work between wake and sleep {} ms, longest cycle {longest}, max descheduled {} ms",
            self.sleeps.len(),
            gaps.join(", "),
            self.max_oversleep().as_millis(),
            self.max_work().as_millis(),
            self.max_descheduled().as_millis()
        )
    }
}

/// The calling thread's CPU time.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn thread_cpu_time() -> Option<Duration> {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `ts` is a valid, writable timespec for the call's duration.
    let rc = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) };
    if rc != 0 {
        return None;
    }
    let secs = u64::try_from(ts.tv_sec).ok()?;
    let nanos = u32::try_from(ts.tv_nsec).ok()?;
    Some(Duration::new(secs, nanos))
}

/// Thread CPU time is read on macOS only.
#[cfg(not(target_os = "macos"))]
fn thread_cpu_time() -> Option<Duration> {
    None
}

/// The shared log of a [`RecordingClock`].
#[derive(Debug, Default)]
struct Log {
    sleeps: Vec<Sleep>,
    costs: Vec<CycleCost>,
    /// CPU time and meter total at the last wake, when metered.
    mark: Option<(Option<Duration>, Duration)>,
}

/// The system clock with every sleep logged. Clones share the log.
#[derive(Debug, Clone)]
pub struct RecordingClock {
    origin: Instant,
    log: Arc<Mutex<Log>>,
    meter: Option<BlockedMeter>,
}

impl RecordingClock {
    pub fn new() -> RecordingClock {
        RecordingClock {
            origin: Instant::now(),
            log: Arc::new(Mutex::new(Log::default())),
            meter: None,
        }
    }

    /// A clock that also samples, per cycle, the sleeping thread's CPU time
    /// and the growth of the worker's blocked-time `meter`.
    pub fn with_meter(meter: BlockedMeter) -> RecordingClock {
        RecordingClock {
            meter: Some(meter),
            ..RecordingClock::new()
        }
    }

    /// A snapshot of the sleeps (and cycle costs) logged so far.
    pub fn evidence(&self) -> PollEvidence {
        self.log
            .lock()
            .map(|l| PollEvidence::with_costs(l.sleeps.clone(), l.costs.clone()))
            .unwrap_or_default()
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
        let before = self.meter.as_ref().map(|m| (thread_cpu_time(), m.total()));
        if let (Some((cpu, blocked)), Ok(mut log)) = (before, self.log.lock())
            && let Some((cpu0, blocked0)) = log.mark.take()
        {
            log.costs.push(CycleCost {
                cpu: cpu.zip(cpu0).map(|(now, then)| now.saturating_sub(then)),
                blocked: blocked.saturating_sub(blocked0),
            });
        }
        std::thread::sleep(duration);
        let woke = self.origin.elapsed();
        let after = self.meter.as_ref().map(|m| (thread_cpu_time(), m.total()));
        if let Ok(mut log) = self.log.lock() {
            log.sleeps.push(Sleep {
                requested: duration,
                started,
                woke,
            });
            log.mark = after;
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

/// A cycle outlasted the poll period with the worker's own CPU or blocked
/// time long, or its CPU time unknown: a stall in the worker, not the runner.
fn stalled(evidence: &PollEvidence) -> bool {
    !evidence.stalls().is_empty()
}

/// The budget for the value of `CI` and the run's sleep log (AC-KQF5GM.2).
///
/// Without `CI` it is [`tolerance`]. On CI, each poll gap that runs past the
/// meter's window because the runner overslept or descheduled the poll
/// thread buys one gap, one hole and the samples past the window (never more
/// than that oversleep plus descheduling). A run with a stalled cycle gets no
/// allowance: that is a stall in the worker, not the runner.
pub fn tolerance_for(ci: Option<&str>, evidence: &PollEvidence) -> Tolerance {
    let strict = tolerance(ci);
    if ci.is_none_or(str::is_empty) || stalled(evidence) {
        return strict;
    }
    let mut gaps = 0;
    let mut lost = 0usize;
    let cycles = evidence.cycles();
    for (w, cycle) in evidence.sleeps().windows(2).zip(&cycles) {
        let gap = w[1].woke.saturating_sub(w[0].woke);
        let oversleep = w[1].woke.saturating_sub(w[1].started + w[1].requested);
        let runner = oversleep.saturating_add(cycle.descheduled());
        let explained = gap.saturating_sub(METER_WINDOW).min(runner);
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
    let stalls = evidence.stalls();
    if lossy && !stalls.is_empty() {
        let cycles: Vec<String> = stalls.iter().map(Cycle::describe).collect();
        breaches.push(format!(
            "the poll loop stalled: {} ms of work between a wake and the next sleep (poll period {} ms); stalled cycles [{}]",
            evidence.max_work().as_millis(),
            POLL_PERIOD.as_millis(),
            cycles.join("; ")
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
