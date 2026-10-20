//! Output formatting shared by the subcommands: pretty JSON and the ADC
//! snapshot views used by `read` and `watch`.

use std::io::Write;

use powerz_protocol::decode::AdcSnapshot;

use crate::error::CliError;

/// Write `value` as pretty-printed JSON followed by a newline.
pub(crate) fn write_json(out: &mut dyn Write, value: &serde_json::Value) -> Result<(), CliError> {
    let text = serde_json::to_string_pretty(value)
        .map_err(|e| CliError::Device(format!("JSON encoding failed: {e}")))?;
    writeln!(out, "{text}")?;
    Ok(())
}

/// Temperature with 3 decimals, or `-` when the meter reports it invalid.
fn temp_text(a: &AdcSnapshot) -> String {
    a.temp_c().map_or_else(|| "-".to_string(), fixed3)
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

/// The snapshot as labelled lines, one value per line, 3 decimals.
pub(crate) fn write_adc_lines(out: &mut dyn Write, a: &AdcSnapshot) -> Result<(), CliError> {
    let temp = temp_text(a);
    writeln!(out, "VBUS:   {} V", fixed3(a.vbus_v()))?;
    writeln!(out, "IBUS:   {} A", fixed3(a.ibus_a()))?;
    writeln!(out, "Power:  {} W", fixed3(a.power_w()))?;
    writeln!(out, "Temp:   {temp} °C")?;
    writeln!(out, "D+:     {} V", fixed3(a.dp_v()))?;
    writeln!(out, "D−:     {} V", fixed3(a.dm_v()))?;
    writeln!(out, "CC1:    {} V", fixed3(a.cc1_v()))?;
    writeln!(out, "CC2:    {} V", fixed3(a.cc2_v()))?;
    Ok(())
}

/// The snapshot as one line, for `watch` when stdout is not a terminal.
pub(crate) fn write_adc_line(out: &mut dyn Write, a: &AdcSnapshot) -> Result<(), CliError> {
    let temp = temp_text(a);
    writeln!(
        out,
        "VBUS {} V  IBUS {} A  Power {} W  Temp {temp} °C  \
         D+ {} V  D− {} V  CC1 {} V  CC2 {} V",
        fixed3(a.vbus_v()),
        fixed3(a.ibus_a()),
        fixed3(a.power_w()),
        fixed3(a.dp_v()),
        fixed3(a.dm_v()),
        fixed3(a.cc1_v()),
        fixed3(a.cc2_v())
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
