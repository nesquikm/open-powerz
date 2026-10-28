//! Device listing, hotplug and the device manager (FR VHY378).
//!
//! Listing reads the OS device list only; it never opens a meter. The demo
//! device adds one entry per simulator scenario.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use powerz_protocol::adcnew::{Frame as AdcnewFrame, Request as AdcnewRequest};
use powerz_protocol::decode::Rate;
use powerz_protocol::firmware::DeviceMode;
use powerz_protocol::session::Clock;
use powerz_protocol::transport::bulk::BulkTransport;
use powerz_protocol::transport::cdc::{CdcTransport, SerialCdcTransport};
use powerz_protocol::transport::discovery::{self, DeviceEntry, Model};
use powerz_protocol::transport::guard::GuardedTransport;
use powerz_protocol::transport::{self, Transport};
use powerz_sim::{PdScenario, Persona, Scenario, SimDevice};
use serde::Serialize;

use crate::history::{HistoryGap, Series, SeriesBuckets};
use crate::recording::{ExportProgress, RecordingSummary};
use crate::stats::StatsSnapshot;
use crate::store::SessionStore;
use crate::ui_error::UiError;
use crate::worker::{
    self, BackendEvent, ConnectedInfo, EventSink, Gap, GapKind, MeasurementSource, Status,
    WorkerConfig, WorkerHandle,
};

/// One entry of the device picker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeviceListing {
    /// Stable id the UI passes back to `connect`.
    pub id: String,
    /// Human-readable label.
    pub label: String,
    /// Model name, e.g. `KM003C`.
    pub model: String,
    /// USB serial number, if the OS reported one.
    pub serial: Option<String>,
    /// USB bus id (real meters only).
    pub bus: Option<String>,
    /// USB device address (real meters only).
    pub address: Option<u8>,
    /// Simulator scenario name (demo entries only).
    pub demo_scenario: Option<String>,
    /// What the entry runs, sent to the UI by name (FR V9SK52, AC .5).
    /// Demo entries are always in application mode.
    #[serde(serialize_with = "serialize_mode")]
    pub mode: DeviceMode,
}

/// Why a bootloader-mode meter is not connected (FR V9SK52, AC .5).
const BOOTLOADER_REFUSAL: &str =
    "This meter is in bootloader mode. Restart it into the application firmware to connect.";

impl DeviceListing {
    /// The bootloader answers none of the measurement requests: refuse it
    /// before opening anything, for a user connect and an automatic reconnect
    /// alike (FR V9SK52, AC .5).
    fn ensure_connectable(&self) -> Result<(), UiError> {
        if self.mode == DeviceMode::Bootloader {
            return Err(UiError::usage(BOOTLOADER_REFUSAL));
        }
        Ok(())
    }
}

fn serialize_mode<S: serde::Serializer>(mode: &DeviceMode, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(mode.name())
}

/// The label suffix naming a real meter's mode; only a bootloader-mode
/// meter gets one.
fn mode_suffix(mode: DeviceMode) -> &'static str {
    match mode {
        DeviceMode::Application | DeviceMode::Unknown => "",
        DeviceMode::Bootloader => " (bootloader mode)",
    }
}

fn model_name(model: Model) -> String {
    match model {
        Model::Km003c => "KM003C".to_owned(),
        Model::Km002c => "KM002C".to_owned(),
        Model::Unknown(pid) => format!("POWER-Z (PID 0x{pid:04X})"),
    }
}

fn usb_listing(entry: &DeviceEntry) -> DeviceListing {
    let model = model_name(entry.model);
    let mode = entry.mode();
    let name = match &entry.serial {
        Some(serial) => format!("{model} — {serial}"),
        None => format!("{model} — bus {} address {}", entry.bus, entry.address),
    };
    let label = format!("{name}{}", mode_suffix(mode));
    DeviceListing {
        id: format!("usb:{}:{}", entry.bus, entry.address),
        label,
        model,
        serial: entry.serial.clone(),
        bus: Some(entry.bus.clone()),
        address: Some(entry.address),
        demo_scenario: None,
        mode,
    }
}

fn demo_listing(scenario: Scenario) -> DeviceListing {
    let name = scenario.name();
    DeviceListing {
        id: format!("demo:{name}"),
        label: format!("Demo device — {name}"),
        model: "KM003C".to_owned(),
        serial: Some(powerz_sim::SIM_SERIAL.to_owned()),
        bus: None,
        address: None,
        demo_scenario: Some(name.to_owned()),
        mode: DeviceMode::Application,
    }
}

/// Picker entries from a discovery result: every attached meter, then one
/// demo entry per simulator scenario. A failed discovery still lists the
/// demo device.
pub fn list_devices_from(
    discovered: Result<Vec<DeviceEntry>, transport::Error>,
) -> Vec<DeviceListing> {
    let usb = discovered.unwrap_or_default();
    usb.iter()
        .map(usb_listing)
        .chain(Scenario::ALL.into_iter().map(demo_listing))
        .collect()
}

/// An open recording as a re-attached page picks it up (FR VEDDFD).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RecordingSnapshot {
    /// Rows written so far.
    pub rows: u64,
    /// Time since the recording started, ms.
    pub elapsed_ms: u64,
}

/// The running session as a new page (after a reload) picks it up (FR VEDDFD).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AttachedSession {
    /// The picker id the session was connected with.
    pub device_id: String,
    /// Model name, e.g. `KM003C`.
    pub model: String,
    /// Firmware version the meter reported.
    pub firmware_version: String,
    /// Serial number, if known.
    pub serial: Option<String>,
    /// Current queue rate, once the queue runs.
    pub rate_sps: Option<u32>,
    /// The meter refused the asked rate and runs at its own.
    pub rate_locked: bool,
    /// Session id, as in the `connected` status.
    pub session_id: u64,
    /// The open recording, if any.
    pub recording: Option<RecordingSnapshot>,
}

/// Opens the pipe for a picked device.
pub trait Opener {
    /// The opened pipe.
    type Transport: Transport;
    /// Open `device`.
    fn open(&self, device: &DeviceListing) -> Result<Self::Transport, transport::Error>;

    /// Open the pipe of `device` that `source` streams over (FR 1HHT02).
    /// Default: the same pipe as [`Opener::open`].
    fn open_source(
        &self,
        device: &DeviceListing,
        source: MeasurementSource,
    ) -> Result<Self::Transport, transport::Error> {
        let _ = source;
        self.open(device)
    }
}

type Discover = Arc<dyn Fn() -> Result<Vec<DeviceEntry>, transport::Error> + Send + Sync>;

/// The pipe the app opens: the demo device, or a real meter over bulk or
/// (for the ADCNEW stream) its CDC serial port.
#[derive(Debug)]
pub enum AppTransport {
    /// The simulated KM003C.
    Demo(Box<SimDevice>),
    /// A real meter on its vendor bulk interface.
    Usb(GuardedTransport<BulkTransport>),
    /// A real meter on its CDC serial port.
    Cdc(GuardedTransport<SerialCdcTransport>),
}

impl Transport for AppTransport {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, transport::Error> {
        match self {
            AppTransport::Demo(t) => t.transact(request, timeout),
            AppTransport::Usb(t) => t.transact(request, timeout),
            AppTransport::Cdc(t) => t.transact(request, timeout),
        }
    }

    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, transport::Error> {
        match self {
            AppTransport::Demo(t) => t.read(timeout),
            AppTransport::Usb(t) => t.read(timeout),
            AppTransport::Cdc(t) => t.read(timeout),
        }
    }

    fn recover(&mut self) -> Result<(), transport::Error> {
        match self {
            AppTransport::Demo(t) => t.recover(),
            AppTransport::Usb(t) => t.recover(),
            AppTransport::Cdc(t) => t.recover(),
        }
    }

    fn adcnew_send(&mut self, request: AdcnewRequest) -> Result<(), transport::Error> {
        match self {
            AppTransport::Demo(t) => t.adcnew_send(request),
            AppTransport::Usb(t) => t.adcnew_send(request),
            AppTransport::Cdc(t) => t.adcnew_send(request),
        }
    }

    fn adcnew_frames(&mut self) -> Result<Vec<AdcnewFrame>, transport::Error> {
        match self {
            AppTransport::Demo(t) => t.adcnew_frames(),
            AppTransport::Usb(t) => t.adcnew_frames(),
            AppTransport::Cdc(t) => t.adcnew_frames(),
        }
    }
}

/// Opens what the picker lists: the demo device for a demo entry, the
/// meter at the entry's USB bus and address otherwise.
#[derive(Debug, Default, Clone, Copy)]
pub struct AppOpener;

impl Opener for AppOpener {
    type Transport = AppTransport;

    fn open(&self, device: &DeviceListing) -> Result<AppTransport, transport::Error> {
        self.open_source(device, MeasurementSource::Queue)
    }

    /// The queue opens the vendor bulk pipe, the CDC source the meter's
    /// serial port (found by its USB serial number); the demo device plays
    /// the matching simulator persona.
    fn open_source(
        &self,
        device: &DeviceListing,
        source: MeasurementSource,
    ) -> Result<AppTransport, transport::Error> {
        if let Some(name) = &device.demo_scenario {
            let scenario = Scenario::from_name(name).ok_or(transport::Error::NotFound)?;
            let sim = match source {
                // The demo device also plays the simulator's PD capture (FR TGE83C).
                MeasurementSource::Queue => SimDevice::new(Persona::Bulk, scenario)
                    .with_pd_scenario(PdScenario::Negotiation),
                MeasurementSource::Cdc => SimDevice::new(Persona::Cdc, scenario),
            };
            // A second demo meter reports a serial of its own (FR 1HHT03).
            let sim = match &device.serial {
                Some(serial) => sim.with_serial(serial),
                None => sim,
            };
            return Ok(AppTransport::Demo(Box::new(sim)));
        }
        if source == MeasurementSource::Cdc {
            let serial = device.serial.as_deref().ok_or(transport::Error::NotFound)?;
            return Ok(AppTransport::Cdc(CdcTransport::open(serial)?));
        }
        let (Some(bus), Some(address)) = (&device.bus, device.address) else {
            return Err(transport::Error::NotFound);
        };
        let entry = discovery::list_devices()?
            .into_iter()
            .find(|e| &e.bus == bus && e.address == address)
            .ok_or(transport::Error::NotFound)?;
        Ok(AppTransport::Usb(BulkTransport::open(&entry)?))
    }
}

/// A queue rate the picker offers, or a plain-language usage error.
fn supported_rate(rate_sps: u32) -> Result<Rate, UiError> {
    Rate::from_sps(rate_sps).ok_or_else(|| {
        UiError::usage(format!(
            "{rate_sps} samples per second is not supported; pick 2, 10, 50 or 1000."
        ))
    })
}

/// Stops the hotplug watcher thread.
#[derive(Debug)]
pub struct WatchHandle {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl WatchHandle {
    /// Stop watching and wait for the watcher thread to end.
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for WatchHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// Meters the app waits for (FR TGE839): the id of the meter's session and
/// the serial it waits to see again.
type Waiting = Arc<Mutex<Vec<(String, String)>>>;

/// One meter's session (FR 1HHT03): every connected meter has its own
/// worker, data, page, trigger session and wait.
struct Meter {
    /// The id the meter was connected with; it keeps naming the session
    /// after a replug moves the meter to another address.
    id: String,
    /// The session's worker, until it is disconnected.
    worker: Option<WorkerHandle>,
    /// The session's data, kept readable after it ends.
    store: SessionStore,
    /// The session's page: where its events go now, and what a new page
    /// needs to pick it up.
    live: Arc<Mutex<Live>>,
    /// The session's stop flag.
    stop: Arc<AtomicBool>,
    /// Rate the session was connected at, SPS.
    rate_sps: u32,
    /// Where the session's measurements come from.
    source: MeasurementSource,
    /// The device the session was (last) opened on.
    device: DeviceListing,
    /// The open trigger session (FR DGSQ6S). Shared with the session's
    /// event sink, so a meter that drops out closes it too.
    trigger: Arc<Mutex<Option<crate::trigger::TriggerWorker>>>,
    /// The queue rate to restore when the trigger session closes, if it was
    /// capped for it.
    rate_before_trigger: Option<Rate>,
}

impl Meter {
    /// Whether `id` names this meter: the id it was connected with, or the
    /// one it is listed under now.
    fn named(&self, id: &str) -> bool {
        self.id == id || self.device.id == id
    }

    /// The session's worker, unless it has ended by itself.
    fn running_worker(&self) -> Option<&WorkerHandle> {
        self.worker.as_ref().filter(|w| !w.is_finished())
    }

    /// Let go of the meter: the trigger session returns to 5 V and closes,
    /// then the worker stops. The session's data stays.
    fn release(&mut self) {
        self.rate_before_trigger = None;
        if let Some(trigger) = take(&self.trigger) {
            let _ = trigger.close();
        }
        if let Some(worker) = self.worker.take() {
            worker.disconnect();
        }
    }
}

/// Owns every meter connection: one session per connected meter (FR 1HHT03).
pub struct DeviceManager<O, C> {
    discover: Discover,
    opener: Arc<O>,
    clock: C,
    next_session_id: u64,
    /// Session per meter serial (or device id when it has none): what the
    /// outage gap needs, and the data every connection appends to.
    histories: HashMap<String, (Arc<Mutex<History>>, SessionStore)>,
    /// Every meter's session, in the order they were connected.
    meters: Vec<Meter>,
    /// The meter connected last. The commands without a device id act on
    /// it while it runs, else on the meter connected last that still runs
    /// (see [`current_id`](Self::current_id)).
    current: Option<String>,
    /// Meters whose session ended without being asked to, which the app
    /// waits for (FR TGE839).
    waiting: Waiting,
    /// Raised while a firmware update holds the meter (FR V9SK55, AC .4);
    /// shared with its [`FlashLease`].
    flashing: Arc<AtomicBool>,
    /// The meters the firmware update took (FR 1HHT03); empty while
    /// flashing means every meter is held.
    flash_targets: Arc<Mutex<Vec<String>>>,
    /// The simulator serials given to second (third, ...) demo meters, by
    /// serial: the demo device id that got each (FR 1HHT03).
    demo_serials: HashMap<String, String>,
}

/// Why a request is refused while a firmware update runs.
const FLASH_REFUSAL: &str =
    "A firmware update is running. Wait for it to finish before using the meter.";

/// Exclusive use of a meter for a firmware update (FR V9SK55, AC .4): while
/// it is held the manager refuses the measurement stream, PD capture,
/// recording, triggering and device switching. Dropping it releases them.
#[derive(Debug)]
pub struct FlashLease {
    device: DeviceListing,
    flashing: Arc<AtomicBool>,
    targets: Arc<Mutex<Vec<String>>>,
}

impl FlashLease {
    /// The meter the update was given.
    pub fn device(&self) -> &DeviceListing {
        &self.device
    }
}

impl Drop for FlashLease {
    fn drop(&mut self) {
        // Targets first: a flash begun in between keeps the ones it set.
        self.targets
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
        self.flashing.store(false, Ordering::SeqCst);
    }
}

/// The queue rate cap while a trigger session is open, SPS. [inferred]
const TRIGGER_MAX_SPS: u32 = 50;
/// How long quitting waits for an open trigger session to return to 5 V
/// and close (longer than the usual quit budget, on purpose).
const TRIGGER_SHUTDOWN_WAIT: Duration = Duration::from_secs(3);

/// The page the running session streams to, and what it has reported.
struct Live {
    device_id: String,
    info: Option<ConnectedInfo>,
    sink: Box<dyn FnMut(BackendEvent) + Send>,
}

/// A sink that forwards to the session's current page sink (replaced on
/// attach) and remembers what the session reported for a later snapshot.
fn live_sink(live: Arc<Mutex<Live>>) -> impl EventSink {
    move |event: BackendEvent| {
        let mut l = live.lock().unwrap_or_else(PoisonError::into_inner);
        match &event {
            BackendEvent::Status(Status::Connected(info)) => l.info = Some(info.clone()),
            BackendEvent::Status(Status::Rate {
                rate_sps,
                rate_locked,
            }) => {
                if let Some(info) = l.info.as_mut() {
                    info.rate_sps = Some(*rate_sps);
                    info.rate_locked = *rate_locked;
                }
            }
            _ => {}
        }
        (l.sink)(event);
    }
}

/// A sink that fills the `connected` status's model and serial from the
/// listing when the session could not read them (the CDC stream carries
/// no device info, FR 1HHT02).
fn listing_sink(device: DeviceListing, mut sink: impl EventSink) -> impl EventSink {
    move |mut event: BackendEvent| {
        if let BackendEvent::Status(Status::Connected(info)) = &mut event {
            if info.model.is_empty() {
                info.model.clone_from(&device.model);
            }
            if info.serial.is_empty()
                && let Some(serial) = &device.serial
            {
                info.serial.clone_from(serial);
            }
        }
        sink(event);
    }
}

/// Hands a running session to a new page (FR VEDDFD); taken from the
/// [`DeviceManager`] by [`DeviceManager::attacher`].
pub struct Attacher {
    control: worker::WorkerControl,
    live: Arc<Mutex<Live>>,
    stop: Arc<AtomicBool>,
}

impl Attacher {
    /// The running session's stop flag, so the new page's sink can end the
    /// session once that page is gone.
    pub fn stop(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.stop)
    }

    /// Send the session's later events to `sink` and describe it. The old
    /// page's sink gets nothing once this returns. A session still opening
    /// the meter takes `sink` too but answers `None`: its `connected`
    /// status reaches the new page later.
    pub fn attach(self, sink: impl EventSink) -> Option<AttachedSession> {
        let mut sink: Option<Box<dyn FnMut(BackendEvent) + Send>> = Some(Box::new(sink));
        {
            let mut l = self.live.lock().unwrap_or_else(PoisonError::into_inner);
            if l.info.is_none() {
                // The worker is opening the meter and reads no requests
                // yet; there is no recording to read either.
                l.sink = sink.take()?;
                return None;
            }
        }
        let live = Arc::clone(&self.live);
        let taken = Arc::new(Mutex::new(None));
        let slot = Arc::clone(&taken);
        // The swap and the reads run on the worker thread, which sends every
        // session and recording event, so the answer matches what the new
        // page receives from here on.
        let recording = self
            .control
            .hand_over(Box::new(move || {
                let mut l = live.lock().unwrap_or_else(PoisonError::into_inner);
                if let Some(sink) = sink.take() {
                    l.sink = sink;
                }
                *slot.lock().unwrap_or_else(PoisonError::into_inner) =
                    Some((l.device_id.clone(), l.info.clone()));
            }))
            .ok()?;
        let (device_id, info) = taken
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()?;
        let info = info?;
        Some(AttachedSession {
            device_id,
            model: info.model,
            firmware_version: info.firmware_version,
            serial: Some(info.serial),
            rate_sps: info.rate_sps,
            rate_locked: info.rate_locked,
            session_id: info.session_id,
            recording: recording.map(|(rows, elapsed_ms)| RecordingSnapshot { rows, elapsed_ms }),
        })
    }
}

/// What a session history remembers between connections.
#[derive(Debug)]
struct History {
    session_id: u64,
    /// Device time of the last sample received, ms.
    last_ms: Option<u64>,
    /// The last connection ended without being asked to (unplug or error).
    outage: bool,
}

/// Wrap `sink` so the events update `history`: the first samples after an
/// outage are preceded by an outage gap. Sample times are session times,
/// which never go backwards across a reconnect.
fn history_sink(history: Arc<Mutex<History>>, mut sink: impl EventSink) -> impl EventSink {
    move |event: BackendEvent| {
        let mut gap = None;
        {
            let mut h = history.lock().unwrap_or_else(PoisonError::into_inner);
            match &event {
                BackendEvent::Batch(batch) => {
                    if let (Some(first), Some(last)) = (batch.samples.first(), batch.samples.last())
                    {
                        if h.outage {
                            h.outage = false;
                            let to_ms = u64::from(first.t_ms);
                            // A meter that rebooted restarts its clock, so
                            // the gap must not run backwards.
                            gap = Some(Gap {
                                kind: GapKind::Outage,
                                from_ms: h.last_ms.unwrap_or(to_ms).min(to_ms),
                                to_ms,
                            });
                        }
                        h.last_ms = Some(u64::from(last.t_ms));
                    }
                }
                BackendEvent::Status(Status::Disconnected { error }) => {
                    // Only a session that delivered samples can have lost some.
                    h.outage = error.is_some() && h.last_ms.is_some();
                }
                _ => {}
            }
        }
        if let Some(gap) = gap {
            sink(BackendEvent::Gap(gap));
        }
        sink(event);
    }
}

/// Opens a trigger session for the connected meter (see
/// [`DeviceManager::trigger_opener`]); a simulated one for a demo device.
#[derive(Debug, Clone)]
pub struct TriggerOpener {
    demo: bool,
    serial: Option<String>,
    now_ms: u64,
}

impl TriggerOpener {
    /// Open the port and the PDM session; may take a few seconds.
    pub fn open(&self) -> Result<crate::trigger::TriggerWorker, UiError> {
        if self.demo {
            let clock = powerz_sim::SimClock::new(self.now_ms);
            let sim = powerz_sim::SimPdm::new(powerz_sim::SimCharger::MultiProtocol, clock.clone());
            return crate::trigger::TriggerWorker::spawn(sim.open_port(), clock);
        }
        let port = powerz_protocol::pdm::open_port(self.serial.as_deref())
            .map_err(|e| crate::trigger::pdm_error(&e))?;
        crate::trigger::TriggerWorker::spawn(port, powerz_protocol::session::SystemClock)
    }
}

/// The open trigger session slot, locked (a poisoned lock still holds it).
fn lock(
    slot: &Mutex<Option<crate::trigger::TriggerWorker>>,
) -> std::sync::MutexGuard<'_, Option<crate::trigger::TriggerWorker>> {
    slot.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Take the open trigger session out of `slot`, so it can be closed
/// without holding the lock.
fn take(
    slot: &Mutex<Option<crate::trigger::TriggerWorker>>,
) -> Option<crate::trigger::TriggerWorker> {
    lock(slot).take()
}

/// Forget any wait for the meter whose session is `id`.
fn forget_wait(waiting: &Waiting, id: &str) {
    waiting
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .retain(|(slot, _)| slot != id);
}

/// Wrap `sink` so a session that ends (asked to or not, e.g. an unplug)
/// first returns an open trigger session to 5 V and closes it (FR DGSQ6S):
/// the page is told the meter disconnected only after that, so it can rely
/// on the trigger session being gone.
fn trigger_sink(
    trigger: Arc<Mutex<Option<crate::trigger::TriggerWorker>>>,
    mut sink: impl EventSink,
) -> impl EventSink {
    move |event: BackendEvent| {
        if matches!(event, BackendEvent::Status(Status::Disconnected { .. }))
            && let Some(open) = take(&trigger)
        {
            let _ = open.close();
        }
        sink(event);
    }
}

/// Wrap `sink` so a real meter's session that ends without being asked to,
/// after it delivered samples, is followed by a waiting status for `serial`,
/// and the manager remembers that session `id` waits for that serial
/// (FR TGE839).
fn waiting_sink(
    waiting: Waiting,
    id: String,
    serial: String,
    history: Arc<Mutex<History>>,
    mut sink: impl EventSink,
) -> impl EventSink {
    move |event: BackendEvent| {
        let ended = matches!(event, BackendEvent::Status(Status::Disconnected { .. }));
        sink(event);
        if !ended {
            return;
        }
        // The history sink has just marked an outage: the meter went away.
        let outage = history
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .outage;
        if outage {
            let mut waits = waiting.lock().unwrap_or_else(PoisonError::into_inner);
            waits.retain(|(slot, _)| slot != &id);
            waits.push((id.clone(), serial.clone()));
            drop(waits);
            sink(BackendEvent::Status(Status::Waiting {
                serial: serial.clone(),
            }));
        }
    }
}

/// Why a request names no connected meter.
const NOT_CONNECTED: &str = "No meter is connected.";

impl<O, C> DeviceManager<O, C>
where
    O: Opener + Send + Sync + 'static,
    C: Clock + Clone + Send + 'static,
{
    /// A manager listing devices with `discover` and opening them with `opener`.
    pub fn new(
        discover: impl Fn() -> Result<Vec<DeviceEntry>, transport::Error> + Send + Sync + 'static,
        opener: O,
        clock: C,
    ) -> DeviceManager<O, C> {
        DeviceManager {
            discover: Arc::new(discover),
            opener: Arc::new(opener),
            clock,
            next_session_id: 1,
            histories: HashMap::new(),
            meters: Vec::new(),
            current: None,
            waiting: Arc::default(),
            flashing: Arc::default(),
            flash_targets: Arc::default(),
            demo_serials: HashMap::new(),
        }
    }

    /// Refuse a request while a firmware update holds the meter.
    fn ensure_not_flashing(&self) -> Result<(), UiError> {
        if self.flashing.load(Ordering::SeqCst) {
            return Err(UiError::usage(FLASH_REFUSAL));
        }
        Ok(())
    }

    /// Refuse a request for meter `id` while a firmware update holds it
    /// (FR 1HHT03): the other meters stay usable.
    fn ensure_not_flashing_for(&self, id: &str) -> Result<(), UiError> {
        if self.flashing.load(Ordering::SeqCst) {
            let targets = self
                .flash_targets
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if targets.is_empty() || targets.iter().any(|t| t == id) {
                return Err(UiError::usage(FLASH_REFUSAL));
            }
        }
        Ok(())
    }

    /// The session `id` names, if any.
    fn meter(&self, id: &str) -> Option<&Meter> {
        self.meters.iter().find(|m| m.named(id))
    }

    /// The session `id` names, if any, to change.
    fn meter_mut(&mut self, id: &str) -> Option<&mut Meter> {
        self.meters.iter_mut().find(|m| m.named(id))
    }

    /// The id of that session ("" before any connect, which names none):
    /// the meter connected last while it runs, else the meter connected
    /// last that still runs, so ending one of several meters never leaves
    /// these commands on the ended one (FR 1HHT03). With none running it
    /// stays the meter connected last, whose data stays readable.
    fn current_id(&self) -> String {
        let last = self.current.as_deref().unwrap_or_default();
        let runs = |m: &&Meter| m.running_worker().is_some();
        if self.meter(last).is_some_and(|m| runs(&m)) {
            return last.to_owned();
        }
        self.meters
            .iter()
            .rev()
            .find(runs)
            .map_or_else(|| last.to_owned(), |m| m.id.clone())
    }

    /// The meter a command acts on (FR 1HHT03): the one it names, or the
    /// session the commands without a device id act on.
    pub fn target(&self, id: Option<&str>) -> String {
        id.map_or_else(|| self.current_id(), str::to_owned)
    }

    /// Forget any wait for the meter whose session is `id`.
    fn stop_waiting_for(&self, id: &str) {
        forget_wait(&self.waiting, id);
    }

    /// Whether a firmware update holds the meter.
    pub fn flashing(&self) -> bool {
        self.flashing.load(Ordering::SeqCst)
    }

    /// Whether the main window may close now: not while a firmware update
    /// holds the meter, since quitting would cut the flash short.
    pub fn close_allowed(&self) -> bool {
        close_allowed(self.flashing())
    }

    /// Take the listed device `id` for a firmware update (FR V9SK55,
    /// AC .4): that meter's trigger session and measurement stream close
    /// first (waiting for the worker to let go of the pipe), its wait for an
    /// unplug ends, and everything else that talks to a meter is refused
    /// until the lease is dropped. A bootloader-mode meter can be taken too.
    pub fn begin_flash(&mut self, id: &str) -> Result<FlashLease, UiError> {
        self.ensure_not_flashing()?;
        let device = self
            .list_devices()
            .into_iter()
            .find(|d| d.id == id)
            .ok_or_else(|| UiError::usage("That meter is no longer attached."))?;
        // The session of that meter (by id, or by serial after a replug);
        // when none matches, every session lets go of its meter.
        let same = |m: &Meter| {
            m.named(id)
                || (device.demo_scenario.is_none()
                    && m.device.demo_scenario.is_none()
                    && device.serial.is_some()
                    && m.device.serial == device.serial)
        };
        let any = self.meters.iter().any(same);
        // The targets are in place before the flag is raised, so the other
        // meters are never refused, not even for a moment.
        *self
            .flash_targets
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = if any {
            self.meters
                .iter()
                .filter(|m| same(m))
                .map(|m| m.id.clone())
                .collect()
        } else {
            Vec::new()
        };
        self.flashing.store(true, Ordering::SeqCst);
        for m in self.meters.iter_mut().filter(|m| !any || same(m)) {
            m.release();
            forget_wait(&self.waiting, &m.id);
        }
        Ok(FlashLease {
            device,
            flashing: Arc::clone(&self.flashing),
            targets: Arc::clone(&self.flash_targets),
        })
    }

    /// Serial of the unplugged meter the app waits for, if any.
    pub fn waiting_for(&self) -> Option<String> {
        self.waiting_serials().into_iter().next()
    }

    /// Serials of every unplugged meter the app waits for (FR 1HHT03).
    pub fn waiting_serials(&self) -> Vec<String> {
        self.waiting
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|(_, serial)| serial.clone())
            .collect()
    }

    /// Stop waiting for unplugged meters: the user picked another device,
    /// so a meter that comes back later is listed but not reconnected.
    pub fn cancel_wait(&mut self) {
        self.waiting
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }

    /// Stop waiting for meter `id` only (FR 1HHT03); the others' waits stay.
    pub fn cancel_wait_for(&mut self, id: &str) {
        self.stop_waiting_for(id);
    }

    /// The hotplug watcher's changed device list; returns whether it
    /// started a connect. Only an awaited meter that is listed again is
    /// reconnected, into its own session; nothing else is ever connected on
    /// its own.
    pub fn devices_changed(&mut self, devices: &[DeviceListing]) -> bool {
        if self.flashing() {
            return false;
        }
        let waits = self
            .waiting
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let mut started = false;
        for (id, serial) in waits {
            let Some(index) = self.meters.iter().position(|m| m.id == id) else {
                continue;
            };
            let meter = &mut self.meters[index];
            if meter.running_worker().is_some() || meter.stop.load(Ordering::SeqCst) {
                // Still running, or the page that waited is gone.
                continue;
            }
            let Some(device) = devices
                .iter()
                .find(|d| d.demo_scenario.is_none() && d.serial.as_deref() == Some(serial.as_str()))
                .cloned()
            else {
                continue;
            };
            // A meter back in bootloader mode is listed but not opened; keep
            // waiting for its application-mode return.
            if device.ensure_connectable().is_err() {
                continue;
            }
            // Resume at the rate the session last ran at. The ended session's
            // info is only dropped once the reconnect is certain to start.
            let rate = {
                let mut l = meter.live.lock().unwrap_or_else(PoisonError::into_inner);
                let rate_sps = l
                    .info
                    .as_ref()
                    .and_then(|i| i.rate_sps)
                    .unwrap_or(meter.rate_sps);
                let Ok(rate) = supported_rate(rate_sps) else {
                    continue;
                };
                l.info = None;
                l.device_id = device.id.clone();
                meter.rate_sps = rate_sps;
                rate
            };
            // The ended worker shares the page's stop flag: let it go without
            // raising that flag, and without joining it under the manager lock.
            if let Some(old) = meter.worker.take() {
                old.retire();
            }
            meter.device = device;
            self.open_session(index, rate);
            started = true;
        }
        started
    }

    /// Every attached meter plus the demo entries; never opens a device.
    pub fn list_devices(&self) -> Vec<DeviceListing> {
        list_devices_from((self.discover)())
    }

    /// Connect to the device with id `id` at `rate_sps` on a dedicated worker
    /// thread; events go to `sink`. Several meters can be connected at once
    /// (FR 1HHT03), each with its own session; the same one twice is refused.
    pub fn connect(
        &mut self,
        id: &str,
        rate_sps: u32,
        sink: impl EventSink,
    ) -> Result<(), UiError> {
        self.connect_source(id, rate_sps, MeasurementSource::Queue, sink)
    }

    /// [`DeviceManager::connect`] with the measurement `source` to stream
    /// from (FR 1HHT02).
    pub fn connect_source(
        &mut self,
        id: &str,
        rate_sps: u32,
        source: MeasurementSource,
        sink: impl EventSink,
    ) -> Result<(), UiError> {
        self.connect_source_with_stop(id, rate_sps, source, sink, Arc::new(AtomicBool::new(false)))
    }

    /// [`DeviceManager::connect`] with a stop flag the caller also holds;
    /// setting it ends the session (for example when the UI is gone).
    pub fn connect_with_stop(
        &mut self,
        id: &str,
        rate_sps: u32,
        sink: impl EventSink,
        stop: Arc<AtomicBool>,
    ) -> Result<(), UiError> {
        self.connect_source_with_stop(id, rate_sps, MeasurementSource::Queue, sink, stop)
    }

    /// [`DeviceManager::connect_source`] with a stop flag the caller also
    /// holds, as [`DeviceManager::connect_with_stop`].
    pub fn connect_source_with_stop(
        &mut self,
        id: &str,
        rate_sps: u32,
        source: MeasurementSource,
        sink: impl EventSink,
        stop: Arc<AtomicBool>,
    ) -> Result<(), UiError> {
        self.ensure_not_flashing()?;
        if self.meter(id).is_some_and(|m| m.running_worker().is_some()) {
            return Err(UiError::usage(
                "This meter is already connected. Disconnect it first.",
            ));
        }
        let rate = supported_rate(rate_sps)?;
        let mut device = self
            .list_devices()
            .into_iter()
            .find(|d| d.id == id)
            .ok_or_else(|| UiError::usage("That meter is no longer attached."))?;
        device.ensure_connectable()?;
        // Sessions that ended (unplugged, failed or disconnected) make way,
        // and their waits end, as with a single meter. While other meters
        // run, a meter added beside them keeps the waits for unplugged ones
        // (FR 1HHT03); only an earlier session of this very meter goes.
        let adding = self.meters.iter().any(|m| m.running_worker().is_some());
        let waits = self
            .waiting
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let keep = |m: &Meter| {
            if m.running_worker().is_some() {
                return true;
            }
            let same_meter = m.named(id)
                || (device.demo_scenario.is_none()
                    && m.device.demo_scenario.is_none()
                    && device.serial.is_some()
                    && m.device.serial == device.serial);
            let waited =
                waits.iter().any(|(slot, _)| slot == &m.id) && !m.stop.load(Ordering::SeqCst);
            adding && waited && !same_meter
        };
        let (kept, ended): (Vec<Meter>, Vec<Meter>) =
            std::mem::take(&mut self.meters).into_iter().partition(keep);
        self.meters = kept;
        for mut m in ended {
            self.stop_waiting_for(&m.id);
            m.release();
        }
        // A second demo meter reports a serial of its own, so the two never
        // share a session; a demo meter connected again gets the one it had,
        // even once the shared demo serial is free again.
        if device.demo_scenario.is_some()
            && (self.serial_in_use(device.serial.as_deref())
                || self.owned_demo_serial(&device.id).is_some())
        {
            device.serial = Some(self.free_demo_serial(&device.id));
        }
        let (_, store) = self.history(&device);
        let live = Arc::new(Mutex::new(Live {
            device_id: device.id.clone(),
            info: None,
            sink: Box::new(sink),
        }));
        self.meters.push(Meter {
            id: device.id.clone(),
            worker: None,
            store,
            live,
            stop,
            rate_sps,
            source,
            device: device.clone(),
            trigger: Arc::default(),
            rate_before_trigger: None,
        });
        self.current = Some(device.id);
        self.open_session(self.meters.len() - 1, rate);
        Ok(())
    }

    /// Whether a connected meter reports `serial`.
    fn serial_in_use(&self, serial: Option<&str>) -> bool {
        serial.is_some_and(|s| {
            self.meters
                .iter()
                .any(|m| m.device.serial.as_deref() == Some(s))
        })
    }

    /// The serial demo meter `id` was given when it shared the demo device
    /// with another demo meter, if any.
    fn owned_demo_serial(&self, id: &str) -> Option<String> {
        self.demo_serials
            .iter()
            .find(|(_, owner)| owner.as_str() == id)
            .map(|(serial, _)| serial.clone())
    }

    /// A simulator serial for demo meter `id` that no connected meter
    /// reports: the one `id` got before, else the next number after
    /// [`powerz_sim::SIM_SERIAL`] that no other demo meter got, so a new
    /// demo meter never continues another one's session and data.
    fn free_demo_serial(&mut self, id: &str) -> String {
        if let Some(serial) = self
            .owned_demo_serial(id)
            .filter(|s| !self.serial_in_use(Some(s)))
        {
            return serial;
        }
        let mut n: u64 = powerz_sim::SIM_SERIAL.parse().unwrap_or(123_456);
        loop {
            n += 1;
            let serial = n.to_string();
            if !self.serial_in_use(Some(&serial)) && !self.demo_serials.contains_key(&serial) {
                self.demo_serials.insert(serial.clone(), id.to_owned());
                return serial;
            }
        }
    }

    /// The history of `device`'s session: it follows the meter's serial, so
    /// a reconnect continues the same session.
    fn history(&mut self, device: &DeviceListing) -> (Arc<Mutex<History>>, SessionStore) {
        let key = device.serial.clone().unwrap_or_else(|| device.id.clone());
        let next_session_id = &mut self.next_session_id;
        let (history, store) = self.histories.entry(key).or_insert_with(|| {
            let session_id = *next_session_id;
            *next_session_id += 1;
            let history = History {
                session_id,
                last_ms: None,
                outage: false,
            };
            (Arc::new(Mutex::new(history)), SessionStore::new())
        });
        (Arc::clone(history), store.clone())
    }

    /// Open the device of session `index` at `rate` on a new worker whose
    /// events go to that session's page; the session id and data follow the
    /// meter's serial, so a reconnect continues the same session.
    fn open_session(&mut self, index: usize, rate: Rate) {
        let (history, store) = self.history(&self.meters[index].device.clone());
        let opener = Arc::clone(&self.opener);
        let clock = self.clock.clone();
        let waiting = Arc::clone(&self.waiting);
        let m = &mut self.meters[index];
        // A new bulk CONNECT silences an open PDM session [device], so the
        // trigger session (if any) returns to 5 V and closes first.
        m.release();
        // A new connection ends the wait for this meter.
        forget_wait(&waiting, &m.id);
        let device = m.device.clone();
        let waits_for = match (&device.demo_scenario, &device.serial) {
            (None, Some(serial)) => Some(serial.clone()),
            _ => None,
        };
        m.store = store.clone();
        let session_id = history
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .session_id;
        let source = m.source;
        let config = WorkerConfig {
            mode: device.mode,
            source,
            ..WorkerConfig::new(rate, session_id)
        };
        let sink = trigger_sink(
            Arc::clone(&m.trigger),
            history_sink(
                Arc::clone(&history),
                listing_sink(device.clone(), live_sink(Arc::clone(&m.live))),
            ),
        );
        let sink: Box<dyn FnMut(BackendEvent) + Send> = match waits_for {
            Some(serial) => Box::new(waiting_sink(waiting, m.id.clone(), serial, history, sink)),
            None => Box::new(sink),
        };
        m.worker = Some(worker::spawn_with_store(
            move || opener.open_source(&device, source),
            clock,
            config,
            store,
            sink,
            Arc::clone(&m.stop),
        ));
    }

    /// The ids of the meters whose session runs, in the order they were
    /// connected (FR 1HHT03).
    pub fn connected_ids(&self) -> Vec<String> {
        self.meters
            .iter()
            .filter(|m| m.running_worker().is_some())
            .map(|m| m.id.clone())
            .collect()
    }

    /// The ids a new page names the running meters by, in the order they
    /// were connected (FR 1HHT03): each one's current listing, which a
    /// replug may have moved, as its attach snapshot reports it.
    pub fn running_ids(&self) -> Vec<String> {
        self.meters
            .iter()
            .filter(|m| m.running_worker().is_some())
            .map(|m| m.device.id.clone())
            .collect()
    }

    /// Restart the open session's queue at `rate_sps`.
    pub fn set_rate(&mut self, rate_sps: u32) -> Result<(), UiError> {
        let id = self.current_id();
        self.set_rate_for(&id, rate_sps)
    }

    /// Restart meter `id`'s queue at `rate_sps`.
    pub fn set_rate_for(&mut self, id: &str, rate_sps: u32) -> Result<(), UiError> {
        let (control, rate) = self.rate_control_for(id, rate_sps)?;
        control.set_rate(rate)
    }

    /// What [`DeviceManager::set_rate`] needs, so a caller can wait for the
    /// worker's answer after releasing its lock on the manager.
    pub fn rate_control(&self, rate_sps: u32) -> Result<(worker::WorkerControl, Rate), UiError> {
        self.rate_control_for(&self.current_id(), rate_sps)
    }

    /// [`DeviceManager::rate_control`] of meter `id`.
    pub fn rate_control_for(
        &self,
        id: &str,
        rate_sps: u32,
    ) -> Result<(worker::WorkerControl, Rate), UiError> {
        let rate = supported_rate(rate_sps)?;
        // The queue stays at 50 SPS or less while triggering. [inferred]
        if rate_sps > TRIGGER_MAX_SPS && self.trigger_open_for(id) {
            return Err(UiError::usage(
                "The rate stays at 50 samples/s or less while the trigger panel is open.",
            ));
        }
        Ok((self.control_for(id)?, rate))
    }

    /// Recording and export control of the open session, so a caller can
    /// wait for the worker's answer after releasing its lock on the manager.
    pub fn control(&self) -> Result<worker::WorkerControl, UiError> {
        self.control_for(&self.current_id())
    }

    /// [`DeviceManager::control`] of meter `id`.
    pub fn control_for(&self, id: &str) -> Result<worker::WorkerControl, UiError> {
        self.ensure_not_flashing_for(id)?;
        self.meter(id)
            .and_then(Meter::running_worker)
            .map(WorkerHandle::control)
            .ok_or_else(|| UiError::usage(NOT_CONNECTED))
    }

    /// Record the open session's stream to a new file at `path`.
    pub fn start_recording(&self, path: &Path) -> Result<(), UiError> {
        self.control()?.record_to(path)
    }

    /// Record meter `id`'s stream to a new file at `path`.
    pub fn start_recording_for(&self, id: &str, path: &Path) -> Result<(), UiError> {
        self.control_for(id)?.record_to(path)
    }

    /// Stop the recording and return what it wrote.
    pub fn stop_recording(&self) -> Result<RecordingSummary, UiError> {
        self.control()?.stop_recording()
    }

    /// Stop meter `id`'s recording and return what it wrote.
    pub fn stop_recording_for(&self, id: &str) -> Result<RecordingSummary, UiError> {
        self.control_for(id)?.stop_recording()
    }

    /// The data of the open session, or of the last one once the meter is
    /// gone (unplugging keeps the data). Refused only before any connect.
    pub fn session_store(&self) -> Result<SessionStore, UiError> {
        self.session_store_for(&self.current_id())
    }

    /// The data of meter `id`'s session, kept readable after it ends.
    pub fn session_store_for(&self, id: &str) -> Result<SessionStore, UiError> {
        self.meter(id)
            .map(|m| m.store.clone())
            .ok_or_else(|| UiError::usage(NOT_CONNECTED))
    }

    /// What [`DeviceManager::export_session`] needs, so a caller can wait
    /// for the worker's answer after releasing its lock on the manager.
    pub fn exporter(&self) -> Result<worker::Exporter, UiError> {
        self.exporter_for(&self.current_id())
    }

    /// [`DeviceManager::exporter`] of meter `id`.
    pub fn exporter_for(&self, id: &str) -> Result<worker::Exporter, UiError> {
        let store = self.session_store_for(id)?;
        let control = self.control_for(id).ok();
        Ok(worker::Exporter::new(
            control,
            store,
            self.clock.clone().now_ms(),
        ))
    }

    /// Export the session so far (the open one, or the last one) to a new
    /// file at `path` on a thread of its own; `progress` is called on that
    /// thread.
    pub fn export_session(
        &self,
        path: &Path,
        progress: impl FnMut(ExportProgress) + Send + 'static,
    ) -> Result<worker::ExportJob, UiError> {
        self.exporter()?.export_to(path, progress)
    }

    /// Statistics of the session.
    pub fn stats(&self) -> Result<StatsSnapshot, UiError> {
        Ok(self.session_store()?.stats())
    }

    /// Statistics of meter `id`'s session.
    pub fn stats_for(&self, id: &str) -> Result<StatsSnapshot, UiError> {
        Ok(self.session_store_for(id)?.stats())
    }

    /// The gaps between measuring runs of the session's history.
    pub fn history_gaps(&self, t0: u64, t1: u64) -> Result<Vec<HistoryGap>, UiError> {
        Ok(self.session_store()?.history_gaps(t0, t1))
    }

    /// Min / max buckets of the session's history.
    pub fn history_range(
        &self,
        series: &[Series],
        t0: u64,
        t1: u64,
        buckets: usize,
    ) -> Result<Vec<SeriesBuckets>, UiError> {
        Ok(self.session_store()?.history_range(series, t0, t1, buckets))
    }

    /// Zero the session's statistics.
    pub fn reset_stats(&self) -> Result<(), UiError> {
        self.session_store()?.reset_stats();
        Ok(())
    }

    /// Discard the session's history.
    pub fn clear_history(&self) -> Result<(), UiError> {
        self.session_store()?.clear_history();
        Ok(())
    }

    /// Take the open session out of the manager, so a caller can close it
    /// (which waits for the worker thread) without holding its lock.
    pub fn take_session(&mut self) -> Result<WorkerHandle, UiError> {
        let id = self.current_id();
        self.take_session_for(&id)
    }

    /// [`DeviceManager::take_session`] of meter `id`; its data stays.
    pub fn take_session_for(&mut self, id: &str) -> Result<WorkerHandle, UiError> {
        self.meter_mut(id)
            .and_then(|m| m.worker.take())
            .ok_or_else(|| UiError::usage(NOT_CONNECTED))
    }

    /// Watch for plugged and unplugged meters on a thread of its own:
    /// `sink` gets the full device list first and again on every change.
    pub fn watch_devices(
        &self,
        sink: impl FnMut(Vec<DeviceListing>) + Send + 'static,
    ) -> WatchHandle {
        self.watch_devices_with_stop(sink, Arc::new(AtomicBool::new(false)))
    }

    /// [`DeviceManager::watch_devices`] with a stop flag the caller also
    /// holds; setting it ends the watcher (for example when the UI is gone).
    pub fn watch_devices_with_stop(
        &self,
        mut sink: impl FnMut(Vec<DeviceListing>) + Send + 'static,
        stop: Arc<AtomicBool>,
    ) -> WatchHandle {
        let flag = Arc::clone(&stop);
        let discover = Arc::clone(&self.discover);
        let waiting = Arc::clone(&self.waiting);
        let mut clock = self.clock.clone();
        let thread = std::thread::spawn(move || {
            let mut hotplug = Hotplug::new();
            while !flag.load(Ordering::SeqCst) {
                // While the app waits for a meter, report every poll, changed or
                // not: a replug seen before the wait was armed, or a reconnect
                // that failed to open, gets another try (FR TGE839).
                if !waiting
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .is_empty()
                {
                    hotplug.forget();
                }
                if let Some(list) = hotplug.tick(clock.now_ms(), || discover()) {
                    sink(list);
                }
                clock.sleep(Duration::from_millis(HOTPLUG_PERIOD_MS));
            }
        });
        WatchHandle {
            stop,
            thread: Some(thread),
        }
    }

    /// Hand the running session to a new page: its later events go to
    /// `sink`, and the answer describes it. `None` when no session is
    /// running (never connected, disconnected or ended by itself); nothing
    /// is opened then and `sink` is dropped unused.
    pub fn attach(&mut self, sink: impl EventSink) -> Option<AttachedSession> {
        self.attacher()?.attach(sink)
    }

    /// What an attach needs of the running session, so the caller can hand
    /// it over without holding the manager. `None` when no session is
    /// running.
    pub fn attacher(&self) -> Option<Attacher> {
        self.attacher_for(&self.current_id())
    }

    /// [`DeviceManager::attacher`] of meter `id` (FR 1HHT03): a reload
    /// hands every running meter to the new page, each with its own sink.
    pub fn attacher_for(&self, id: &str) -> Option<Attacher> {
        let meter = self.meter(id)?;
        let worker = meter.running_worker()?;
        Some(Attacher {
            control: worker.control(),
            live: Arc::clone(&meter.live),
            stop: Arc::clone(&meter.stop),
        })
    }

    /// Stop the queue and close the open session.
    pub fn disconnect(&mut self) -> Result<(), UiError> {
        let id = self.current_id();
        self.disconnect_for(&id)
    }

    /// Stop meter `id`'s queue and close its session; the other meters run
    /// on, and its data stays readable.
    pub fn disconnect_for(&mut self, id: &str) -> Result<(), UiError> {
        // Back to 5 V and `pdm close` before the queue stops.
        if let Some(trigger) = self.take_trigger_for(id) {
            let _ = trigger.close();
        }
        self.take_session_for(id)?.disconnect();
        Ok(())
    }

    /// Hand the open session a trigger session (FR DGSQ6S): refused (and
    /// `worker` closed) without a connected meter or while one is open. The
    /// queue is kept at 50 SPS or less while it is open. [inferred]
    pub fn install_trigger(
        &mut self,
        worker: crate::trigger::TriggerWorker,
    ) -> Result<(), UiError> {
        let id = self.current_id();
        self.install_trigger_for(&id, worker)
    }

    /// [`DeviceManager::install_trigger`] on meter `id`.
    pub fn install_trigger_for(
        &mut self,
        id: &str,
        mut worker: crate::trigger::TriggerWorker,
    ) -> Result<(), UiError> {
        let Some(meter) = self.meter_mut(id) else {
            let _ = worker.close();
            return Err(UiError::usage("Connect a meter first."));
        };
        // A session that ended on its own (unplug) may have left this set.
        if lock(&meter.trigger).is_none() {
            meter.rate_before_trigger = None;
        }
        let Some(control) = meter.running_worker().map(WorkerHandle::control) else {
            let _ = worker.close();
            return Err(UiError::usage("Connect a meter first."));
        };
        if lock(&meter.trigger).is_some() {
            let _ = worker.close();
            return Err(UiError::usage("A trigger session is already open."));
        }
        // The PD capture pauses while triggering. [inferred]
        let _ = control.stop_pd();
        let current = meter
            .live
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .info
            .as_ref()
            .and_then(|i| i.rate_sps)
            .unwrap_or(meter.rate_sps);
        if current > TRIGGER_MAX_SPS
            && let (Some(capped), Some(before)) =
                (Rate::from_sps(TRIGGER_MAX_SPS), Rate::from_sps(current))
            && control.set_rate(capped).is_ok()
        {
            meter.rate_before_trigger = Some(before);
        }
        // Polling pauses while the automatic list runs (AC-DGSQ6S.11).
        worker.pause_during_list(control);
        *lock(&meter.trigger) = Some(worker);
        Ok(())
    }

    /// Open a trigger session on the connected meter's virtual serial port
    /// (a simulated one for a demo device).
    pub fn open_trigger(&mut self) -> Result<(), UiError> {
        let opener = self.trigger_opener()?;
        let worker = opener.open()?;
        self.install_trigger(worker)
    }

    /// What opening a trigger session needs, checked under the manager lock,
    /// so the caller can open the port and run the PDM handshake without it
    /// and then [`DeviceManager::install_trigger`] the result.
    pub fn trigger_opener(&self) -> Result<TriggerOpener, UiError> {
        self.trigger_opener_for(&self.current_id())
    }

    /// [`DeviceManager::trigger_opener`] of meter `id`.
    pub fn trigger_opener_for(&self, id: &str) -> Result<TriggerOpener, UiError> {
        self.ensure_not_flashing_for(id)?;
        let Some(meter) = self.meter(id).filter(|m| m.running_worker().is_some()) else {
            return Err(UiError::usage("Connect a meter first."));
        };
        if lock(&meter.trigger).is_some() {
            return Err(UiError::usage("A trigger session is already open."));
        }
        if meter.source == MeasurementSource::Cdc {
            // The serial stream already holds the CDC port the trigger needs.
            return Err(UiError::usage(
                "The serial stream uses the meter's serial port; reconnect with the USB queue source to trigger.",
            ));
        }
        Ok(TriggerOpener {
            demo: meter.device.demo_scenario.is_some(),
            serial: meter.device.serial.clone(),
            now_ms: self.clock.clone().now_ms(),
        })
    }

    /// The open trigger session's handle.
    pub fn trigger_control(&self) -> Result<crate::trigger::TriggerControl, UiError> {
        self.trigger_control_for(&self.current_id())
    }

    /// The handle of meter `id`'s trigger session.
    pub fn trigger_control_for(&self, id: &str) -> Result<crate::trigger::TriggerControl, UiError> {
        self.meter(id)
            .and_then(|m| {
                lock(&m.trigger)
                    .as_ref()
                    .map(crate::trigger::TriggerWorker::control)
            })
            .ok_or_else(|| UiError::usage("No trigger session is open."))
    }

    /// Whether a trigger session is open.
    pub fn trigger_open(&self) -> bool {
        self.trigger_open_for(&self.current_id())
    }

    /// Whether meter `id` has a trigger session open.
    pub fn trigger_open_for(&self, id: &str) -> bool {
        self.meter(id).is_some_and(|m| lock(&m.trigger).is_some())
    }

    /// Take the open trigger session out of the manager, so a caller can
    /// close it without holding its lock; the capped queue rate is restored.
    pub fn take_trigger(&mut self) -> Option<crate::trigger::TriggerWorker> {
        let id = self.current_id();
        self.take_trigger_for(&id)
    }

    /// [`DeviceManager::take_trigger`] of meter `id`.
    pub fn take_trigger_for(&mut self, id: &str) -> Option<crate::trigger::TriggerWorker> {
        let meter = self.meter_mut(id)?;
        let trigger = take(&meter.trigger)?;
        if let Some(rate) = meter.rate_before_trigger.take()
            && let Some(worker) = meter.running_worker()
        {
            let _ = worker.set_rate(rate);
        }
        Some(trigger)
    }

    /// Back to 5 V and close the trigger session, if one is open.
    pub fn close_trigger(&mut self) -> Result<(), UiError> {
        match self.take_trigger() {
            Some(trigger) => trigger.close(),
            None => Ok(()),
        }
    }

    /// End every open session on quit or window close (FR X2SPDW): stops
    /// the queues and releases the interfaces. Events go through the
    /// forwarder thread, so a slow page never holds this up.
    pub fn shutdown(&mut self) {
        // Back to 5 V and `pdm close` first, while the meters are still
        // there. Bounded, but generously: a contract left above 5 V outlives
        // the app (the PD contract continues after the port closes
        // [device]), so returning to 5 V is worth more than the 1 s quit
        // budget here.
        for m in &mut self.meters {
            if let Some(trigger) = take(&m.trigger) {
                m.rate_before_trigger = None;
                let _ = trigger.close_within(TRIGGER_SHUTDOWN_WAIT);
            }
            m.stop.store(true, Ordering::SeqCst);
        }
        // Called on the main thread at quit: never hold it past the AC's 1 s,
        // however many meters are open.
        let deadline = std::time::Instant::now() + SHUTDOWN_WAIT;
        for m in &mut self.meters {
            if let Some(worker) = m.worker.take() {
                let left = deadline.saturating_duration_since(std::time::Instant::now());
                worker.disconnect_within(left);
            }
        }
    }
}

/// How long a shutdown waits for the worker to stop the queue and close the pipe.
const SHUTDOWN_WAIT: Duration = Duration::from_millis(900);

/// How often the hotplug poller re-lists devices; well under the 2 s the UI
/// needs to reflect a plug or unplug.
pub const HOTPLUG_PERIOD_MS: u64 = 500;

/// Re-lists devices periodically and reports changes.
#[derive(Debug, Default)]
pub struct Hotplug {
    last: Option<Vec<DeviceListing>>,
    next_due_ms: u64,
}

impl Hotplug {
    /// A hotplug poller that has not listed yet.
    pub fn new() -> Hotplug {
        Hotplug::default()
    }

    /// The new device list when it changed at `now_ms`. The first tick always
    /// lists; later ticks re-list every [`HOTPLUG_PERIOD_MS`]. A failed
    /// discovery keeps the last list.
    pub fn tick(
        &mut self,
        now_ms: u64,
        discover: impl FnOnce() -> Result<Vec<DeviceEntry>, transport::Error>,
    ) -> Option<Vec<DeviceListing>> {
        if self.last.is_some() && now_ms < self.next_due_ms {
            return None;
        }
        self.next_due_ms = now_ms.saturating_add(HOTPLUG_PERIOD_MS);
        let discovered = discover();
        if discovered.is_err() && self.last.is_some() {
            return None;
        }
        let list = list_devices_from(discovered);
        if self.last.as_ref() == Some(&list) {
            return None;
        }
        self.last = Some(list.clone());
        Some(list)
    }

    /// Forget the last list, so the next tick reports the list even when it
    /// did not change.
    pub fn forget(&mut self) {
        self.last = None;
    }
}

/// A close request goes through unless a firmware update is running.
pub fn close_allowed(flashing: bool) -> bool {
    !flashing
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::batch::{Batch, Sample};

    #[test]
    fn closing_the_window_is_refused_only_while_a_firmware_update_runs() {
        assert!(close_allowed(false));
        assert!(!close_allowed(true));
    }

    fn batch_at(t_ms: u32) -> BackendEvent {
        BackendEvent::Batch(Batch {
            samples: vec![Sample {
                t_ms,
                vbus_v: 5.0,
                ibus_a: 1.0,
                cc1_v: 0.0,
                cc2_v: 0.0,
                dp_v: 0.0,
                dm_v: 0.0,
            }],
            temp_c: None,
            stats: Default::default(),
        })
    }

    #[test]
    fn a_failed_open_before_any_sample_leaves_no_outage_gap() {
        let history = Arc::new(Mutex::new(History {
            session_id: 1,
            last_ms: None,
            outage: false,
        }));
        let got = Arc::new(Mutex::new(Vec::new()));
        let keep = Arc::clone(&got);
        let mut sink = history_sink(history, move |e: BackendEvent| keep.lock().unwrap().push(e));
        sink(BackendEvent::Status(Status::Disconnected {
            error: Some(UiError::usage("could not open")),
        }));
        sink(batch_at(5_000));
        assert!(
            !got.lock()
                .unwrap()
                .iter()
                .any(|e| matches!(e, BackendEvent::Gap(_))),
            "a failed open produced an outage gap"
        );
    }

    #[test]
    fn an_outage_gap_never_runs_backwards_when_the_meter_clock_restarts() {
        let history = Arc::new(Mutex::new(History {
            session_id: 1,
            last_ms: None,
            outage: false,
        }));
        let got = Arc::new(Mutex::new(Vec::new()));
        let keep = Arc::clone(&got);
        let mut sink = history_sink(history, move |e: BackendEvent| keep.lock().unwrap().push(e));
        sink(batch_at(90_000));
        sink(BackendEvent::Status(Status::Disconnected {
            error: Some(UiError::usage("unplugged")),
        }));
        // The meter rebooted: its device time starts again near zero.
        sink(batch_at(1_000));
        let gaps: Vec<Gap> = got
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| match e {
                BackendEvent::Gap(g) => Some(g.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(gaps.len(), 1, "one outage gap");
        assert_eq!(gaps[0].kind, GapKind::Outage);
        assert!(
            gaps[0].from_ms <= gaps[0].to_ms,
            "gap runs backwards: {:?}",
            gaps[0]
        );
    }
}
