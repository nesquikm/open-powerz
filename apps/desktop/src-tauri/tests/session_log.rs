//! AC-4PJRVM.5: app.log records the session events (connect, disconnect,
//! replug, errors) so it can serve as evidence. The manager writes through an
//! `AppLog`, one timestamped line per event, naming the meter. Simulators
//! only; synthetic serials.

#![allow(dead_code)]

mod common;

use std::path::{Path, PathBuf};

use common::*;
use open_powerz_desktop_lib::app_log::AppLog;

include!("common/two_meters.inc.rs");

fn log_path(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("open-powerz-applog-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("app.log")
}

fn lines(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .map(|s| s.lines().map(str::to_owned).collect())
        .unwrap_or_default()
}

/// The first line with `word` (any case) and, when given, `id`; waits for it.
fn wait_line(path: &Path, word: &str, id: Option<&str>) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let found = lines(path)
            .into_iter()
            .find(|l| l.to_lowercase().contains(word) && id.is_none_or(|i| l.contains(i)));
        if let Some(line) = found {
            return line;
        }
        assert!(
            Instant::now() < deadline,
            "no {word:?} line in app.log: {:#?}",
            lines(path)
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn rig_logging_to(path: &Path) -> Rig {
    let mut rig = Rig::new();
    rig.m.set_app_log(AppLog::open(path).expect("open app.log"));
    rig
}

#[test]
fn a_connect_and_a_disconnect_are_logged_with_time_and_meter() {
    let path = log_path("connect");
    let mut rig = rig_logging_to(&path);
    let (_rx, _) = stream(&mut rig, METER_A);
    let connected = wait_line(&path, "connect", Some(METER_A));
    assert!(
        connected.chars().take(4).all(|c| c.is_ascii_digit()),
        "a line starts with its timestamp: {connected}"
    );
    rig.m.disconnect_for(METER_A).unwrap();
    wait_line(&path, "disconnect", Some(METER_A));
}

#[test]
fn an_unplug_and_the_replug_are_logged() {
    let path = log_path("replug");
    let mut rig = rig_logging_to(&path);
    let (rx, _) = stream(&mut rig, METER_A);
    rig.unplug_a();
    recv_until(&rx, |e| {
        matches!(e, BackendEvent::Status(Status::Disconnected { .. }))
    });
    wait_line(&path, "unplug", Some(METER_A));
    let mut back = rig.replug_a();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !back && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
        let list = rig.m.list_devices();
        back = rig.m.devices_changed(&list);
    }
    assert!(back, "meter A did not come back");
    wait_line(&path, "replug", None);
    for id in rig.m.connected_ids() {
        rig.m.disconnect_for(&id).unwrap();
    }
}

#[test]
fn a_failed_connect_is_logged_as_an_error() {
    let path = log_path("error");
    let mut rig = rig_logging_to(&path);
    // Listed by the OS, but the opener cannot reach it.
    rig.plug_a.store(false, Ordering::SeqCst);
    let (_rx, sink) = channel_sink();
    let _ = rig.m.connect(METER_A, 50, sink);
    wait_line(&path, "error", Some(METER_A));
}

#[test]
fn a_new_app_run_adds_to_the_log_and_keeps_what_is_there() {
    let path = log_path("append");
    std::fs::write(&path, "2026-10-30T10:00:00Z earlier run\n").unwrap();
    let mut rig = rig_logging_to(&path);
    let (_rx, _) = stream(&mut rig, METER_A);
    wait_line(&path, "connect", Some(METER_A));
    assert_eq!(lines(&path)[0], "2026-10-30T10:00:00Z earlier run");
    rig.m.disconnect_for(METER_A).unwrap();
}

#[test]
fn the_app_logs_to_a_file_named_app_log() {
    let source = include_str!("../src/lib.rs");
    assert!(source.contains("\"app.log\""), "lib.rs names app.log");
    assert!(source.contains("set_app_log"), "lib.rs wires the log in");
}
