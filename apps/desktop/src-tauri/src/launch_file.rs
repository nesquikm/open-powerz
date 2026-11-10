//! A recording handed to the app at launch or by the OS while it runs (FR M078V5).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Whether `path` names an existing file the File tab can open: a CSV or a
/// vendor `.db`, extension matched ignoring case.
fn is_recording_file(path: &Path) -> bool {
    let wanted = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("csv") || e.eq_ignore_ascii_case("db"));
    wanted && path.is_file()
}

/// The launch file among the process arguments (`argv[0]` is skipped): the
/// first that names an existing CSV or `.db` file. Flags and anything else
/// are ignored.
pub fn launch_path_from_args(args: impl IntoIterator<Item = OsString>) -> Option<PathBuf> {
    args.into_iter()
        .skip(1)
        .map(PathBuf::from)
        .find(|p| is_recording_file(p))
}

/// The recording files among the URLs of an OS open-file event; anything
/// that is not a `file:` URL of an existing CSV or `.db` is dropped.
pub fn launch_paths_from_urls(urls: &[tauri::Url]) -> Vec<PathBuf> {
    urls.iter()
        .filter(|u| u.scheme() == "file")
        .filter_map(|u| u.to_file_path().ok())
        .filter(|p| is_recording_file(p))
        .collect()
}

/// The file waiting to be opened: taken once; a later hand-over replaces it.
#[derive(Debug, Default)]
pub struct PendingLaunch(Mutex<Option<PathBuf>>);

impl PendingLaunch {
    /// Hand over `path`, replacing any file not yet taken.
    pub fn offer(&self, path: PathBuf) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(path);
    }

    /// Take the pending file, leaving none.
    pub fn take(&self) -> Option<PathBuf> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};

    /// A fresh directory of our own under the temp dir.
    fn scratch(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("open-powerz-m078v5-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    fn touch(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, b"x").expect("write file");
        path
    }

    fn args(items: &[&std::ffi::OsStr]) -> Vec<OsString> {
        std::iter::once(OsString::from("/Applications/open-powerz"))
            .chain(items.iter().map(|s| s.to_os_string()))
            .collect()
    }

    // AC-M078V5.1: an existing CSV or `.db` argument is the launch file.
    #[test]
    fn an_existing_csv_argument_is_the_launch_file() {
        let dir = scratch("csv");
        let csv = touch(&dir, "run.csv");
        assert_eq!(
            launch_path_from_args(args(&[csv.as_os_str()])),
            Some(csv.clone())
        );
    }

    #[test]
    fn an_existing_db_argument_is_the_launch_file() {
        let dir = scratch("db");
        let db = touch(&dir, "run.db");
        assert_eq!(launch_path_from_args(args(&[db.as_os_str()])), Some(db));
    }

    #[test]
    fn the_extension_is_matched_ignoring_case() {
        let dir = scratch("case");
        let csv = touch(&dir, "RUN.CSV");
        let db = touch(&dir, "Run.Db");
        assert_eq!(
            launch_path_from_args(args(&[csv.as_os_str()])),
            Some(csv.clone())
        );
        assert_eq!(launch_path_from_args(args(&[db.as_os_str()])), Some(db));
    }

    #[test]
    fn the_first_usable_argument_wins() {
        let dir = scratch("first");
        let a = touch(&dir, "a.csv");
        let b = touch(&dir, "b.db");
        assert_eq!(
            launch_path_from_args(args(&[a.as_os_str(), b.as_os_str()])),
            Some(a)
        );
    }

    // AC-M078V5.2: anything else is ignored.
    #[test]
    fn no_arguments_give_no_launch_file() {
        assert_eq!(launch_path_from_args(args(&[])), None);
        assert_eq!(launch_path_from_args(Vec::<OsString>::new()), None);
    }

    #[test]
    fn argv0_is_never_the_launch_file() {
        let dir = scratch("argv0");
        let csv = touch(&dir, "self.csv");
        assert_eq!(launch_path_from_args(vec![csv.into_os_string()]), None);
    }

    #[test]
    fn a_missing_file_is_ignored() {
        let dir = scratch("missing");
        let gone = dir.join("gone.csv");
        assert_eq!(launch_path_from_args(args(&[gone.as_os_str()])), None);
    }

    #[test]
    fn another_extension_or_a_directory_is_ignored() {
        let dir = scratch("other");
        let txt = touch(&dir, "notes.txt");
        let bare = touch(&dir, "csv");
        let folder = dir.join("folder.csv");
        std::fs::create_dir_all(&folder).expect("dir");
        for arg in [&txt, &bare, &folder] {
            assert_eq!(
                launch_path_from_args(args(&[arg.as_os_str()])),
                None,
                "{arg:?}"
            );
        }
    }

    #[test]
    fn flags_are_skipped_and_a_later_file_still_opens() {
        let dir = scratch("flags");
        let csv = touch(&dir, "run.csv");
        let found = launch_path_from_args(args(&[
            std::ffi::OsStr::new("-psn_0_12345"),
            std::ffi::OsStr::new("--verbose"),
            csv.as_os_str(),
        ]));
        assert_eq!(found, Some(csv));
        assert_eq!(
            launch_path_from_args(args(&[std::ffi::OsStr::new("-psn_0_12345")])),
            None
        );
    }

    // AC-M078V5.3: the macOS open-file event hands file URLs.
    #[test]
    fn file_urls_become_paths_and_others_are_dropped() {
        let dir = scratch("urls");
        let csv = touch(&dir, "from finder.csv");
        let db = touch(&dir, "log.db");
        let urls = vec![
            tauri::Url::from_file_path(&csv).expect("file url"),
            tauri::Url::parse("https://example.com/run.csv").expect("url"),
            tauri::Url::from_file_path(dir.join("notes.txt")).expect("file url"),
            tauri::Url::from_file_path(dir.join("missing.db")).expect("file url"),
            tauri::Url::from_file_path(&db).expect("file url"),
        ];
        assert_eq!(launch_paths_from_urls(&urls), vec![csv, db]);
    }

    // AC-M078V5.1 / .3: the pending file is taken once; a later hand-over replaces it.
    #[test]
    fn the_pending_file_is_taken_once() {
        let pending = PendingLaunch::default();
        assert_eq!(pending.take(), None);
        pending.offer(PathBuf::from("/tmp/a.csv"));
        pending.offer(PathBuf::from("/tmp/b.db"));
        assert_eq!(pending.take(), Some(PathBuf::from("/tmp/b.db")));
        assert_eq!(pending.take(), None);
    }

    // AC-M078V5.3: the bundle can open CSV and `.db` but is never their default app.
    #[test]
    fn the_bundle_declares_csv_and_db_as_an_alternate_viewer() {
        // AC-M078V5.3: only the macOS bundle declares the associations.
        // Tauri's Windows installers (WiX / NSIS) write the extension's
        // default ProgId, so a shared `bundle.fileAssociations` would make
        // the app the default CSV handler there.
        let read = |name: &str| -> serde_json::Value {
            let path = format!("{}/{name}", env!("CARGO_MANIFEST_DIR"));
            let text =
                std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{name} must exist: {e}"));
            serde_json::from_str(&text).unwrap_or_else(|e| panic!("{name} is JSON: {e}"))
        };

        let shared = read("tauri.conf.json");
        assert!(
            shared["bundle"].get("fileAssociations").is_none(),
            "tauri.conf.json must not declare bundle.fileAssociations \
             (Windows/Linux installers would claim the default app): {}",
            shared["bundle"]["fileAssociations"]
        );

        let mac = read("tauri.macos.conf.json");
        let assocs = mac["bundle"]["fileAssociations"]
            .as_array()
            .expect("tauri.macos.conf.json bundle.fileAssociations is a list");
        let mut exts: Vec<String> = Vec::new();
        for assoc in assocs {
            assert_eq!(assoc["rank"], "Alternate", "{assoc}");
            assert_eq!(assoc["role"], "Viewer", "{assoc}");
            for ext in assoc["ext"].as_array().expect("ext list") {
                exts.push(ext.as_str().expect("ext string").to_ascii_lowercase());
            }
        }
        exts.sort();
        exts.dedup();
        assert_eq!(exts, vec!["csv".to_string(), "db".to_string()]);
    }
}
