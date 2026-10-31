//! `powerz records list` and `powerz records get <n>|--all`: the meter's
//! offline records, read-only (FR 1HHT00); `powerz records delete
//! <n>|--all` deletes them after a CSV backup (FR JKWT3V).

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use powerz_protocol::decode::records::RecordEntry;
use powerz_protocol::session::{Clock, Session, record_delete_table};
use powerz_protocol::transport::Transport;
use powerz_record::{offline_record_csv, offline_record_file_name, sig8};

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
/// one table row (or JSON object) per record. Charge and energy are shown as
/// magnitudes, as the meter's screen shows them; the meter stores them
/// signed (negative for one current direction), and `--json` also gives
/// those raw values as `charge_raw_uah` / `energy_raw_uwh`.
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
                    "capacity_points": e.capacity_points,
                    "duration_s": e.duration_s,
                    "charge_ah": e.charge_magnitude_ah(),
                    "energy_wh": e.energy_magnitude_wh(),
                    "charge_raw_uah": e.charge_uah,
                    "energy_raw_uwh": e.energy_uwh,
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
                format!("{} Ah", sig8(e.charge_magnitude_ah())),
                format!("{} Wh", sig8(e.energy_magnitude_wh())),
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
/// selected records (consecutive memory reads of at most 256 bytes each)
/// and write each as
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
        let path = dir.join(offline_record_file_name(index, &entry.name));
        std::fs::write(&path, offline_record_csv(entry, &samples))
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

/// Options of `powerz records delete`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteOptions {
    /// `--i-understand`: the user acknowledged that a delete is final.
    pub i_understand: bool,
    /// `--yes`: skip the typed confirmation (still needs `--i-understand`).
    pub yes: bool,
    /// Folder the CSV backup is written to (must exist).
    pub out: PathBuf,
}

/// What `--i-understand` acknowledges.
pub const DELETE_RISK: &str = "deleting offline records cannot be undone; \
     pass --i-understand to delete (a CSV backup is written first)";

/// `powerz records delete`: read the record list, ask for the record name
/// (or "all") unless `--yes`, back the affected records up to `NN_<name>.csv`
/// files in `opts.out` (the `records get` download) and only then rewrite
/// the record table and read it again. A table too long to write is refused
/// before any of that. A failed backup deletes nothing; a failure after the
/// write names the backup folder and is not retried.
pub fn records_delete<T: Transport, C: Clock>(
    session: &mut Session<T, C>,
    which: RecordSelection,
    opts: &DeleteOptions,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    if !opts.i_understand {
        return Err(CliError::Usage(DELETE_RISK.into()));
    }
    let entries = session.list_records()?;
    let (positions, expected): (Vec<usize>, &str) = match which {
        RecordSelection::One(n) => {
            let Some(entry) = n.checked_sub(1).and_then(|i| entries.get(i)) else {
                return Err(CliError::Usage(format!(
                    "no record {n}: the meter has {} record(s); see `powerz records list`",
                    entries.len()
                )));
            };
            (vec![n - 1], &entry.name)
        }
        RecordSelection::All => ((0..entries.len()).collect(), "all"),
    };
    if positions.is_empty() {
        writeln!(out, "No offline records on the meter.")?;
        return Ok(());
    }
    record_delete_table(&entries, &positions)?;
    if !opts.yes {
        write!(out, "Type {expected:?} to delete it for good: ")?;
        out.flush()?;
        let mut line = String::new();
        input
            .read_line(&mut line)
            .map_err(|e| CliError::Usage(format!("no confirmation read: {e}")))?;
        let typed = line.strip_suffix('\n').unwrap_or(&line);
        let typed = typed.strip_suffix('\r').unwrap_or(typed);
        if typed != expected {
            return Err(CliError::Usage(format!(
                "confirmation did not match {expected:?}; nothing was deleted"
            )));
        }
    }
    for &p in &positions {
        let entry = &entries[p];
        let samples = if entry.points == 0 {
            Vec::new()
        } else {
            session.download_record(entry).map_err(|e| {
                CliError::Device(format!(
                    "backup of record {} failed, nothing was deleted: {e}",
                    p + 1
                ))
            })?
        };
        let path = opts.out.join(offline_record_file_name(p + 1, &entry.name));
        std::fs::write(&path, offline_record_csv(entry, &samples)).map_err(|e| {
            CliError::Device(format!(
                "backup failed, cannot write {}: {e}; nothing was deleted",
                path.display()
            ))
        })?;
        writeln!(out, "backed up {:?} to {}", entry.name, path.display())?;
    }
    let after = session.delete_records(&entries, &positions).map_err(|e| {
        let note = format!("; the backup is in {}", opts.out.display());
        match CliError::from(e) {
            CliError::Usage(m) => CliError::Usage(m + &note),
            CliError::NoDevice(m) => CliError::NoDevice(m + &note),
            CliError::Permission(m) => CliError::Permission(m + &note),
            CliError::Device(m) => CliError::Device(m + &note),
            closed @ CliError::OutputClosed => closed,
        }
    })?;
    match which {
        RecordSelection::One(_) => writeln!(out, "deleted {expected:?}")?,
        RecordSelection::All => writeln!(out, "deleted all records")?,
    }
    writeln!(out, "{} record(s) left on the meter", after.len())?;
    Ok(())
}
