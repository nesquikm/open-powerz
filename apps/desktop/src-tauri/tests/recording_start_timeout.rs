//! AC-AXHK2K.8: the file a recording start reserved. An answer that timed out
//! says nothing: the worker may still apply the start and write to the file,
//! so it stays. A definite refusal leaves an empty placeholder nobody will
//! use, which goes.

use std::path::{Path, PathBuf};

use open_powerz_desktop_lib::recording::{default_recording_path, start_at};
use open_powerz_desktop_lib::ui_error::{ErrorKind, UiError};

fn placeholder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("open-powerz-start-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    default_recording_path(&dir, "powerz-1.csv").expect("a path")
}

fn timed_out() -> UiError {
    UiError {
        kind: ErrorKind::Timeout,
        message: "The meter is busy. Try again.".to_owned(),
        hint: None,
        sessions: Vec::new(),
    }
}

#[test]
fn a_start_that_timed_out_keeps_its_file() {
    let path = placeholder("timeout");
    let answer = start_at(&path, |_: &Path| Err(timed_out()));
    assert_eq!(
        answer.expect_err("the start timed out").kind,
        ErrorKind::Timeout
    );
    assert!(path.exists(), "the worker may still write to this file");
}

#[test]
fn a_late_start_finds_its_file_where_it_was() {
    let path = placeholder("late");
    let _ = start_at(&path, |_: &Path| Err(timed_out()));
    // What the worker does when it gets to the request after all.
    std::fs::write(&path, "Time,VBUS\n").expect("the worker writes");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "Time,VBUS\n");
}

#[test]
fn a_definite_refusal_removes_the_empty_placeholder() {
    let path = placeholder("refused");
    let answer = start_at(&path, |_: &Path| {
        Err(UiError::usage("A recording is already running."))
    });
    assert_eq!(answer.expect_err("refused").kind, ErrorKind::Usage);
    assert!(!path.exists(), "the empty placeholder stayed behind");
}

#[test]
fn a_start_that_was_applied_keeps_its_file() {
    let path = placeholder("applied");
    start_at(&path, |_: &Path| Ok(())).expect("started");
    assert!(path.exists());
}

#[test]
fn the_start_command_goes_through_that_rule() {
    let source = include_str!("../src/lib.rs");
    let from = source
        .find("async fn start_recording")
        .expect("the start_recording command");
    let rest = &source[from..];
    let body = &rest[..rest.find("#[tauri::command]").unwrap_or(rest.len())];
    assert!(
        body.contains("start_at("),
        "the command has a rule of its own"
    );
    assert!(
        !body.contains("remove_file"),
        "the command removes the file on every error"
    );
}
