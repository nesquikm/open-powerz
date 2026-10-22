//! The e2e build's save-path choice (FR AH2AEN, AC-AH2AEN.3).
#![cfg(feature = "e2e")]

use std::ffi::OsStr;
use std::path::Path;

use open_powerz_desktop_lib::e2e::{SAVE_DIR_ENV, save_path};

#[test]
fn env_var_name_matches_the_wdio_suite() {
    assert_eq!(SAVE_DIR_ENV, "OPEN_POWERZ_E2E_SAVE_DIR");
}

#[test]
fn saves_into_the_dir_under_the_suggested_name() {
    let dir = Path::new("some").join("dir");
    assert_eq!(
        save_path(Some(dir.as_os_str()), "rec-2026.csv"),
        Some(dir.join("rec-2026.csv"))
    );
}

#[test]
fn unset_or_empty_dir_falls_back_to_the_dialog() {
    assert_eq!(save_path(None, "rec.csv"), None);
    assert_eq!(save_path(Some(OsStr::new("")), "rec.csv"), None);
}

#[test]
fn suggested_name_cannot_escape_the_dir() {
    let dir = Path::new("out");
    assert_eq!(
        save_path(Some(dir.as_os_str()), "../../etc/x.csv"),
        Some(dir.join("x.csv"))
    );
    assert_eq!(save_path(Some(dir.as_os_str()), ".."), None);
}
