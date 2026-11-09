//! The VBUS / CC watch (FRs AXHK2N, TFA5N7): a collapse, or both CC lines
//! low, under a contract ends what stood on the charger with no press.

use std::io::{Read, Write};
use std::time::Duration;

use powerz_protocol::session::Clock;

use super::Session;
use crate::types::{ArmedEnd, VbusReading};

/// What times the VBUS watch: monotonic-enough milliseconds.
pub type WatchClock = Box<dyn FnMut() -> u64 + Send>;

/// The latest VBUS reading; `None` when there is none.
pub(crate) type VbusReader = Box<dyn FnMut() -> Option<VbusReading>>;

/// What the VBUS watch knows of the charger (FR AXHK2N).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Powered {
    /// No contract stands: this charger reads 0 V until a PD session starts,
    /// and Detect and resets take VBUS down on purpose, so low means nothing.
    No,
    /// A contract was accepted at this time and no reading of [`VBUS_UP_V`]
    /// came yet. A sample from just before the request must not count as a
    /// collapse, so low only counts once [`VBUS_RISE_WAIT`] has passed.
    Rising(u64),
    /// VBUS rose under the contract: below [`VBUS_GONE_V`] the charger is gone.
    Up,
}

/// A spell of low records in the stream's device time, with its readings for
/// the log line: at most [`LOW_READINGS_MAX`], the latest kept.
pub(super) struct Spell<T> {
    /// The device time of the spell's first record.
    first_ms: Option<u32>,
    readings: std::collections::VecDeque<T>,
}

impl<T> Spell<T> {
    pub(super) fn new() -> Spell<T> {
        Spell {
            first_ms: None,
            readings: std::collections::VecDeque::new(),
        }
    }

    /// The spell, if any, is over: its readings are forgotten.
    pub(super) fn end(&mut self) {
        self.first_ms = None;
        self.readings.clear();
    }

    /// Counts one low record and gives how long the spell has lasted in device
    /// time. A first low record, or the device clock going back, starts a new
    /// spell.
    pub(super) fn push(&mut self, time_ms: u32, reading: T) -> Duration {
        let first = match self.first_ms {
            Some(first) if time_ms >= first => first,
            _ => {
                self.end();
                self.first_ms = Some(time_ms);
                time_ms
            }
        };
        if self.readings.len() >= LOW_READINGS_MAX {
            self.readings.pop_front();
        }
        self.readings.push_back(reading);
        Duration::from_millis(u64::from(time_ms - first))
    }

    /// Ends the spell and gives its readings, each as `show` writes it, joined
    /// with `, `.
    pub(super) fn take(&mut self, show: impl Fn(&T) -> String) -> String {
        self.first_ms = None;
        self.readings
            .drain(..)
            .map(|r| show(&r))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

impl<P: Read + Write, C: Clock> Session<P, C> {
    /// Read VBUS with `read` from here on; a collapse under a contract ends
    /// what stood on the charger with no press. [`Session::check_vbus`] looks.
    pub fn watch_vbus(&mut self, read: impl FnMut() -> Option<VbusReading> + 'static) {
        self.vbus = Some(Box::new(read));
    }

    /// Time the VBUS watch with `now_ms` instead of the session clock.
    pub fn time_watch_by(&mut self, now_ms: WatchClock) {
        self.watch_clock = Some(now_ms);
    }

    /// The VBUS watch's time, ms.
    pub(super) fn watch_now(&mut self) -> u64 {
        match self.watch_clock.as_mut() {
            Some(now) => now(),
            None => self.pdm.now_ms(),
        }
    }
}

impl<P: Read + Write, C: Clock> Session<P, C> {
    /// Ends the current low spell and gives its readings as
    /// `1.95 V, 1.85 V, …`.
    pub(super) fn take_low_spell(&mut self) -> String {
        self.low_spell.take(|v| format!("{v:.2} V"))
    }

    /// The current low spell, if any, is over: its readings are forgotten.
    pub(super) fn end_low_spell(&mut self) {
        self.low_spell.end();
    }

    /// The current low spell, if any, did not count: logged as a dip.
    pub(super) fn ignore_low_spell(&mut self) {
        if !self.low_spell.readings.is_empty() {
            let readings = self.take_low_spell();
            self.log_line(format!("trigger vbus dip ignored: {readings}"));
        }
        self.low_spell.first_ms = None;
    }

    /// A PD request or the way back was accepted: VBUS is watched again. The
    /// reading right now tells whether it has already risen.
    pub(super) fn contract_accepted(&mut self) {
        // A new contract on a charger that was up stays watched as up.
        if self.powered != Powered::Up {
            self.powered = Powered::Rising(self.watch_now());
        }
        // Low readings from before the contract say nothing of it.
        self.end_low_spell();
        self.check_vbus();
    }

    /// One look at VBUS. Nothing is written; a collapse only ends what stood
    /// on the charger.
    pub fn check_vbus(&mut self) {
        let Some(reading) = self.vbus.as_mut().and_then(|read| read()) else {
            // No reading: the spell cannot go on.
            self.ignore_low_spell();
            self.end_cc_spell();
            return;
        };
        let now = self.watch_now();
        // A stream that made no new record for this long has stopped.
        let stopped = |seen: u64| {
            Duration::from_millis(now.saturating_sub(seen)) > reading.period + VBUS_LOW_SPELL
        };
        match self.last_record {
            // The same record again counts once.
            Some((time, seen)) if time == reading.time_ms => {
                if stopped(seen) {
                    self.ignore_low_spell();
                    self.end_cc_spell();
                }
                return;
            }
            Some((_, seen)) if stopped(seen) => {
                self.ignore_low_spell();
                self.end_cc_spell();
            }
            _ => {}
        }
        self.last_record = Some((reading.time_ms, now));
        self.check_cc(&reading);
        if self.powered == Powered::No {
            self.end_low_spell();
            return;
        }
        let volts = reading.volts;
        if volts >= VBUS_GONE_V {
            // One low reading between normal ones is no collapse.
            self.ignore_low_spell();
            if volts >= VBUS_UP_V {
                self.powered = Powered::Up;
            }
            return;
        }
        // Until the rise wait is over a low record may be an old one: no spell
        // starts, so none carries over.
        if let Powered::Rising(since) = self.powered
            && Duration::from_millis(now.saturating_sub(since)) < VBUS_RISE_WAIT
        {
            self.end_low_spell();
            return;
        }
        if self.low_spell.push(reading.time_ms, volts) >= VBUS_LOW_SPELL {
            let readings = self.take_low_spell();
            self.log_line(format!("trigger vbus collapse: {readings}"));
            self.mark_gone();
        }
    }

    /// The charger is gone (a VBUS collapse or both CC lines low): the offers
    /// are forgotten and every request is refused until a PD listing.
    pub(super) fn mark_gone(&mut self) {
        self.powered = Powered::No;
        self.end_low_spell();
        self.end_cc_spell();
        self.gone = true;
        self.offers.clear();
        self.shown.clear();
        self.end_armed(ArmedEnd::ChargerGone);
    }

    /// The current CC spell, if any, is over: its readings are forgotten.
    pub(super) fn end_cc_spell(&mut self) {
        self.cc_spell.end();
    }

    /// One look at the CC lines of a new record (FR TFA5N7): both below
    /// [`CC_GONE_V`] for [`VBUS_LOW_SPELL`] of device time means nothing is
    /// attached, with or without a contract.
    pub(super) fn check_cc(&mut self, reading: &VbusReading) {
        if self.gone {
            self.end_cc_spell();
            return;
        }
        if reading.cc1_v >= CC_GONE_V || reading.cc2_v >= CC_GONE_V {
            self.end_cc_spell();
            return;
        }
        let span = self
            .cc_spell
            .push(reading.time_ms, (reading.cc1_v, reading.cc2_v));
        if span >= VBUS_LOW_SPELL {
            let readings = self
                .cc_spell
                .take(|(cc1, cc2)| format!("CC1 {cc1:.2} V CC2 {cc2:.2} V"));
            self.log_line(format!("trigger cc gone: {readings}"));
            self.mark_gone();
        }
    }
}

/// Below this, V, the charger is gone: PD holds 4.75 V or more across a
/// voltage change, and 0.8 V is its off level.
pub const VBUS_GONE_V: f64 = 2.0;
/// How long VBUS stays below [`VBUS_GONE_V`] before the charger counts as
/// gone, in the records' device time: the meter hands out single low readings
/// between normal ones.
pub const VBUS_LOW_SPELL: Duration = Duration::from_millis(300);
/// Below this, V, on both CC lines nothing is attached: a source pulls one
/// line up to 1.6 V or more with Rp, and an open line reads near 0 V
/// `[inferred]`.
pub const CC_GONE_V: f64 = 0.3;
/// Readings of one low spell kept for its log line.
const LOW_READINGS_MAX: usize = 32;
/// At this, V, VBUS has risen under an accepted contract.
const VBUS_UP_V: f64 = 3.0;
/// How long a low reading after a new contract may still be an old sample.
pub const VBUS_RISE_WAIT: Duration = Duration::from_secs(1);
