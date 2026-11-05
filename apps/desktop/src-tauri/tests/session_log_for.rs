//! FR AXHK2G, AC .2: what fails before a request reaches the worker (no
//! backup folder for a record delete, no session to send it to) still gets its
//! line. The commands have no test runtime, so they log through
//! `DeviceManager::session_log_for`, tested here; `tests/session_log.rs` pins
//! that the commands call it. Simulators only; synthetic serials.

#![allow(dead_code)]

mod common;

use std::path::{Path, PathBuf};

use common::*;
use open_powerz_desktop_lib::app_log::AppLog;
use open_powerz_desktop_lib::ui_error::UiError;

include!("common/two_meters.inc.rs");

fn lines(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .map(|s| s.lines().map(str::to_owned).collect())
        .unwrap_or_default()
}

fn logged_rig(tag: &str) -> (Rig, PathBuf) {
    let dir = std::env::temp_dir().join(format!("open-powerz-logfor-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("app.log");
    let mut rig = Rig::new();
    rig.m
        .set_app_log(AppLog::open(&path).expect("open app.log"));
    (rig, path)
}

/// Log `error` through `id`'s session log; one line naming meter A.
fn assert_logs_one_line(rig: &Rig, path: &Path, id: &str, error: &UiError) {
    let before = lines(path).len();
    rig.m.session_log_for(id).failure(error);
    let all = lines(path);
    let added = &all[before..];
    assert_eq!(added.len(), 1, "one line for one failure: {added:#?}");
    assert!(added[0].contains(METER_A), "names the meter: {}", added[0]);
    assert!(added[0].contains("KM003C"), "names the model: {}", added[0]);
    assert!(added[0].contains("usage"), "names the kind: {}", added[0]);
    assert!(!added[0].contains(SERIAL_A), "a serial: {}", added[0]);
}

#[test]
fn a_backup_folder_not_found_is_logged_for_a_connected_meter() {
    let (mut rig, path) = logged_rig("connected");
    let (_rx, _) = stream(&mut rig, METER_A);
    let error = UiError::usage("No folder found to write the record backup to.");
    assert_logs_one_line(&rig, &path, METER_A, &error);
    rig.m.disconnect_for(METER_A).unwrap();
}

#[test]
fn a_refusal_is_logged_for_a_meter_waiting_for_its_replug() {
    let (mut rig, path) = logged_rig("waiting");
    let (rx, _) = stream(&mut rig, METER_A);
    rig.unplug_a();
    recv_until(&rx, |e| {
        matches!(e, BackendEvent::Status(Status::Waiting { .. }))
    });
    let error = rig.m.control_for(METER_A).expect_err("no session");
    assert_logs_one_line(&rig, &path, METER_A, &error);
}
