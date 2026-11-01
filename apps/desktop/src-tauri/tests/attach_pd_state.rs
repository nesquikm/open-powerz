//! AC-AXHK2K.7: the attach snapshot says whether that meter's PD capture
//! runs, so a reloaded page reads it from the backend instead of assuming it
//! is idle. Demo meters only; no USB discovery.

use std::sync::mpsc;
use std::time::Duration;

use open_powerz_desktop_lib::devices::{AppOpener, DeviceManager};
use open_powerz_desktop_lib::worker::{BackendEvent, Status};
use powerz_protocol::session::SystemClock;

const CAPTURING: &str = "demo:pd-9v-2a";
const IDLE: &str = "demo:load-5v-1a";

type Manager = DeviceManager<AppOpener, SystemClock>;

fn manager() -> Manager {
    DeviceManager::new(|| Ok(Vec::new()), AppOpener, SystemClock)
}

/// Connect `id` and wait for its `connected` status.
fn connect(m: &mut Manager, id: &str) {
    let (tx, rx) = mpsc::channel();
    m.connect(id, 50, move |event: BackendEvent| {
        if matches!(event, BackendEvent::Status(Status::Connected(_))) {
            let _ = tx.send(());
        }
    })
    .expect("the demo meter connects");
    rx.recv_timeout(Duration::from_secs(10))
        .expect("the demo meter reports connected");
}

fn snapshot_capturing(m: &Manager, id: &str) -> bool {
    m.attacher_for(id)
        .expect("a running session")
        .attach(|_: BackendEvent| {})
        .expect("a snapshot")
        .pd_capturing
}

#[test]
fn the_snapshot_follows_each_meters_capture() {
    let mut m = manager();
    connect(&mut m, IDLE);
    connect(&mut m, CAPTURING);
    assert!(!snapshot_capturing(&m, CAPTURING), "no capture runs yet");

    let control = m.control_for(CAPTURING).expect("the meter runs");
    control.start_pd().expect("the capture starts");
    assert!(snapshot_capturing(&m, CAPTURING), "its capture runs");
    assert!(!snapshot_capturing(&m, IDLE), "the other meter's does not");

    control.stop_pd().expect("the capture stops");
    assert!(!snapshot_capturing(&m, CAPTURING), "its capture stopped");

    for id in m.connected_ids() {
        let _ = m.disconnect_for(&id);
    }
}
