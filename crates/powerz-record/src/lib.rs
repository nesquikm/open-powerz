//! Vendor-compatible CSV recording shared by `powerz-cli` and the desktop
//! app (FR 0P7HSJ): the recorder header, elapsed-time and plain-decimal
//! formatting, charge/energy integration and [`CsvRecorder`], which writes
//! queue records as CSV rows to any [`Write`].

use std::io::{self, Write};

use powerz_protocol::decode::QueueRecord;

/// Header of the recorder CSV, matching the vendor recorder.
pub const CSV_HEADER: &str = "ElapsedTime,Unix,VBUS,IBUS,DP,DM,CC1,CC2,TEMP,CHARGE,ENERGY,PWR";

/// Format elapsed milliseconds as `h:mm:ss.mmm` (hours unpadded).
pub fn format_elapsed(elapsed_ms: u64) -> String {
    let ms = elapsed_ms % 1000;
    let s = elapsed_ms / 1000;
    format!("{}:{:02}:{:02}.{ms:03}", s / 3600, s / 60 % 60, s % 60)
}

/// Running charge (Ah) and energy (Wh) integrated from signed current.
#[derive(Debug, Clone, Default)]
pub struct Integrator {
    /// Previous sample: `(time_ms, vbus_v, ibus_a)`.
    last: Option<(u32, f64, f64)>,
    charge_ah: f64,
    energy_wh: f64,
}

impl Integrator {
    /// A fresh integrator at 0 Ah / 0 Wh.
    pub fn new() -> Integrator {
        Integrator::default()
    }

    /// Add the sample at device time `time_ms`; returns the cumulative
    /// `(charge_ah, energy_wh)` up to and including it. The first sample
    /// returns `(0.0, 0.0)`.
    pub fn push(&mut self, time_ms: u32, vbus_v: f64, ibus_a: f64) -> (f64, f64) {
        if let Some((t0, v0, i0)) = self.last {
            // Trapezoidal rule over the interval since the previous sample.
            // A backwards step (meter reboot) contributes nothing.
            let hours = f64::from(time_ms.saturating_sub(t0)) / 3_600_000.0;
            self.charge_ah += (i0 + ibus_a) / 2.0 * hours;
            self.energy_wh += (v0 * i0 + vbus_v * ibus_a) / 2.0 * hours;
        }
        self.last = Some((time_ms, vbus_v, ibus_a));
        (self.charge_ah, self.energy_wh)
    }

    /// End the current run without zeroing the sums: the interval up to the
    /// next sample contributes nothing (use it across an outage or a clock
    /// jump, where no samples were measured).
    pub fn break_run(&mut self) {
        self.last = None;
    }
}

/// `x` with at most 8 significant digits as a plain decimal, trailing zeros
/// trimmed; never exponent form (spreadsheets and the vendor recorder expect
/// plain decimals).
pub fn sig8(x: f64) -> String {
    if x == 0.0 || !x.is_finite() {
        return if x == 0.0 { "0".into() } else { x.to_string() };
    }
    let mag = x.abs().log10().floor() as i32;
    if mag >= 8 {
        return format!("{x:.0}");
    }
    let decimals = usize::try_from(7 - mag).unwrap_or(0);
    trim_zeros(&format!("{x:.decimals$}")).to_string()
}

fn trim_zeros(s: &str) -> &str {
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.')
    } else {
        s
    }
}

/// Writes [`QueueRecord`]s as rows under [`CSV_HEADER`] to `W`.
///
/// Elapsed time and charge/energy count from the first pushed record; the
/// `Unix` column is the wall clock passed with the first record plus the
/// device time elapsed since it. `TEMP` is the temperature passed with the
/// row ([`CsvRecorder::push_with_temp`]); queue records carry none, so
/// [`CsvRecorder::push`] leaves it empty.
#[derive(Debug)]
pub struct CsvRecorder<W: Write> {
    out: W,
    /// `(first record's device time, wall clock at it in ms)`.
    origin: Option<(u32, u64)>,
    integrator: Integrator,
    rows: u64,
}

impl<W: Write> CsvRecorder<W> {
    /// Write the header to `out` and return a recorder with no rows.
    pub fn new(mut out: W) -> io::Result<CsvRecorder<W>> {
        writeln!(out, "{CSV_HEADER}")?;
        Ok(CsvRecorder {
            out,
            origin: None,
            integrator: Integrator::new(),
            rows: 0,
        })
    }

    /// Write one row for `r`. `now_unix_ms` (ms since the Unix epoch) is
    /// used only for the first record: it fixes the `Unix` origin.
    pub fn push(&mut self, r: &QueueRecord, now_unix_ms: u64) -> io::Result<()> {
        self.push_with_temp(r, now_unix_ms, None)
    }

    /// [`Self::push`] with the `TEMP` column set to `temp_c` (°C), empty
    /// when `None`.
    pub fn push_with_temp(
        &mut self,
        r: &QueueRecord,
        now_unix_ms: u64,
        temp_c: Option<f64>,
    ) -> io::Result<()> {
        let t = r.time_ms();
        let (t0, unix0) = *self.origin.get_or_insert((t, now_unix_ms));
        let elapsed_ms = u64::from(t.saturating_sub(t0));
        let (charge_ah, energy_wh) = self.integrator.push(t, r.vbus_v(), r.ibus_a());
        let unix = (unix0 + elapsed_ms) as f64 / 1000.0;
        let temp = temp_c.map(sig8).unwrap_or_default();
        writeln!(
            self.out,
            "{},{unix:.3},{},{},{},{},{},{},{temp},{},{},{}",
            format_elapsed(elapsed_ms),
            sig8(r.vbus_v()),
            sig8(r.ibus_a()),
            sig8(r.dp_v()),
            sig8(r.dm_v()),
            sig8(r.cc1_v()),
            sig8(r.cc2_v()),
            sig8(charge_ah),
            sig8(energy_wh),
            sig8(r.vbus_v() * r.ibus_a()),
        )?;
        self.rows += 1;
        Ok(())
    }

    /// The interval up to the next row was not measured (an outage or a
    /// clock jump): it adds no charge or energy. Elapsed time is unchanged.
    pub fn break_run(&mut self) {
        self.integrator.break_run();
    }

    /// Rows written so far.
    pub fn rows(&self) -> u64 {
        self.rows
    }

    /// The underlying writer.
    pub fn get_ref(&self) -> &W {
        &self.out
    }

    /// Flush the underlying writer.
    pub fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }

    /// Flush and return the underlying writer.
    pub fn finish(mut self) -> io::Result<W> {
        self.out.flush()?;
        Ok(self.out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-12 + 1e-9 * b.abs()
    }

    // ---- AC-STQEA1.5: header, elapsed time, integration ----

    #[test]
    fn csv_header_matches_vendor_recorder() {
        assert_eq!(
            CSV_HEADER,
            "ElapsedTime,Unix,VBUS,IBUS,DP,DM,CC1,CC2,TEMP,CHARGE,ENERGY,PWR"
        );
    }

    #[test]
    fn format_elapsed_zero() {
        assert_eq!(format_elapsed(0), "0:00:00.000");
    }

    #[test]
    fn format_elapsed_pads_minutes_seconds_and_millis() {
        assert_eq!(format_elapsed(100), "0:00:00.100");
        assert_eq!(format_elapsed(7), "0:00:00.007");
        assert_eq!(format_elapsed(61_005), "0:01:01.005");
        assert_eq!(format_elapsed(3_723_456), "1:02:03.456");
    }

    #[test]
    fn format_elapsed_hours_are_not_padded_or_wrapped() {
        assert_eq!(format_elapsed(36_000_000), "10:00:00.000");
        assert_eq!(format_elapsed(100 * 3_600_000 + 59_999), "100:00:59.999");
    }

    #[test]
    fn integrator_first_sample_is_zero() {
        let mut i = Integrator::new();
        assert_eq!(i.push(50_000, 5.0, 2.0), (0.0, 0.0));
    }

    #[test]
    fn integrator_constant_positive_current() {
        let mut i = Integrator::new();
        i.push(10_000, 5.0, 2.0);
        // One hour at 2 A / 10 W in 1 s steps.
        let mut last = (0.0, 0.0);
        for k in 1..=3600u32 {
            last = i.push(10_000 + k * 1000, 5.0, 2.0);
        }
        assert!(close(last.0, 2.0), "charge {}", last.0);
        assert!(close(last.1, 10.0), "energy {}", last.1);
    }

    #[test]
    fn integrator_signed_current_gives_negative_charge_and_energy() {
        let mut i = Integrator::new();
        i.push(0, 5.0, -2.0);
        let (q, e) = i.push(100, 5.0, -2.0);
        assert!(close(q, -2.0 * 0.1 / 3600.0), "charge {q}");
        assert!(close(e, -10.0 * 0.1 / 3600.0), "energy {e}");
    }

    #[test]
    fn sig8_keeps_eight_significant_digits_without_float_noise() {
        assert_eq!(sig8(0.0), "0");
        assert_eq!(sig8(5.0), "5");
        assert_eq!(sig8(12.345678912), "12.345679");
        assert_eq!(sig8(-0.000012085555555555555), "-0.000012085556");
    }

    #[test]
    fn sig8_never_uses_exponent_form() {
        assert_eq!(sig8(-0.00000038183600000000003), "-0.000000381836");
        assert_eq!(sig8(-0.000000124), "-0.000000124");
        assert_eq!(sig8(2.5e-12), "0.0000000000025");
        assert_eq!(sig8(123_456_789.4), "123456789");
        for x in [1e-7, -3.3e-9, 4.2e-15, 1.5e9] {
            assert!(!sig8(x).contains('e'), "{x} -> {}", sig8(x));
        }
    }

    #[test]
    fn break_run_skips_one_interval_and_keeps_the_sums() {
        let mut i = Integrator::new();
        i.push(0, 5.0, 1.0);
        let before = i.push(3_600, 5.0, 1.0); // 0.001 Ah, 0.005 Wh
        i.break_run();
        // An hour-long outage adds nothing; the sums carry on after it.
        assert_eq!(i.push(3_603_600, 5.0, 1.0), before);
        let (q, e) = i.push(3_607_200, 5.0, 1.0);
        assert!(close(q, 0.002), "charge {q}");
        assert!(close(e, 0.010), "energy {e}");
    }

    #[test]
    fn a_recorder_run_break_skips_one_interval_in_charge_and_energy() {
        let row = |t: u32| {
            let mut b = Vec::with_capacity(20);
            b.extend_from_slice(&t.to_le_bytes());
            b.extend_from_slice(&5_000_000i32.to_le_bytes());
            b.extend_from_slice(&1_000_000i32.to_le_bytes());
            b.extend_from_slice(&[0u8; 8]);
            QueueRecord::decode(&b, powerz_protocol::decode::Rate::Sps1000).unwrap()
        };
        let mut csv = CsvRecorder::new(Vec::new()).unwrap();
        csv.push(&row(0), 0).unwrap();
        csv.push(&row(3_600), 0).unwrap();
        csv.break_run();
        csv.push(&row(3_603_600), 0).unwrap();
        let text = String::from_utf8(csv.finish().unwrap()).unwrap();
        let last: Vec<&str> = text.lines().last().unwrap().split(',').collect();
        assert_eq!((last[9], last[10]), ("0.001", "0.005"), "{text}");
    }

    #[test]
    fn integrator_ignores_an_interval_that_runs_backwards() {
        let mut i = Integrator::new();
        i.push(1_000, 5.0, 1.0);
        let (c1, e1) = i.push(4_600, 5.0, 1.0); // 3.6 s at 1 A = 0.001 Ah
        let (c2, e2) = i.push(100, 5.0, 1.0); // meter rebooted: no huge jump
        assert!((c1 - 0.001).abs() < 1e-12);
        assert_eq!((c2, e2), (c1, e1));
    }
}
