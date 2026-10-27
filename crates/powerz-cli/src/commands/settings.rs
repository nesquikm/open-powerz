//! `powerz settings`.

use std::io::Write;

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
    let width = rows.iter().map(|r| r.label.len()).max().unwrap_or(0) + 1;
    for r in &rows {
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
