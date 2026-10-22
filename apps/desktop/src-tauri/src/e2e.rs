//! The real-app end-to-end build (FR AH2AEN, `--features e2e`): recordings
//! and exports go into a directory named by an environment variable instead
//! of through the save dialog, so a WebDriver suite can drive them. Absent
//! from the normal build.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// The environment variable naming the directory the e2e build saves into.
pub const SAVE_DIR_ENV: &str = "OPEN_POWERZ_E2E_SAVE_DIR";

/// Where the e2e build saves a file proposed as `suggested_name`:
/// `<dir>/<suggested_name>` when `dir` is set and non-empty, `None` (ask the
/// dialog as usual) otherwise. Only the last component of `suggested_name`
/// is used, so a name can never leave `dir`.
pub fn save_path(dir: Option<&OsStr>, suggested_name: &str) -> Option<PathBuf> {
    let dir = dir.filter(|d| !d.is_empty())?;
    let name = Path::new(suggested_name).file_name()?;
    Some(Path::new(dir).join(name))
}

#[cfg(test)]
mod tests {
    // `tests/e2e_save.rs` covers the plain name, an unset or empty dir, a
    // `../../`-prefixed name and a bare `..`; these are the remaining edges.
    use super::*;

    fn out() -> &'static OsStr {
        OsStr::new("out")
    }

    #[test]
    fn an_absolute_name_keeps_only_its_last_component() {
        assert_eq!(
            save_path(Some(out()), "/etc/x.csv"),
            Some(Path::new("out").join("x.csv"))
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_windows_absolute_name_keeps_only_its_last_component() {
        assert_eq!(
            save_path(Some(out()), r"C:\Users\x\rec.csv"),
            Some(Path::new("out").join("rec.csv"))
        );
    }

    #[test]
    fn a_name_ending_in_a_parent_component_has_no_file_name() {
        assert_eq!(save_path(Some(out()), "a/.."), None);
        assert_eq!(save_path(Some(out()), "a/b/../.."), None);
    }

    #[test]
    fn dot_components_and_trailing_separators_are_ignored() {
        assert_eq!(
            save_path(Some(out()), "./sub/./rec.csv/"),
            Some(Path::new("out").join("rec.csv"))
        );
        assert_eq!(save_path(Some(out()), "."), None);
    }

    #[test]
    fn an_empty_or_root_name_has_no_file_name() {
        assert_eq!(save_path(Some(out()), ""), None);
        assert_eq!(save_path(Some(out()), "/"), None);
    }

    #[test]
    fn an_empty_dir_wins_over_a_valid_name() {
        // Even a name that would be fine never turns an empty dir into a
        // relative path (the file would land in the working directory).
        assert_eq!(save_path(Some(OsStr::new("")), "/abs/rec.csv"), None);
        assert_eq!(save_path(None, "/abs/rec.csv"), None);
    }
}
