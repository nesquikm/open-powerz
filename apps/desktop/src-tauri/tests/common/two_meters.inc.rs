// Two simulated meters behind their own plugs, included by the tests of
// FR 4PJRVM (same rig as multi_meter.rs). Every serial here is synthetic.

#[allow(unused_imports)]
use std::sync::atomic::{AtomicBool, Ordering};
#[allow(unused_imports)]
use std::sync::mpsc;
#[allow(unused_imports)]
use std::sync::{Arc, Mutex};
#[allow(unused_imports)]
use std::time::{Duration, Instant};

#[allow(unused_imports)]
use open_powerz_desktop_lib::devices::{DeviceListing, DeviceManager, Opener};
#[allow(unused_imports)]
use open_powerz_desktop_lib::worker::{BackendEvent, Status};
#[allow(unused_imports)]
use powerz_protocol::transport::{self, Transport, discovery::DeviceEntry, discovery::Model};
#[allow(unused_imports)]
use powerz_sim::PdScenario;

const SERIAL_A: &str = "SERIAL-MULTI-A";
const SERIAL_B: &str = "SERIAL-MULTI-B";
const METER_A: &str = "usb:1:7";
const METER_B: &str = "usb:1:8";
fn meter(serial: &str, bus: &str, address: u8) -> DeviceEntry {
    DeviceEntry {
        model: Model::Km003c,
        pid: 0x0063,
        serial: Some(serial.to_owned()),
        product: Some("POWER-Z KM003C".to_owned()),
        interface_string: Some("APP".to_owned()),
        bus: bus.to_owned(),
        address,
    }
}

/// The simulator behind one USB plug: once pulled, every request fails.
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

/// Opens the simulated meter a listing names (by serial): meter A plays
/// 5 V / 1 A, meter B 9 V / 2 A, each reporting its own serial and each
/// behind its own plug.
struct TwoMeterOpener {
    clock: SimClock,
    plug_a: Arc<AtomicBool>,
    plug_b: Arc<AtomicBool>,
    opened: Arc<Mutex<Vec<DeviceListing>>>,
}

impl Opener for TwoMeterOpener {
    type Transport = Plugged;
    fn open(&self, device: &DeviceListing) -> Result<Plugged, transport::Error> {
        self.opened.lock().unwrap().push(device.clone());
        let (scenario, plug, serial) = match device.serial.as_deref() {
            Some(SERIAL_A) => (Scenario::Load5v1a, &self.plug_a, SERIAL_A),
            Some(SERIAL_B) => (Scenario::Pd9v2a, &self.plug_b, SERIAL_B),
            _ => return Err(transport::Error::NotFound),
        };
        if !plug.load(Ordering::SeqCst) {
            return Err(transport::Error::NotFound);
        }
        Ok(Plugged {
            sim: SimDevice::with_clock(Persona::Bulk, scenario, self.clock.clone())
                .with_serial(serial)
                .with_pd_scenario(PdScenario::Negotiation),
            plugged: Arc::clone(plug),
        })
    }
}

/// A manager over two plugs, with the OS device list under the test's control.
struct Rig {
    m: DeviceManager<TwoMeterOpener, SimClock>,
    attached: Arc<Mutex<Vec<DeviceEntry>>>,
    plug_a: Arc<AtomicBool>,
    opened: Arc<Mutex<Vec<DeviceListing>>>,
}

impl Rig {
    fn new() -> Rig {
        let clock = clock();
        let attached = Arc::new(Mutex::new(vec![
            meter(SERIAL_A, "1", 7),
            meter(SERIAL_B, "1", 8),
        ]));
        let plug_a = Arc::new(AtomicBool::new(true));
        let plug_b = Arc::new(AtomicBool::new(true));
        let opened = Arc::new(Mutex::new(Vec::new()));
        let listed = Arc::clone(&attached);
        let m = DeviceManager::new(
            move || Ok(listed.lock().unwrap().clone()),
            TwoMeterOpener {
                clock: clock.clone(),
                plug_a: Arc::clone(&plug_a),
                plug_b,
                opened: Arc::clone(&opened),
            },
            clock,
        );
        Rig {
            m,
            attached,
            plug_a,
            opened,
        }
    }

    /// Pull meter A's cable: its pipe fails at once and the OS stops listing
    /// it; meter B stays.
    fn unplug_a(&mut self) {
        self.plug_a.store(false, Ordering::SeqCst);
        self.attached
            .lock()
            .unwrap()
            .retain(|e| e.serial.as_deref() != Some(SERIAL_A));
    }

    /// Plug meter A back in at a new address and let the watcher report it.
    fn replug_a(&mut self) -> bool {
        self.plug_a.store(true, Ordering::SeqCst);
        self.attached.lock().unwrap().push(meter(SERIAL_A, "1", 9));
        let list = self.m.list_devices();
        self.m.devices_changed(&list)
    }

    fn opens_of(&self, serial: &str) -> usize {
        self.opened
            .lock()
            .unwrap()
            .iter()
            .filter(|d| d.serial.as_deref() == Some(serial))
            .count()
    }
}

fn channel_sink() -> (
    mpsc::Receiver<BackendEvent>,
    impl FnMut(BackendEvent) + Send + 'static,
) {
    let (tx, rx) = mpsc::channel();
    (rx, move |e| {
        let _ = tx.send(e);
    })
}

/// Receive events until `want` matches one; returns everything received.
fn recv_until(
    rx: &mpsc::Receiver<BackendEvent>,
    mut want: impl FnMut(&BackendEvent) -> bool,
) -> Vec<BackendEvent> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut got = Vec::new();
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(e) => {
                let hit = want(&e);
                got.push(e);
                if hit {
                    return got;
                }
            }
            Err(e) => panic!("event never arrived ({e:?}); got {} events", got.len()),
        }
    }
}

/// Everything already queued, without waiting.
fn drain(rx: &mpsc::Receiver<BackendEvent>) -> Vec<BackendEvent> {
    rx.try_iter().collect()
}

fn is_samples(e: &BackendEvent) -> bool {
    matches!(e, BackendEvent::Batch(b) if !b.samples.is_empty())
}

fn is_connected(e: &BackendEvent) -> bool {
    matches!(e, BackendEvent::Status(Status::Connected(_)))
}

/// Connect meter `id` at 50 SPS and wait for its first samples.
fn stream(rig: &mut Rig, id: &str) -> (mpsc::Receiver<BackendEvent>, Vec<BackendEvent>) {
    let (rx, sink) = channel_sink();
    rig.m
        .connect(id, 50, sink)
        .unwrap_or_else(|e| panic!("connect {id} failed: {}", e.message));
    let first = recv_until(&rx, is_samples);
    (rx, first)
}

/// Both meters streaming: (A's events, B's events).
fn stream_both(rig: &mut Rig) -> (mpsc::Receiver<BackendEvent>, mpsc::Receiver<BackendEvent>) {
    let (rx_a, _) = stream(rig, METER_A);
    let (rx_b, _) = stream(rig, METER_B);
    (rx_a, rx_b)
}
