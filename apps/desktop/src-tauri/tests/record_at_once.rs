//! AC-4PJRVM.2 on the backend side: Record starts at once on a default file
//! and asks where to save only at Stop. The recording's file lives at a
//! session default path; `save_recording_as` moves it where the user chose.

use std::path::PathBuf;

use open_powerz_desktop_lib::recording::{
    default_recording_path, is_recording_in, save_recording_as,
};

fn fresh_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("open-powerz-rec-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn the_default_path_is_the_proposed_name_in_a_folder_it_creates() {
    let dir = fresh_dir("default");
    let path = default_recording_path(&dir, "powerz-20261031-090507.csv").expect("a path");
    assert_eq!(path, dir.join("powerz-20261031-090507.csv"));
    assert!(dir.is_dir());
}

#[test]
fn the_default_path_never_takes_an_existing_file() {
    let dir = fresh_dir("taken");
    let first = default_recording_path(&dir, "powerz-1.csv").expect("a path");
    std::fs::write(&first, "keep me").unwrap();
    let second = default_recording_path(&dir, "powerz-1.csv").expect("a path");
    assert_ne!(first, second);
    assert_eq!(second.parent(), Some(dir.as_path()));
    assert_eq!(second.extension().and_then(|e| e.to_str()), Some("csv"));
    assert_eq!(std::fs::read_to_string(&first).unwrap(), "keep me");
}

#[test]
fn saving_as_moves_the_finished_file() {
    let dir = fresh_dir("move");
    let from = default_recording_path(&dir, "powerz-2.csv").expect("a path");
    std::fs::write(&from, "rows").unwrap();
    let to = dir.join("chosen.csv");
    save_recording_as(&from, &to).expect("moved");
    assert_eq!(std::fs::read_to_string(&to).unwrap(), "rows");
    assert!(!from.exists(), "no second copy is left behind");
}

#[test]
fn a_failed_move_keeps_the_recording_at_the_default_path() {
    let dir = fresh_dir("kept");
    let from = default_recording_path(&dir, "powerz-3.csv").expect("a path");
    std::fs::write(&from, "rows").unwrap();
    let to = dir.join("no-such-folder").join("chosen.csv");
    assert!(save_recording_as(&from, &to).is_err());
    assert_eq!(std::fs::read_to_string(&from).unwrap(), "rows");
}

#[test]
fn the_start_command_opens_no_dialog_and_stop_can_save_elsewhere() {
    let source = include_str!("../src/lib.rs");
    let from = source
        .find("fn start_recording")
        .expect("the start_recording command");
    let rest = &source[from..];
    let body = &rest[..rest.find("#[tauri::command]").unwrap_or(rest.len())];
    assert!(!body.contains("dialog()"), "start_recording opens a dialog");
    assert!(
        source.contains("save_recording_as"),
        "a command saves the finished recording elsewhere"
    );
}

#[test]
fn only_a_file_in_the_recordings_folder_can_be_moved() {
    let dir = PathBuf::from("/data/open-powerz-recordings");
    assert!(is_recording_in(&dir, &dir.join("powerz-1.csv")));
    assert!(!is_recording_in(&dir, &dir.join("sub/powerz-1.csv")));
    assert!(!is_recording_in(&dir, &dir.join("../secret.txt")));
    assert!(!is_recording_in(&dir, &PathBuf::from("/etc/hosts")));
    assert!(!is_recording_in(&dir, &dir));
}

/// A page reloaded mid-recording holds the worker's canonical path, while the
/// folder is named as the OS gave it, so the two can differ only in spelling.
#[cfg(unix)]
#[test]
fn a_recording_in_a_folder_reached_through_a_link_can_still_be_moved() {
    let base = fresh_dir("link");
    let real = base.join("real");
    std::fs::create_dir_all(&real).unwrap();
    let link = base.join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let file = std::fs::canonicalize(&real).unwrap().join("powerz-1.csv");
    std::fs::write(&file, "a,b\n").unwrap();

    assert!(
        is_recording_in(&link, &file),
        "the same folder, spelled differently, was refused"
    );
    assert!(
        !is_recording_in(&base, &file),
        "a file below the folder is not in it"
    );
}
