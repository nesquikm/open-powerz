//! CSV recording helpers for `powerz stream`: the vendor-compatible header,
//! elapsed-time formatting and charge/energy integration.

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
            let hours = f64::from(time_ms.wrapping_sub(t0)) / 3_600_000.0;
            self.charge_ah += (i0 + ibus_a) / 2.0 * hours;
            self.energy_wh += (v0 * i0 + vbus_v * ibus_a) / 2.0 * hours;
        }
        self.last = Some((time_ms, vbus_v, ibus_a));
        (self.charge_ah, self.energy_wh)
    }
}

/// `x` with at most 8 significant digits, trailing zeros trimmed, in
/// exponent form below 1e-5 (like the vendor recorder's precision 8).
pub fn sig8(x: f64) -> String {
    if x == 0.0 || !x.is_finite() {
        return if x == 0.0 { "0".into() } else { x.to_string() };
    }
    let mag = x.abs().log10().floor() as i32;
    if !(-5..8).contains(&mag) {
        let text = format!("{x:.7e}");
        let (mantissa, exp) = text.split_once('e').unwrap_or((&text, "0"));
        return format!("{}e{exp}", trim_zeros(mantissa));
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
        assert_eq!(sig8(-0.00000038183600000000003), "-3.81836e-7");
        assert_eq!(sig8(0.0), "0");
        assert_eq!(sig8(5.0), "5");
        assert_eq!(sig8(12.345678912), "12.345679");
        assert_eq!(sig8(-0.000012085555555555555), "-0.000012085556");
        assert_eq!(sig8(2.5e-12), "2.5e-12");
    }
}
