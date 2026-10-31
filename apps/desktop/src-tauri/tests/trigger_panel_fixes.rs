//! FR 4PJRVH, the backend side: a stream error that is not a device loss
//! leaves an open trigger session alone (AC .5), the console listens long
//! enough after `entry` lines (AC .3), and a detection carries the D+ / D-
//! hint (AC .4). Every serial here is synthetic.

mod common;

use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use common::*;
use open_powerz_desktop_lib::devices::{DeviceListing, DeviceManager, Opener};
use open_powerz_desktop_lib::trigger::{
    ConsoleDirection, TriggerProtocol, TriggerRequest, TriggerWorker,
};
use open_powerz_desktop_lib::worker::{BackendEvent, Status};
use powerz_protocol::frame;
use powerz_protocol::transport::{self, Transport, discovery::DeviceEntry, discovery::Model};
use powerz_sim::{SimCharger, SimPdm, SimPdmPort};

const SERIAL: &str = "SERIAL-TRIGGER-A";
const METER: &str = "usb:1:7";
const GET_DATA: u8 = 0x0C;

/// What the meter answers to GET_DATA while something is wrong with it.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Fault {
    /// ACCEPT where only PUT_DATA is allowed: `Error::Protocol`.
    Protocol,
    /// A PUT_DATA whose section claims more bytes than it has: the reply
    /// cannot be decoded.
    Decode,
}

impl Fault {
    fn reply(self, request: &[u8]) -> Vec<u8> {
        match self {
            Fault::Protocol => vec![0x05, request[1], 0, 0],
            // Attribute ADC, size 100, no payload.
            Fault::Decode => vec![0x41, request[1], 0x10, 0, 0x01, 0, 0, 0x19],
        }
    }
}

/// The simulator behind a link that stays up while its answers go wrong.
struct Faulty {
    sim: SimDevice,
    fault: Arc<Mutex<Option<Fault>>>,
    served: Arc<AtomicUsize>,
}

impl Transport for Faulty {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, transport::Error> {
        let fault = *self.fault.lock().unwrap();
        if let Some(fault) = fault
            && request.first() == Some(&GET_DATA)
        {
            self.served.fetch_add(1, Ordering::SeqCst);
            return Ok(fault.reply(request));
        }
        self.sim.transact(request, timeout)
    }

    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, transport::Error> {
        self.sim.read(timeout)
    }
}

struct FaultyOpener {
    clock: SimClock,
    fault: Arc<Mutex<Option<Fault>>>,
    served: Arc<AtomicUsize>,
    opens: Arc<AtomicUsize>,
}

impl Opener for FaultyOpener {
    type Transport = Faulty;
    fn open(&self, _device: &DeviceListing) -> Result<Faulty, transport::Error> {
        self.opens.fetch_add(1, Ordering::SeqCst);
        Ok(Faulty {
            sim: SimDevice::with_clock(Persona::Bulk, Scenario::Load5v1a, self.clock.clone()),
            fault: Arc::clone(&self.fault),
            served: Arc::clone(&self.served),
        })
    }
}

struct Rig {
    m: DeviceManager<FaultyOpener, SimClock>,
    clock: SimClock,
    fault: Arc<Mutex<Option<Fault>>>,
    served: Arc<AtomicUsize>,
    opens: Arc<AtomicUsize>,
    events: mpsc::Receiver<BackendEvent>,
}

impl Rig {
    /// A meter connected at 50 SPS and streaming.
    fn streaming() -> Rig {
        let clock = clock();
        let fault = Arc::new(Mutex::new(None));
        let served = Arc::new(AtomicUsize::new(0));
        let opens = Arc::new(AtomicUsize::new(0));
        let mut m = DeviceManager::new(
            || {
                Ok(vec![DeviceEntry {
                    model: Model::Km003c,
                    pid: 0x0063,
                    serial: Some(SERIAL.to_owned()),
                    product: Some("POWER-Z KM003C".to_owned()),
                    interface_string: Some("APP".to_owned()),
                    bus: "1".to_owned(),
                    address: 7,
                }])
            },
            FaultyOpener {
                clock: clock.clone(),
                fault: Arc::clone(&fault),
                served: Arc::clone(&served),
                opens: Arc::clone(&opens),
            },
            clock.clone(),
        );
        let (tx, events) = mpsc::channel();
        m.connect(METER, 50, move |e| {
            let _ = tx.send(e);
        })
        .expect("connect");
        let mut rig = Rig {
            m,
            clock,
            fault,
            served,
            opens,
            events,
        };
        rig.until(|e| matches!(e, BackendEvent::Batch(b) if !b.samples.is_empty()));
        rig
    }

    /// A trigger session at 9 V on the connected meter.
    fn triggering(&mut self) -> SimPdm {
        let clock = SimClock::new(T0);
        let pdm = SimPdm::new(SimCharger::Epr140W, clock.clone());
        let worker = TriggerWorker::spawn(pdm.open_port(), clock).expect("trigger session");
        self.m.install_trigger(worker).expect("install");
        let c = self.m.trigger_control().expect("control");
        c.list(TriggerProtocol::Pd).expect("list pd");
        c.request(
            TriggerRequest::Pd {
                position: 2,
                voltage_mv: None,
                current_ma: None,
            },
            true,
        )
        .expect("9 V");
        assert!(pdm.vbus_mv() > 8000, "the charger is at 9 V");
        pdm
    }

    /// Events until `want` matches one.
    fn until(&mut self, mut want: impl FnMut(&BackendEvent) -> bool) -> Vec<BackendEvent> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut got = Vec::new();
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.events.recv_timeout(left) {
                Ok(e) => {
                    let hit = want(&e);
                    got.push(e);
                    if hit {
                        return got;
                    }
                }
                Err(e) => panic!("event never arrived ({e:?}); got {got:?}"),
            }
        }
    }

    /// The meter starts answering with `fault`; returns once the stream has
    /// met it several times and settled.
    fn go_wrong(&mut self, fault: Fault) -> Vec<BackendEvent> {
        *self.fault.lock().unwrap() = Some(fault);
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut got = Vec::new();
        let ended = |got: &[BackendEvent]| {
            got.iter().any(|e| {
                matches!(
                    e,
                    BackendEvent::Status(Status::Disconnected { .. } | Status::Waiting { .. })
                )
            })
        };
        while self.served.load(Ordering::SeqCst) < 1 && Instant::now() < deadline {
            got.extend(self.events.try_iter());
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            self.served.load(Ordering::SeqCst) > 0,
            "the stream never met the fault"
        );
        let settle = Instant::now() + Duration::from_millis(300);
        while Instant::now() < settle && !ended(&got) {
            got.extend(self.events.try_iter());
            std::thread::sleep(Duration::from_millis(5));
        }
        std::thread::sleep(Duration::from_millis(50));
        got.extend(self.events.try_iter());
        got
    }

    /// What the hotplug watcher would hand the manager now.
    fn relist(&mut self) -> bool {
        let list = self.m.list_devices();
        self.m.devices_changed(&list)
    }
}

fn no_disconnect(events: &[BackendEvent]) {
    let bad: Vec<&BackendEvent> = events
        .iter()
        .filter(|e| {
            matches!(
                e,
                BackendEvent::Status(Status::Disconnected { .. } | Status::Waiting { .. })
            )
        })
        .collect();
    assert!(bad.is_empty(), "the panel would be told it ended: {bad:?}");
}

// ---- AC-4PJRVH.5: an error that is not a device loss keeps the PDM session ----

fn the_session_outlives(fault: Fault) {
    let mut rig = Rig::streaming();
    let pdm = rig.triggering();
    let writes = pdm.writes().len();
    let opens = rig.opens.load(Ordering::SeqCst);

    let events = rig.go_wrong(fault);

    no_disconnect(&events);
    assert!(rig.m.trigger_open(), "the trigger session was closed");
    assert!(pdm.pdm_active(), "pdm close was sent");
    assert!(pdm.vbus_mv() > 8000, "the charger left the request");
    let later = pdm.writes();
    assert!(
        later.get(writes..).unwrap_or_default().is_empty(),
        "commands were sent on the PDM session: {later:?}"
    );

    // No bulk CONNECT while it is open: it would silence PDM. [device]
    for _ in 0..3 {
        assert!(!rig.relist(), "a reconnect started");
        rig.clock.advance(Duration::from_secs(60));
    }
    assert_eq!(
        rig.opens.load(Ordering::SeqCst),
        opens,
        "the meter was reopened while PDM is open"
    );

    // Once it is closed and the meter answers again, the measurements return.
    *rig.fault.lock().unwrap() = None;
    rig.m
        .take_trigger()
        .expect("the trigger session")
        .close()
        .expect("close");
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut seen = false;
    while !seen && Instant::now() < deadline {
        rig.relist();
        seen = rig
            .events
            .try_iter()
            .any(|e| matches!(e, BackendEvent::Batch(b) if !b.samples.is_empty()));
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(seen, "the measurements never came back");
    let _ = rig.m.disconnect();
}

/// The scripted fault is what its name says: a well-formed PUT_DATA header
/// over a section that cannot be read.
#[test]
fn the_decode_fault_is_a_put_data_that_does_not_parse() {
    let reply = Fault::Decode.reply(&[GET_DATA, 9, 0, 0]);
    assert!(matches!(
        frame::classify_reply(&reply),
        Ok(frame::Reply::PutData(_))
    ));
    assert!(frame::parse_put_data(&reply).is_err());
}

/// Between the slot's emptying and `pdm close`, PDM is still on the line: a
/// fault there must not bring the bulk CONNECT that would silence it.
#[test]
fn a_fault_while_the_session_closes_starts_no_reconnect() {
    let mut rig = Rig::streaming();
    let pdm = rig.triggering();
    let closing = rig.m.take_trigger().expect("the trigger session");
    let opens = rig.opens.load(Ordering::SeqCst);

    let events = rig.go_wrong(Fault::Protocol);

    no_disconnect(&events);
    assert!(!rig.relist(), "a reconnect started");
    assert_eq!(
        rig.opens.load(Ordering::SeqCst),
        opens,
        "the meter reopened"
    );
    assert!(pdm.pdm_active(), "the session closed before its time");
    *rig.fault.lock().unwrap() = None;
    closing.close().expect("close");
    let _ = rig.m.disconnect();
}

/// The same before the slot fills: the handshake may be running.
#[test]
fn a_fault_while_the_session_opens_starts_no_reconnect() {
    let mut rig = Rig::streaming();
    let opener = rig.m.trigger_opener().expect("opener");
    let opens = rig.opens.load(Ordering::SeqCst);

    let events = rig.go_wrong(Fault::Decode);

    no_disconnect(&events);
    assert!(!rig.relist(), "a reconnect started");
    assert_eq!(
        rig.opens.load(Ordering::SeqCst),
        opens,
        "the meter reopened"
    );
    *rig.fault.lock().unwrap() = None;
    drop(opener);
    let _ = rig.m.disconnect();
}

#[test]
fn a_protocol_error_keeps_the_trigger_session_and_starts_no_reconnect() {
    the_session_outlives(Fault::Protocol);
}

#[test]
fn a_decode_error_keeps_the_trigger_session_and_starts_no_reconnect() {
    the_session_outlives(Fault::Decode);
}

#[test]
fn without_a_trigger_session_a_stream_error_still_ends_the_session() {
    let mut rig = Rig::streaming();
    *rig.fault.lock().unwrap() = Some(Fault::Protocol);
    rig.until(|e| matches!(e, BackendEvent::Status(Status::Disconnected { .. })));
}

// ---- AC-4PJRVH.3: the console's listen window after `entry` lines ----

/// A serial port that holds everything from the first `pdo:` of an `entry`
/// reply back for `delay` on the session's clock: the meter answers `ok` at
/// once and the block when the negotiation is done.
struct LatePdo {
    port: SimPdmPort,
    clock: SimClock,
    delay: Duration,
    held: Vec<u8>,
    release_at: Option<u64>,
}

impl Read for LatePdo {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut chunk = [0u8; 256];
        loop {
            match self.port.read(&mut chunk) {
                Ok(n) => self.held.extend_from_slice(&chunk[..n]),
                Err(e) if e.kind() == io::ErrorKind::TimedOut => break,
                Err(e) => return Err(e),
            }
        }
        let mut ready = self.held.len();
        if let Some(at) = self.release_at {
            if self.clock.now_ms() >= at {
                self.release_at = None;
            } else if let Some(i) = self.held.windows(4).position(|w| w == b"pdo:") {
                ready = i;
            }
        }
        if ready == 0 {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "nothing yet"));
        }
        let n = buf.len().min(ready);
        buf[..n].copy_from_slice(&self.held[..n]);
        self.held.drain(..n);
        Ok(n)
    }
}

impl Write for LatePdo {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.starts_with(b"entry pd") {
            let delay = u64::try_from(self.delay.as_millis()).unwrap_or(u64::MAX);
            self.release_at = Some(self.clock.now_ms() + delay);
        }
        self.port.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.port.flush()
    }
}

#[test]
fn the_console_catches_a_pdo_block_that_comes_late_after_entry_pd() {
    let clock = SimClock::new(T0);
    let pdm = SimPdm::new(SimCharger::Epr140W, clock.clone());
    let port = LatePdo {
        port: pdm.open_port(),
        clock: clock.clone(),
        // Inside the library's own 3 s bound for `entry pd`.
        delay: Duration::from_millis(2000),
        held: Vec::new(),
        release_at: None,
    };
    let worker = TriggerWorker::spawn(port, clock.clone()).expect("trigger session");
    let c = worker.control();

    let lines = c.console_send("entry pd".to_owned(), false).expect("sent");

    let block = lines
        .iter()
        .find(|l| matches!(l.direction, ConsoleDirection::Rx) && l.text.starts_with("pdo:"));
    assert!(
        block.is_some_and(|l| l.block.is_some()),
        "no pdo: block in the transcript: {lines:?}"
    );
    let _ = worker.close();
}

#[test]
fn other_console_lines_keep_the_short_window() {
    let clock = SimClock::new(T0);
    let pdm = SimPdm::new(SimCharger::Epr140W, clock.clone());
    let worker = TriggerWorker::spawn(pdm.open_port(), clock.clone()).expect("trigger session");
    let c = worker.control();
    let before = clock.now_ms();

    c.console_send("pdm parm".to_owned(), false).expect("sent");

    assert!(
        clock.now_ms() - before < 1000,
        "pdm parm listened for {} ms",
        clock.now_ms() - before
    );
    let _ = worker.close();
}

// ---- AC-4PJRVH.4: the D+ / D- hint of a detection ----

#[test]
fn a_detection_carries_the_d_plus_d_minus_hint_the_cli_prints() {
    let mut rig = Rig::streaming();
    let clock = SimClock::new(T0);
    let pdm = SimPdm::new(SimCharger::Epr140W, clock.clone());
    let worker = TriggerWorker::spawn(pdm.open_port(), clock).expect("trigger session");
    rig.m.install_trigger(worker).expect("install");
    let c = rig.m.trigger_control().expect("control");

    let detection = c.detect().expect("detect");

    let json = serde_json::to_value(&detection).expect("serializes");
    let hint = &json["hint"];
    let label = hint["label"].as_str().or(hint.as_str());
    let label = label.unwrap_or_else(|| panic!("no hint in {json}"));
    assert!(label.starts_with("hint: "), "{label}");
    assert!(label.contains("(D+ ") && label.contains(", D- "), "{label}");
    let _ = rig.m.disconnect();
}
