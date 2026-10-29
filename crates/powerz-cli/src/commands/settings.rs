//! `powerz settings`.

use std::io::Write;

use powerz_protocol::decode::Rate;
use powerz_protocol::decode::settings::{Row, WritableField};
use powerz_protocol::session::{Clock, Session};
use powerz_protocol::transport::Transport;

use crate::error::CliError;
use crate::output::write_json;

/// How `powerz settings` prints the block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsOutput {
    /// One `Label: value [tag]` line per field.
    Text,
    /// `{"fields": [{key, label, value, tag}, ...]}`.
    Json,
    /// Hex dump of the 180-byte block, 16 bytes per line with offsets; no
    /// CRC check, so a damaged block can still be inspected.
    Raw,
}

/// `powerz settings`: read the SETTINGS block with one GET_DATA (never a
/// write) and print it. A CRC mismatch fails the text and JSON views.
pub fn settings<T: Transport, C: Clock>(
    session: &mut Session<T, C>,
    how: SettingsOutput,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let block = session.read_settings()?;
    if how == SettingsOutput::Raw {
        for (i, chunk) in block.raw().chunks(16).enumerate() {
            let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
            writeln!(out, "{:04x}: {}", i * 16, hex.join(" "))?;
        }
        return Ok(());
    }
    let rows = block
        .meter_settings()
        .map_err(|e| CliError::Device(e.to_string()))?
        .rows();
    if how == SettingsOutput::Json {
        return write_json(out, &serde_json::json!({ "fields": rows }));
    }
    write_rows(&rows, out)
}

/// One `Label: value [tag]` line per row.
fn write_rows(rows: &[Row], out: &mut dyn Write) -> Result<(), CliError> {
    let width = rows.iter().map(|r| r.label.len()).max().unwrap_or(0) + 1;
    for r in rows {
        let label = format!("{}:", r.label);
        writeln!(
            out,
            "{label:<width$} {} [{}]",
            r.value,
            r.tag.as_str(),
            width = width
        )?;
    }
    Ok(())
}

/// What `--i-understand` acknowledges.
pub const SETTINGS_RISK: &str = "this changes the meter's settings; \
     pass --i-understand to write them (calibration is never written)";

/// Theme names in wire-code order.
const THEMES: [&str; 7] = [
    "blue",
    "green",
    "pink",
    "orange",
    "yellow",
    "dark-green",
    "cyan",
];

/// Wire code of `value` for `field`, written the way the setting is shown.
fn parse_value(field: WritableField, value: &str) -> Option<i32> {
    let v = value.trim().to_ascii_lowercase();
    let pick = |names: &[&str]| names.iter().position(|n| *n == v).map(|i| i as i32);
    let code = match field {
        WritableField::Language => pick(&["chinese", "english"]),
        WritableField::Brightness => v.strip_suffix('%').unwrap_or(&v).parse::<i32>().ok(),
        WritableField::DimTimeout => pick(&["off", "15", "30", "60"]),
        WritableField::Theme => pick(&THEMES),
        WritableField::Rotation => pick(&["0", "90", "180", "270"]),
        WritableField::SampleRate => v
            .parse::<u32>()
            .ok()
            .and_then(|sps| Rate::ALL.into_iter().find(|r| r.sps() == sps))
            .map(|r| i32::from(r.code())),
    }?;
    field.range().contains(&code).then_some(code)
}

/// Accepted values of `field`, for a usage error.
fn accepted(field: WritableField) -> &'static str {
    match field {
        WritableField::Language => "chinese or english",
        WritableField::Brightness => "1..100",
        WritableField::DimTimeout => "off, 15, 30 or 60",
        WritableField::Theme => "blue, green, pink, orange, yellow, dark-green or cyan",
        WritableField::Rotation => "0, 90, 180 or 270",
        WritableField::SampleRate => "2, 10, 50 or 1000",
    }
}

/// Parse `<field>=<value>` arguments; any bad one is a usage error.
pub fn parse_changes(args: &[String]) -> Result<Vec<(WritableField, i32)>, CliError> {
    if args.is_empty() {
        return Err(CliError::Usage(
            "no setting to change: pass <field>=<value> (e.g. brightness=80)".into(),
        ));
    }
    args.iter()
        .map(|arg| {
            let Some((name, value)) = arg.split_once('=') else {
                return Err(CliError::Usage(format!("`{arg}` is not <field>=<value>")));
            };
            let name = name.trim();
            if name.is_empty() {
                return Err(CliError::Usage(format!("`{arg}` names no field")));
            }
            let Some(field) = WritableField::from_name(name) else {
                let names: Vec<&str> = WritableField::ALL.iter().map(|f| f.name()).collect();
                return Err(CliError::Usage(format!(
                    "`{name}` cannot be changed; writable fields: {}",
                    names.join(", ")
                )));
            };
            let code = parse_value(field, value).ok_or_else(|| {
                CliError::Usage(format!(
                    "{name}={value} is not allowed; {name} takes {}",
                    accepted(field)
                ))
            })?;
            Ok((field, code))
        })
        .collect()
}

/// `powerz settings set <field>=<value> … --i-understand`: check every
/// change, send one settings write between two SETTINGS reads and print
/// the settings read back. A mismatching re-read is a device error naming
/// the field; nothing is retried.
pub fn settings_set<T: Transport, C: Clock>(
    session: &mut Session<T, C>,
    args: &[String],
    i_understand: bool,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    if !i_understand {
        return Err(CliError::Usage(SETTINGS_RISK.into()));
    }
    let changes = parse_changes(args)?;
    let after = session.write_settings(&changes)?;
    write_rows(&after.rows(), out)
}
