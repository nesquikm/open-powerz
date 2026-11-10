//! Read-only import of a vendor PD log database into the PD analyzer view
//! (FR TGE83E, AC .6).
//!
//! The vendor app keeps a PD capture in an SQLite file `[static]`:
//!
//! - `pd_table(Time, Vbus, Ibus, Raw)`: one row per record, `Time` in s from
//!   the log's start, VBUS in V, IBUS in A and `Raw` the record bytes as the
//!   meter sends them (relative timestamp);
//! - `pd_chart(Time, VBUS, IBUS, CC1, CC2)`: the status samples (s, V, A, V, V);
//! - `pd_table_key(key)`: not needed here.
//!
//! The file is opened read-only (never created, never written), with a
//! shared lock, so rows the vendor app has committed but still holds in its
//! write-ahead log are seen too. Only when that open fails (read-only media,
//! say) is it opened immutable, which needs no lock but ignores a WAL. The
//! file is untrusted: its schema runs no functions and every value is
//! bounded ([`ImportLimits`]).

use std::path::Path;

use powerz_protocol::pd_capture::{NOT_PD_LOG, PdCapture, VendorLog, vendor_sql};
use rusqlite::config::DbConfig;
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags, OptionalExtension, Row};

/// Why a vendor PD log could not be imported.
#[derive(Debug)]
pub enum PdImportError {
    /// The file could not be opened or read as an SQLite database.
    Database(rusqlite::Error),
    /// The database has no `pd_table`: it is not a vendor PD log.
    NotPdLog,
}

impl std::fmt::Display for PdImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PdImportError::Database(e) => write!(f, "could not read the vendor PD log: {e}"),
            PdImportError::NotPdLog => f.write_str(NOT_PD_LOG),
        }
    }
}

impl std::error::Error for PdImportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            PdImportError::Database(e) => Some(e),
            PdImportError::NotPdLog => None,
        }
    }
}

impl From<rusqlite::Error> for PdImportError {
    fn from(e: rusqlite::Error) -> PdImportError {
        PdImportError::Database(e)
    }
}

pub use powerz_protocol::pd_capture::{ImportLimits, VendorImport};

/// Import the vendor PD log database at `path`, read-only, within the
/// default [`ImportLimits`]: every `pd_table` row becomes a decoded row
/// (with its own VBUS and IBUS), every `pd_chart` row a status sample;
/// relative times (s) become session time (ms).
pub fn import_vendor_pd_log(path: &Path) -> Result<PdCapture, PdImportError> {
    import_vendor_pd_log_with(path, &ImportLimits::DEFAULT).map(|i| i.capture)
}

/// [`import_vendor_pd_log`] within `limits`, telling what was left out. The
/// SQL and the row rules are the shared ones the web edition runs too.
pub fn import_vendor_pd_log_with(
    path: &Path,
    limits: &ImportLimits,
) -> Result<VendorImport, PdImportError> {
    let db = open_read_only(path)?;
    if !has_table(&db, "pd_table")? {
        return Err(PdImportError::NotPdLog);
    }
    let max_raw = i64::try_from(limits.max_raw_bytes).unwrap_or(i64::MAX);
    let oversized: i64 = db.query_row(vendor_sql::OVERSIZED, [max_raw], |r| r.get(0))?;
    let mut log = VendorLog::new(*limits, u64::try_from(oversized).unwrap_or(0));
    let fetch = i64::try_from(log.fetch()).unwrap_or(i64::MAX);
    let mut rows = db.prepare(vendor_sql::RECORDS)?;
    let mut rows = rows.query([max_raw, fetch])?;
    while let Some(row) = rows.next()? {
        if !log.push_record(real(row, 0)?, real(row, 1)?, real(row, 2)?, blob(row, 3)?) {
            break;
        }
    }
    if has_table(&db, "pd_chart")? {
        let mut chart = db.prepare(vendor_sql::CHART)?;
        let mut chart = chart.query([fetch])?;
        while let Some(row) = chart.next()? {
            let values = [
                real(row, 0)?,
                real(row, 1)?,
                real(row, 2)?,
                real(row, 3)?,
                real(row, 4)?,
            ];
            if !log.push_chart(values) {
                break;
            }
        }
    }
    Ok(log.finish())
}

/// Column `i` as a number; text, blobs and NULL are `None`.
pub(crate) fn real(row: &Row<'_>, i: usize) -> Result<Option<f64>, rusqlite::Error> {
    Ok(match row.get_ref(i)? {
        ValueRef::Real(f) => Some(f),
        ValueRef::Integer(n) => Some(n as f64),
        _ => None,
    })
}

/// Column `i` as bytes; only a blob is record bytes.
fn blob<'r>(row: &'r Row<'_>, i: usize) -> Result<Option<&'r [u8]>, rusqlite::Error> {
    Ok(match row.get_ref(i)? {
        ValueRef::Blob(b) => Some(b),
        _ => None,
    })
}

/// Open `path` read-only; never creates a file. A plain read-only open
/// (shared lock, sees a WAL) first; immutable only when that fails.
pub(crate) fn open_read_only(path: &Path) -> Result<Connection, rusqlite::Error> {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let plain = Connection::open_with_flags(path, flags).and_then(checked);
    match plain {
        Ok(db) => Ok(db),
        Err(first) => {
            Connection::open_with_flags(file_uri(path), flags | OpenFlags::SQLITE_OPEN_URI)
                .and_then(checked)
                // The first error says best why the file did not open.
                .map_err(|_| first)
        }
    }
}

/// Open `path` immutable: nothing is locked and nothing is written beside
/// it, and a WAL or journal is not read. Never creates a file.
pub(crate) fn open_immutable(path: &Path) -> Result<Connection, rusqlite::Error> {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
        | OpenFlags::SQLITE_OPEN_NO_MUTEX
        | OpenFlags::SQLITE_OPEN_URI;
    Connection::open_with_flags(file_uri(path), flags).and_then(checked)
}

/// `db` hardened for an untrusted file, once it reads as an SQLite database.
fn checked(db: Connection) -> Result<Connection, rusqlite::Error> {
    // The file's schema (views, triggers, defaults) may not call functions
    // with side effects, and nothing may corrupt or change it.
    db.set_db_config(DbConfig::SQLITE_DBCONFIG_TRUSTED_SCHEMA, false)?;
    db.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)?;
    db.pragma_update(None, "query_only", true)?;
    // Fail here, not later, when the file is not an SQLite database.
    db.query_row("SELECT count(*) FROM sqlite_master", [], |r| {
        r.get::<_, i64>(0)
    })?;
    Ok(db)
}

/// Whether the database has a table named `name`.
fn has_table(db: &Connection, name: &str) -> Result<bool, rusqlite::Error> {
    Ok(db
        .query_row(vendor_sql::HAS_TABLE, [name], |_| Ok(()))
        .optional()?
        .is_some())
}

/// An SQLite `file:` URI for `path`, opened immutable. On Unix the path's
/// bytes are kept as they are (a backslash is part of a file name there);
/// on Windows backslashes are separators.
fn file_uri(path: &Path) -> String {
    #[cfg(unix)]
    let mut p: Vec<u8> = std::os::unix::ffi::OsStrExt::as_bytes(path.as_os_str()).to_vec();
    #[cfg(windows)]
    let mut p: Vec<u8> = path.to_string_lossy().replace('\\', "/").into_bytes();
    #[cfg(not(any(unix, windows)))]
    let mut p: Vec<u8> = path.to_string_lossy().into_owned().into_bytes();
    // `C:/x` must read `/C:/x` in a URI.
    if p.first() != Some(&b'/') {
        p.insert(0, b'/');
    }
    let mut uri = String::from("file://");
    for b in p {
        if b.is_ascii_alphanumeric() || b"/-._~:".contains(&b) {
            uri.push(char::from(b));
        } else {
            uri.push_str(&format!("%{b:02X}"));
        }
    }
    uri.push_str("?immutable=1");
    uri
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_becomes_an_escaped_immutable_uri() {
        assert_eq!(
            file_uri(Path::new("/tmp/a b#?%.sqlite")),
            "file:///tmp/a%20b%23%3F%25.sqlite?immutable=1"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_backslash_in_a_unix_file_name_stays_part_of_the_name() {
        assert_eq!(
            file_uri(Path::new("/tmp/pd\\log.sqlite")),
            "file:///tmp/pd%5Clog.sqlite?immutable=1"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_non_utf8_unix_path_keeps_its_bytes() {
        use std::os::unix::ffi::OsStrExt;
        let path = Path::new(std::ffi::OsStr::from_bytes(b"/tmp/\xff.sqlite"));
        assert_eq!(file_uri(path), "file:///tmp/%FF.sqlite?immutable=1");
    }
}
