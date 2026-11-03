//! `app.log`: the session events, one timestamped line each, as evidence of
//! what happened during a run (AC-4PJRVM.5). Each run adds to the file, until
//! it grows past [`MAX_LOG_BYTES`] and moves aside (AC-AXHK2G.6). A line names
//! a meter by its device id (a bus address or a demo scenario) and model, never
//! by serial or UID.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::ui_error::UiError;

/// A log over this size moves to `app.log.1` when the app starts.
const MAX_LOG_BYTES: u64 = 5_000_000;

/// Where session events go; the default logs nothing. Clones share the file.
#[derive(Debug, Clone, Default)]
pub struct AppLog {
    file: Option<Arc<Mutex<File>>>,
}

impl AppLog {
    /// Open the log at `path` for appending, creating it and its folder. The
    /// app opens it once, at start, so this is where an oversized log rotates.
    pub fn open(path: &Path) -> io::Result<AppLog> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        rotate_if_oversized(path);
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(AppLog {
            file: Some(Arc::new(Mutex::new(file))),
        })
    }

    /// Add one timestamped line. A failed write is dropped: the log must
    /// never get in the way of a session.
    pub fn line(&self, text: &str) {
        let Some(file) = &self.file else {
            return;
        };
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let line = format!("{} {text}\n", utc_timestamp(secs));
        let mut file = file.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = file.write_all(line.as_bytes());
    }
}

/// Move a log over [`MAX_LOG_BYTES`] to `<path>.1`, replacing any earlier one.
/// A failed move only says so on stderr: the run goes on, adding to the log
/// it found.
fn rotate_if_oversized(path: &Path) {
    let oversized = fs::metadata(path).is_ok_and(|m| m.len() > MAX_LOG_BYTES);
    if oversized && let Err(e) = fs::rename(path, path.with_added_extension("1")) {
        eprintln!("app.log stays over {MAX_LOG_BYTES} bytes: {e}");
    }
}

/// The log as one meter's session writes to it: every line names the meter
/// by id and model (AC-AXHK2G.4). The default logs nothing.
#[derive(Debug, Clone, Default)]
pub struct SessionLog {
    log: AppLog,
    id: String,
    model: String,
}

impl SessionLog {
    /// `id` and `model` come from the listing, never the serial.
    pub fn new(log: AppLog, id: String, model: String) -> SessionLog {
        SessionLog { log, id, model }
    }

    /// The meter as every line names it.
    fn meter(&self) -> String {
        format!("{} ({})", self.id, self.model)
    }

    /// Add the line for `event` on this meter.
    pub fn event(&self, event: &str) {
        self.log.line(&format!("{event} {}", self.meter()));
    }

    /// Add the line for an error that leaves the session running, with its
    /// message. For an error whose message holds no serial or UID and no
    /// folder: one that can quote a path goes to
    /// [`failure_kind`](Self::failure_kind).
    pub fn failure(&self, error: &UiError) {
        let (meter, kind) = (self.meter(), kind_name(error));
        self.log
            .line(&format!("error {meter}: {kind}: {}", error.message));
    }

    /// [`failure`](Self::failure) with the kind alone: a failed record delete
    /// quotes its backup folder, and an OS error can quote more.
    pub fn failure_kind(&self, error: &UiError) {
        let (meter, kind) = (self.meter(), kind_name(error));
        self.log.line(&format!("error {meter}: {kind}"));
    }

    /// Add the line for `event` on this meter with its `detail`, like
    /// `live batches dropped usb:1:7 (KM003C): page fell 512 batches behind`.
    pub fn event_with(&self, event: &str, detail: &str) {
        self.log
            .line(&format!("{event} {}: {detail}", self.meter()));
    }

    /// Add a line of a firmware update: its start, a stage it entered, or
    /// how it ended (AC-AXHK2G.3).
    pub fn firmware(&self, event: &str) {
        self.event(&format!("firmware {event}"));
    }

    /// Add the outcome line of a failed firmware update. The kind alone: the
    /// message can quote a file path or the meter's raw replies.
    pub fn firmware_failed(&self, error: &UiError) {
        let (meter, kind) = (self.meter(), kind_name(error));
        self.log.line(&format!("firmware failed {meter}: {kind}"));
    }
}

/// An error's kind by the name the UI gets.
fn kind_name(error: &UiError) -> String {
    let kind = serde_json::to_value(error.kind).unwrap_or_default();
    kind.as_str().unwrap_or("unknown").to_owned()
}

/// `secs` since the Unix epoch in UTC, like `2026-10-30T10:00:00Z`. The date
/// comes from the proleptic Gregorian calendar's 400-year cycle, counted in
/// years that start on 1 March.
fn utc_timestamp(secs: u64) -> String {
    let (days, rest) = (secs / 86_400, secs % 86_400);
    let shifted = days + 719_468;
    let (era, day_of_era) = (shifted / 146_097, shifted % 146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = era * 400 + year_of_era + u64::from(month <= 2);
    let (hour, minute, second) = (rest / 3_600, rest / 60 % 60, rest % 60);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_are_utc_dates() {
        assert_eq!(utc_timestamp(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc_timestamp(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(utc_timestamp(1_793_354_641), "2026-10-30T10:04:01Z");
    }
}
