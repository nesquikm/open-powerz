//! `powerz records list` and `powerz records get <n>|--all`: the meter's
//! offline records, read-only (FR 1HHT00).

use std::io::Write;
use std::path::Path;

use powerz_protocol::decode::records::{RecordEntry, RecordSample};
use powerz_protocol::session::{Clock, Session};
use powerz_protocol::transport::Transport;
use powerz_record::{CSV_HEADER, format_elapsed, sig8};

use crate::error::CliError;
use crate::output::write_json;

/// Which records `powerz records get` downloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordSelection {
    /// One record by its 1-based index, as `records list` shows it.
    One(usize),
    /// Every record with samples.
    All,
}

/// `powerz records list`: authenticate, read the record list once and print
/// one table row (or JSON object) per record.
pub fn records_list<T: Transport, C: Clock>(
    session: &mut Session<T, C>,
    json: bool,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let entries = session.list_records()?;
    if json {
        let records: Vec<serde_json::Value> = entries
            .iter()
            .enumerate()
            .map(|(i, e)| {
                serde_json::json!({
                    "index": i + 1,
                    "name": e.name,
                    "interval_s": e.interval_s,
                    "points": e.points,
                    "duration_s": e.duration_s,
                    "charge_ah": e.charge_ah(),
                    "energy_wh": e.energy_wh(),
                    "offset": e.offset,
                })
            })
            .collect();
        return write_json(out, &serde_json::json!({ "records": records }));
    }
    if entries.is_empty() {
        writeln!(out, "No offline records on the meter.")?;
        return Ok(());
    }
    let rows: Vec<[String; 8]> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            [
                (i + 1).to_string(),
                e.name.clone(),
                format!("{} s", e.interval_s),
                e.points.to_string(),
                format_duration(e.duration_s),
                format!("{} Ah", sig8(e.charge_ah())),
                format!("{} Wh", sig8(e.energy_wh())),
                format!("{:#010x}", e.offset),
            ]
        })
        .collect();
    let header = [
        "#", "Name", "Interval", "Points", "Duration", "Charge", "Energy", "Offset",
    ];
    let mut widths = header.map(str::len);
    for r in &rows {
        for (w, cell) in widths.iter_mut().zip(r) {
            *w = (*w).max(cell.chars().count());
        }
    }
    let line = |cells: &[&str]| {
        let parts: Vec<String> = cells
            .iter()
            .zip(widths)
            .enumerate()
            .map(|(i, (c, w))| {
                // Name left-aligned, numbers right-aligned.
                if i == 1 {
                    format!("{c:<w$}")
                } else {
                    format!("{c:>w$}")
                }
            })
            .collect();
        parts.join("  ").trim_end().to_string()
    };
    writeln!(out, "{}", line(&header))?;
    for r in &rows {
        let cells: Vec<&str> = r.iter().map(String::as_str).collect();
        writeln!(out, "{}", line(&cells))?;
    }
    Ok(())
}

/// `powerz records get`: read the record list once, then download the
/// selected records (one memory read each) and write each as
/// `NN_<name>.csv` in `dir`, in the recorder CSV layout. `--all` skips
/// records with no samples; an index outside the list is a usage error and
/// nothing is read or written.
pub fn records_get<T: Transport, C: Clock>(
    session: &mut Session<T, C>,
    which: RecordSelection,
    dir: &Path,
    json: bool,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let entries = session.list_records()?;
    let selected: Vec<(usize, &RecordEntry)> = match which {
        RecordSelection::One(n) => {
            if n == 0 || n > entries.len() {
                return Err(CliError::Usage(format!(
                    "no record {n}: the meter has {} record(s); see `powerz records list`",
                    entries.len()
                )));
            }
            vec![(n, &entries[n - 1])]
        }
        RecordSelection::All => entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.points > 0)
            .map(|(i, e)| (i + 1, e))
            .collect(),
    };
    let mut written = Vec::new();
    for (index, entry) in selected {
        let samples = if entry.points == 0 {
            Vec::new()
        } else {
            session.download_record(entry)?
        };
        let path = dir.join(file_name(index, &entry.name));
        std::fs::write(&path, record_csv(entry, &samples))
            .map_err(|e| CliError::Device(format!("cannot write {}: {e}", path.display())))?;
        if !json {
            writeln!(out, "wrote {} ({} rows)", path.display(), samples.len())?;
        }
        written.push((index, entry, samples.len(), path));
    }
    if json {
        let written: Vec<serde_json::Value> = written
            .iter()
            .map(|(index, entry, rows, path)| {
                serde_json::json!({
                    "index": index,
                    "name": entry.name,
                    "rows": rows,
                    "path": path.display().to_string(),
                })
            })
            .collect();
        return write_json(out, &serde_json::json!({ "written": written }));
    }
    Ok(())
}

/// `h:mm:ss` of `secs`.
fn format_duration(secs: u32) -> String {
    format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
}

/// `NN_<name>.csv`, with every character outside `[A-Za-z0-9_-]` replaced
/// by `_` so the file always lands inside the output directory.
fn file_name(index: usize, name: &str) -> String {
    let mut safe: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if safe.is_empty() {
        safe.push_str("record");
    }
    format!("{index:02}_{safe}.csv")
}

/// The record as recorder CSV: elapsed time from the entry's interval, no
/// `Unix` time (the start is unknown), VBUS/IBUS/charge/energy from the
/// samples, power = VBUS × IBUS; D+/D-/CC/TEMP are not recorded.
fn record_csv(entry: &RecordEntry, samples: &[RecordSample]) -> String {
    let mut csv = format!("{CSV_HEADER}\n");
    for (k, s) in samples.iter().enumerate() {
        csv.push_str(&format!(
            "{},,{},{},,,,,,{},{},{}\n",
            format_elapsed(entry.sample_time_s(k).saturating_mul(1000)),
            sig8(s.vbus_v()),
            sig8(s.ibus_a()),
            sig8(s.charge_ah()),
            sig8(s.energy_wh()),
            sig8(s.vbus_v() * s.ibus_a()),
        ));
    }
    csv
}
