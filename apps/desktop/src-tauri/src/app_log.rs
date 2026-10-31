//! `app.log`: the session events, one timestamped line each, as evidence of
//! what happened during a run (AC-4PJRVM.5). Each run adds to the file. A
//! line names a meter by its device id (a bus address or a demo scenario),
//! never by serial or UID.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

/// Where session events go; the default logs nothing. Clones share the file.
#[derive(Debug, Clone, Default)]
pub struct AppLog {
    file: Option<Arc<Mutex<File>>>,
}

impl AppLog {
    /// Open the log at `path` for appending, creating it and its folder.
    pub fn open(path: &Path) -> io::Result<AppLog> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
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
