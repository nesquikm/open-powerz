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
    a.temp_c()
        .map_or_else(|| "-".to_string(), |t| format!("{t:.3}"))
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
    writeln!(out, "VBUS:   {:.3} V", a.vbus_v())?;
    writeln!(out, "IBUS:   {:.3} A", a.ibus_a())?;
    writeln!(out, "Power:  {:.3} W", a.power_w())?;
    writeln!(out, "Temp:   {temp} °C")?;
    writeln!(out, "D+:     {:.3} V", a.dp_v())?;
    writeln!(out, "D−:     {:.3} V", a.dm_v())?;
    writeln!(out, "CC1:    {:.3} V", a.cc1_v())?;
    writeln!(out, "CC2:    {:.3} V", a.cc2_v())?;
    Ok(())
}

/// The snapshot as one line, for `watch` when stdout is not a terminal.
pub(crate) fn write_adc_line(out: &mut dyn Write, a: &AdcSnapshot) -> Result<(), CliError> {
    let temp = temp_text(a);
    writeln!(
        out,
        "VBUS {:.3} V  IBUS {:.3} A  Power {:.3} W  Temp {temp} °C  \
         D+ {:.3} V  D− {:.3} V  CC1 {:.3} V  CC2 {:.3} V",
        a.vbus_v(),
        a.ibus_a(),
        a.power_w(),
        a.dp_v(),
        a.dm_v(),
        a.cc1_v(),
        a.cc2_v()
    )?;
    Ok(())
}
