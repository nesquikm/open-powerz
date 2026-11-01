//! A firmware update refused for its typed confirmation or its file releases
//! nothing (FR AXHK2F, AC .2): both are checked before any session is let go,
//! and before the manager lock is taken, so a slow file stalls no other
//! command. `checked_start` below is what the `firmware_flash` command does,
//! which the last tests pin.
//!
//! Only the simulator is used; no real device is opened. The container is
//! synthetic and every identifier is the simulator's made-up one.

mod common;
#[path = "../../../../crates/powerz-protocol/tests/support/fw_container.rs"]
mod fw_container;

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::*;
use fw_container::*;
use open_powerz_desktop_lib::devices::{
    DeviceListing, DeviceManager, FlashHold, FlashLease, Opener,
};
use open_powerz_desktop_lib::firmware_flash::check_confirmation;
use open_powerz_desktop_lib::ui_error::{ErrorKind, UiError};
use open_powerz_desktop_lib::worker::BackendEvent;
use powerz_protocol::transport::{self, discovery::DeviceEntry, discovery::Model};

const VERSION: &str = "2.0.7";

/// Hands out the simulated meter behind a logging pipe and keeps every log.
#[derive(Clone)]
struct LoggingOpener {
    clock: SimClock,
    logs: Arc<Mutex<Vec<Arc<Mutex<Log>>>>>,
}

impl Opener for LoggingOpener {
    type Transport = Pipe;
    fn open(&self, _device: &DeviceListing) -> Result<Pipe, transport::Error> {
        let dev = SimDevice::with_clock(Persona::Bulk, Scenario::Load5v1a, self.clock.clone());
        let pipe = Pipe::new(dev, &self.clock, PipeOpts::default());
        self.logs.lock().unwrap().push(Arc::clone(&pipe.log));
        Ok(pipe)
    }
}

/// A real meter with no serial at `address`; every value is synthetic.
fn no_serial(address: u8) -> DeviceEntry {
    DeviceEntry {
        model: Model::Km003c,
        pid: 0x0063,
        serial: None,
        product: Some("POWER-Z KM003C".to_owned()),
        interface_string: Some("APP".to_owned()),
        bus: "1".to_owned(),
        address,
    }
}

fn temp_file(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("firmware-consent");
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir.join(name)
}

/// A KM003C 2.0.7 container.
fn firmware(name: &str) -> PathBuf {
    let path = temp_file(name);
    std::fs::write(&path, ContainerBuilder::new().build()).expect("write container");
    path
}

/// Two meters that cannot be told apart; the first streams and records.
struct Rig {
    m: DeviceManager<LoggingOpener, SimClock>,
    logs: Arc<Mutex<Vec<Arc<Mutex<Log>>>>>,
    _events: mpsc::Receiver<BackendEvent>,
    session: String,
    target: String,
}

fn recording_rig(name: &str) -> Rig {
    let clock = clock();
    let logs = Arc::new(Mutex::new(Vec::new()));
    let opener = LoggingOpener {
        clock: clock.clone(),
        logs: Arc::clone(&logs),
    };
    let mut m = DeviceManager::new(
        || Ok(vec![no_serial(4), no_serial(5)]),
        opener,
        clock.clone(),
    );
    let (session, target) = ("usb:1:4".to_owned(), "usb:1:5".to_owned());
    let (tx, rx) = mpsc::channel();
    m.connect(&session, 50, move |e| {
        let _ = tx.send(e);
    })
    .unwrap_or_else(|e| panic!("connect: {e:?}"));
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let event = rx.recv_timeout(left).expect("the meter never streamed");
        if matches!(&event, BackendEvent::Batch(b) if !b.samples.is_empty()) {
            break;
        }
    }
    m.start_recording_for(&session, &temp_file(&format!("{name}.csv")))
        .unwrap_or_else(|e| panic!("record: {e:?}"));
    Rig {
        m,
        logs,
        _events: rx,
        session,
        target,
    }
}

impl Rig {
    /// The `firmware_flash` command's start.
    fn checked_start(
        &mut self,
        agreed: &[FlashHold],
        path: &Path,
        confirmation: &str,
    ) -> Result<FlashLease, UiError> {
        check_confirmation(path, confirmation)?;
        let target = self.target.clone();
        self.m.begin_flash(&target, agreed)
    }

    /// A start with full consent must be refused, with everything as it was.
    fn assert_refused_with_full_consent(&mut self, path: &Path, confirmation: &str) -> String {
        let consent = self.m.flash_holds(&self.target);
        assert_eq!(consent.len(), 1, "{consent:?}");
        assert!(consent[0].recording);

        let e = match self.checked_start(&consent, path, confirmation) {
            Err(e) => e,
            Ok(lease) => panic!("the update started on {}", lease.device().id),
        };

        assert_eq!(e.kind, ErrorKind::Usage, "{e:?}");
        assert_eq!(self.m.running_ids(), vec![self.session.clone()]);
        assert!(!self.m.flashing(), "the update flag stayed up");
        assert_eq!(self.m.flash_status(), None, "an update is still reported");
        assert_eq!(
            self.m.flash_holds(&self.target),
            consent,
            "the recording ended"
        );
        for log in self.logs.lock().unwrap().iter() {
            let log = log.lock().unwrap();
            assert!(!log.closed, "the meter's pipe was closed");
            for (_, frame) in &log.frames {
                let ty = frame.first().map_or(0, |b| b & 0x7F);
                assert!(SAFE_TYPES.contains(&ty), "unsafe frame: {frame:02X?}");
            }
        }
        e.message
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        for id in self.m.connected_ids() {
            let _ = self.m.disconnect_for(&id);
        }
    }
}

#[test]
fn a_wrong_confirmation_with_full_consent_releases_nothing() {
    let mut rig = recording_rig("wrong-confirmation");
    let path = firmware("wrong-confirmation.mencrypt");

    let message = rig.assert_refused_with_full_consent(&path, "9.9.9");

    assert!(message.contains(VERSION), "names the version: {message}");
}

#[test]
fn a_file_that_fails_inspection_with_full_consent_releases_nothing() {
    let mut rig = recording_rig("bad-file");
    let path = temp_file("no-such-file.mencrypt");

    let message = rig.assert_refused_with_full_consent(&path, VERSION);

    assert!(!message.is_empty());
}

#[test]
fn the_right_confirmation_with_full_consent_starts_the_update() {
    let mut rig = recording_rig("right-confirmation");
    let path = firmware("right-confirmation.mencrypt");
    let consent = rig.m.flash_holds(&rig.target);

    let lease = rig
        .checked_start(&consent, &path, VERSION)
        .unwrap_or_else(|e| panic!("refused: {e:?}"));

    assert_eq!(lease.device().id, rig.target);
    assert_eq!(rig.m.running_ids(), Vec::<String>::new());
}

/// The body of the Tauri command `name`.
fn command_body(name: &str) -> &'static str {
    let source = include_str!("../src/lib.rs");
    let (_, rest) = source
        .split_once(&format!("async fn {name}("))
        .expect("the command exists");
    let (command, _) = rest.split_once("\n}\n").expect("the command ends");
    command
}

/// The commands have no test runtime of their own, so this pins that the
/// file is checked first, without the manager lock, and the start then goes
/// through consent.
#[test]
fn the_firmware_flash_command_checks_the_file_before_it_takes_the_manager_lock() {
    let command = command_body("firmware_flash");

    let check = command.find("check_confirmation(").expect("no check");
    let lock = command.find(".manager()").expect("no manager lock");
    assert!(check < lock, "{command}");
    assert!(command.contains(".begin_flash("), "{command}");
    assert!(!command.contains(".begin_confirmed_flash("), "{command}");
}

/// A recording start registers with the manager before it asks the worker,
/// under the lock an update's start takes (AC-AXHK2F.5).
#[test]
fn the_start_recording_command_registers_its_start_under_the_manager_lock() {
    let command = command_body("start_recording");

    let lock = command.find(".manager()").expect("no manager lock");
    let register = command
        .find(".recording_start_for(")
        .expect("the start is not registered");
    let ask = command.find(".record_to(").expect("no recording asked for");
    assert!(lock < register && register < ask, "{command}");
    assert!(!command.contains("control(&state"), "{command}");
}
