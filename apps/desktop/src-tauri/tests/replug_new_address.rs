//! AC-AXHK2J.4: a replug that lands on a new bus address. `listed_entry` is
//! how the real opener finds a listing in what the OS lists at open time;
//! the fake listing here stands in for the OS. Every serial is synthetic.

mod common;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::*;
use open_powerz_desktop_lib::devices::{DeviceListing, DeviceManager, Opener, listed_entry};
use open_powerz_desktop_lib::worker::{BackendEvent, Status};
use powerz_protocol::transport::{self, Transport, discovery::DeviceEntry, discovery::Model};

const SERIAL_A: &str = "SERIAL-ADDRESS-A";
const SERIAL_B: &str = "SERIAL-ADDRESS-B";
const METER_A: &str = "usb:1:7";

fn meter(serial: Option<&str>, bus: &str, address: u8) -> DeviceEntry {
    DeviceEntry {
        model: Model::Km003c,
        pid: 0x0063,
        serial: serial.map(str::to_owned),
        product: Some("POWER-Z KM003C".to_owned()),
        interface_string: Some("APP".to_owned()),
        bus: bus.to_owned(),
        address,
    }
}

fn listing(serial: Option<&str>, bus: &str, address: u8) -> DeviceListing {
    DeviceListing {
        id: format!("usb:{bus}:{address}"),
        label: "KM003C".to_owned(),
        model: "KM003C".to_owned(),
        serial: serial.map(str::to_owned),
        bus: Some(bus.to_owned()),
        address: Some(address),
        demo_scenario: None,
        mode: meter(serial, bus, address).mode(),
    }
}

#[test]
fn a_known_serial_finds_the_meter_at_its_new_address() {
    let os = [meter(Some(SERIAL_B), "1", 3), meter(Some(SERIAL_A), "2", 9)];
    let found = listed_entry(&listing(Some(SERIAL_A), "1", 7), &os);
    assert_eq!(found.map(|e| e.address), Some(9));
}

#[test]
fn another_meter_on_the_old_address_is_not_this_one() {
    let os = [meter(Some(SERIAL_B), "1", 7)];
    assert_eq!(listed_entry(&listing(Some(SERIAL_A), "1", 7), &os), None);
}

#[test]
fn without_a_serial_the_bus_and_address_decide() {
    let os = [meter(None, "1", 9), meter(None, "1", 7)];
    let found = listed_entry(&listing(None, "1", 7), &os);
    assert_eq!(found.map(|e| e.address), Some(7));
    assert_eq!(listed_entry(&listing(None, "1", 8), &os), None);
}

/// The simulator behind a USB plug: once pulled, every request fails.
struct Plugged {
    sim: SimDevice,
    plugged: Arc<AtomicBool>,
}

impl Transport for Plugged {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, transport::Error> {
        if !self.plugged.load(Ordering::SeqCst) {
            return Err(transport::Error::Disconnected);
        }
        self.sim.transact(request, timeout)
    }

    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, transport::Error> {
        if !self.plugged.load(Ordering::SeqCst) {
            return Err(transport::Error::Disconnected);
        }
        self.sim.read(timeout)
    }
}

/// Opens what [`listed_entry`] finds in the fake OS list, as `AppOpener`
/// does in the real one, and remembers the address it opened.
struct ListingOpener {
    clock: SimClock,
    os: Arc<Mutex<Vec<DeviceEntry>>>,
    plugged: Arc<AtomicBool>,
    opened: Arc<Mutex<Vec<u8>>>,
}

impl Opener for ListingOpener {
    type Transport = Plugged;

    fn open(&self, device: &DeviceListing) -> Result<Plugged, transport::Error> {
        let os = self.os.lock().unwrap();
        let entry = listed_entry(device, &os).ok_or(transport::Error::NotFound)?;
        self.opened.lock().unwrap().push(entry.address);
        Ok(Plugged {
            sim: SimDevice::with_clock(Persona::Bulk, Scenario::Load5v1a, self.clock.clone()),
            plugged: Arc::clone(&self.plugged),
        })
    }
}

fn recv_until(
    rx: &mpsc::Receiver<BackendEvent>,
    want: impl Fn(&BackendEvent) -> bool,
) -> Vec<BackendEvent> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut seen = Vec::new();
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let event = rx.recv_timeout(left).expect("the event never came");
        let done = want(&event);
        seen.push(event);
        if done {
            return seen;
        }
    }
}

fn session_id(events: &[BackendEvent]) -> Option<u64> {
    events.iter().find_map(|e| match e {
        BackendEvent::Status(Status::Connected(info)) => Some(info.session_id),
        _ => None,
    })
}

#[test]
fn the_same_session_follows_the_meter_when_the_address_moves_again_before_the_open() {
    let clock = clock();
    let os = Arc::new(Mutex::new(vec![meter(Some(SERIAL_A), "1", 7)]));
    let plugged = Arc::new(AtomicBool::new(true));
    let opened = Arc::new(Mutex::new(Vec::new()));
    let list = Arc::clone(&os);
    let mut m = DeviceManager::new(
        move || Ok(list.lock().unwrap().clone()),
        ListingOpener {
            clock: clock.clone(),
            os: Arc::clone(&os),
            plugged: Arc::clone(&plugged),
            opened: Arc::clone(&opened),
        },
        clock.clone(),
    );
    let (tx, rx) = mpsc::channel();
    m.connect(METER_A, 50, move |e: BackendEvent| {
        let _ = tx.send(e);
    })
    .expect("the meter connects");
    let before = recv_until(
        &rx,
        |e| matches!(e, BackendEvent::Batch(b) if !b.samples.is_empty()),
    );
    let first = session_id(&before).expect("never connected");

    plugged.store(false, Ordering::SeqCst);
    os.lock().unwrap().clear();
    recv_until(&rx, |e| {
        matches!(e, BackendEvent::Status(Status::Waiting { .. }))
    });

    // The watcher saw address 9, and by the open the OS lists 11.
    let seen = vec![listing(Some(SERIAL_A), "1", 9)];
    plugged.store(true, Ordering::SeqCst);
    *os.lock().unwrap() = vec![meter(Some(SERIAL_A), "1", 11)];
    clock.advance(Duration::from_secs(4));
    assert!(m.devices_changed(&seen), "the meter was not reconnected");
    let after = recv_until(&rx, |e| {
        matches!(e, BackendEvent::Status(Status::Connected(_)))
    });
    assert_eq!(session_id(&after), Some(first), "the same session");
    assert_eq!(*opened.lock().unwrap(), vec![7, 11]);
    assert_eq!(m.waiting_for(), None);
}
