//! Running session statistics (FR AH2AEJ).

use powerz_protocol::decode::QueueRecord;
use powerz_record::Integrator;
use serde::Serialize;

/// Min, max and mean of one quantity.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct MinMaxAvg {
    pub min: f64,
    pub max: f64,
    pub avg: f64,
}

/// The statistics sent to the UI.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct StatsSnapshot {
    pub elapsed_ms: u64,
    pub samples: u64,
    pub charge_ah: f64,
    pub energy_wh: f64,
    pub vbus: Option<MinMaxAvg>,
    pub ibus: Option<MinMaxAvg>,
    pub power: Option<MinMaxAvg>,
}

/// A sum with Neumaier compensation, so long sessions keep their averages
/// exact to well under 1e-9.
#[derive(Debug, Clone, Copy, Default)]
struct Sum {
    sum: f64,
    comp: f64,
}

impl Sum {
    fn add(&mut self, x: f64) {
        let t = self.sum + x;
        if self.sum.abs() >= x.abs() {
            self.comp += (self.sum - t) + x;
        } else {
            self.comp += (x - t) + self.sum;
        }
        self.sum = t;
    }

    fn value(&self) -> f64 {
        self.sum + self.comp
    }
}

/// Running min, max and sum of one quantity.
#[derive(Debug, Clone, Copy)]
struct Running {
    min: f64,
    max: f64,
    sum: Sum,
}

impl Default for Running {
    fn default() -> Running {
        Running {
            min: f64::INFINITY,
            max: f64::NEG_INFINITY,
            sum: Sum::default(),
        }
    }
}

impl Running {
    fn push(&mut self, x: f64) {
        self.min = self.min.min(x);
        self.max = self.max.max(x);
        self.sum.add(x);
    }

    fn snapshot(&self, n: u64) -> Option<MinMaxAvg> {
        (n > 0).then(|| MinMaxAvg {
            min: self.min,
            max: self.max,
            avg: self.sum.value() / n as f64,
        })
    }
}

/// Running statistics over the records pushed since the last reset.
#[derive(Debug, Default)]
pub struct Stats {
    integrator: Integrator,
    /// Device time of the first and the latest record, ms.
    first_ms: Option<u32>,
    last_ms: u32,
    samples: u64,
    charge_ah: f64,
    energy_wh: f64,
    vbus: Running,
    ibus: Running,
    power: Running,
}

impl Stats {
    /// Statistics over no records.
    pub fn new() -> Stats {
        Stats::default()
    }

    /// Count one record.
    pub fn push(&mut self, r: &QueueRecord) {
        let (t, v, i) = (r.time_ms(), r.vbus_v(), r.ibus_a());
        (self.charge_ah, self.energy_wh) = self.integrator.push(t, v, i);
        self.first_ms.get_or_insert(t);
        self.last_ms = t;
        self.samples += 1;
        self.vbus.push(v);
        self.ibus.push(i);
        self.power.push(v * i);
    }

    /// The interval up to the next record was not measured (an outage or
    /// a clock jump): it adds no charge or energy.
    pub fn break_run(&mut self) {
        self.integrator.break_run();
    }

    /// Start over from the next record.
    pub fn reset(&mut self) {
        *self = Stats::new();
    }

    /// The statistics so far.
    pub fn snapshot(&self) -> StatsSnapshot {
        let n = self.samples;
        StatsSnapshot {
            elapsed_ms: self
                .first_ms
                .map_or(0, |first| u64::from(self.last_ms.saturating_sub(first))),
            samples: n,
            charge_ah: self.charge_ah,
            energy_wh: self.energy_wh,
            vbus: self.vbus.snapshot(n),
            ibus: self.ibus.snapshot(n),
            power: self.power.snapshot(n),
        }
    }
}
