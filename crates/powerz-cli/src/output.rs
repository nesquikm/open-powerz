//! Output formatting shared by the subcommands: pretty JSON and the ADC
//! reading views used by `read` and `watch`.

use std::io::{self, Write};

use powerz_protocol::decode::{AdcSnapshot, QueueRecord};
use serde::Serialize;
use serde_json::ser::{Formatter, PrettyFormatter};

use crate::error::CliError;

/// Write `value` as pretty-printed JSON followed by a newline. Floats are
/// written as plain decimals, never in exponent form.
pub(crate) fn write_json(out: &mut dyn Write, value: &serde_json::Value) -> Result<(), CliError> {
    let mut buf = Vec::new();
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, PlainFloats::default());
    value
        .serialize(&mut ser)
        .map_err(|e| CliError::Device(format!("JSON encoding failed: {e}")))?;
    out.write_all(&buf)?;
    writeln!(out)?;
    Ok(())
}

/// Pretty JSON whose floats use Rust's `Display`, the shortest round-trip
/// digits without an exponent (`-0.000001`, not `-1e-6`); whole numbers keep
/// a `.0`.
#[derive(Default)]
struct PlainFloats(PrettyFormatter<'static>);

impl Formatter for PlainFloats {
    fn write_f64<W: ?Sized + io::Write>(&mut self, w: &mut W, value: f64) -> io::Result<()> {
        let text = value.to_string();
        // Keep whole numbers float-shaped (`5.0`), as serde_json does.
        let suffix = if text.contains('.') { "" } else { ".0" };
        write!(w, "{text}{suffix}")
    }
    fn begin_array<W: ?Sized + io::Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.0.begin_array(w)
    }
    fn end_array<W: ?Sized + io::Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.0.end_array(w)
    }
    fn begin_array_value<W: ?Sized + io::Write>(
        &mut self,
        w: &mut W,
        first: bool,
    ) -> io::Result<()> {
        self.0.begin_array_value(w, first)
    }
    fn end_array_value<W: ?Sized + io::Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.0.end_array_value(w)
    }
    fn begin_object<W: ?Sized + io::Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.0.begin_object(w)
    }
    fn end_object<W: ?Sized + io::Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.0.end_object(w)
    }
    fn begin_object_key<W: ?Sized + io::Write>(
        &mut self,
        w: &mut W,
        first: bool,
    ) -> io::Result<()> {
        self.0.begin_object_key(w, first)
    }
    fn begin_object_value<W: ?Sized + io::Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.0.begin_object_value(w)
    }
    fn end_object_value<W: ?Sized + io::Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.0.end_object_value(w)
    }
}

/// The values `read` and `watch` show, from an ADC snapshot or a pushed
/// ADCNEW record (which carries no temperature).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Reading {
    vbus_v: f64,
    ibus_a: f64,
    power_w: f64,
    temp_c: Option<f64>,
    dp_v: f64,
    dm_v: f64,
    cc1_v: f64,
    cc2_v: f64,
}

impl From<&AdcSnapshot> for Reading {
    fn from(a: &AdcSnapshot) -> Reading {
        Reading {
            vbus_v: a.vbus_v(),
            ibus_a: a.ibus_a(),
            power_w: a.power_w(),
            temp_c: a.temp_c(),
            dp_v: a.dp_v(),
            dm_v: a.dm_v(),
            cc1_v: a.cc1_v(),
            cc2_v: a.cc2_v(),
        }
    }
}

impl From<&QueueRecord> for Reading {
    fn from(r: &QueueRecord) -> Reading {
        Reading {
            vbus_v: r.vbus_v(),
            ibus_a: r.ibus_a(),
            power_w: r.vbus_v() * r.ibus_a(),
            temp_c: None,
            dp_v: r.dp_v(),
            dm_v: r.dm_v(),
            cc1_v: r.cc1_v(),
            cc2_v: r.cc2_v(),
        }
    }
}

/// Temperature with 1 decimal, or `-` when invalid or not reported.
fn temp_text(a: &Reading) -> String {
    a.temp_c
        .map_or_else(|| "-".to_string(), |t| format!("{t:.1}"))
}

/// The snapshot as JSON: `vbus_v`, `ibus_a`, `power_w`, `temp_c` (null when
/// invalid), `dp_v`, `dm_v`, `cc1_v`, `cc2_v`.
pub(crate) fn adc_json(a: &AdcSnapshot) -> serde_json::Value {
    serde_json::json!({
        "vbus_v": a.vbus_v(),
        "ibus_a": a.ibus_a(),
        "power_w": a.power_w(),
        "temp_c": a.temp_c(),
        "dp_v": a.dp_v(),
        "dm_v": a.dm_v(),
        "cc1_v": a.cc1_v(),
        "cc2_v": a.cc2_v(),
    })
}

/// The reader of our output went away, as `powerz watch | head` makes it do.
/// Not a failure: a streaming command simply ends.
pub(crate) fn is_closed_pipe(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::BrokenPipe
}

/// The snapshot as labelled lines, one value per line, 3 decimals
/// (temperature 1).
pub(crate) fn write_adc_lines(out: &mut dyn Write, a: &Reading) -> io::Result<()> {
    let temp = temp_text(a);
    writeln!(out, "VBUS:   {} V", fixed3(a.vbus_v))?;
    writeln!(out, "IBUS:   {} A", fixed3(a.ibus_a))?;
    writeln!(out, "Power:  {} W", fixed3(a.power_w))?;
    writeln!(out, "Temp:   {temp} °C")?;
    writeln!(out, "D+:     {} V", fixed3(a.dp_v))?;
    writeln!(out, "D−:     {} V", fixed3(a.dm_v))?;
    writeln!(out, "CC1:    {} V", fixed3(a.cc1_v))?;
    writeln!(out, "CC2:    {} V", fixed3(a.cc2_v))?;
    Ok(())
}

/// The snapshot as one line, for `watch` when stdout is not a terminal.
pub(crate) fn write_adc_line(out: &mut dyn Write, a: &Reading) -> io::Result<()> {
    let temp = temp_text(a);
    writeln!(
        out,
        "VBUS {} V  IBUS {} A  Power {} W  Temp {temp} °C  \
         D+ {} V  D− {} V  CC1 {} V  CC2 {} V",
        fixed3(a.vbus_v),
        fixed3(a.ibus_a),
        fixed3(a.power_w),
        fixed3(a.dp_v),
        fixed3(a.dm_v),
        fixed3(a.cc1_v),
        fixed3(a.cc2_v)
    )?;
    Ok(())
}

/// `x` with three decimals; values that round to zero print as `0.000`,
/// never `-0.000`.
pub(crate) fn fixed3(x: f64) -> String {
    let text = format!("{x:.3}");
    if text == "-0.000" {
        "0.000".to_string()
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed3_never_prints_negative_zero() {
        assert_eq!(fixed3(-0.000115), "0.000");
        assert_eq!(fixed3(-0.0), "0.000");
        assert_eq!(fixed3(-0.0012), "-0.001");
        assert_eq!(fixed3(5.0), "5.000");
    }
}
