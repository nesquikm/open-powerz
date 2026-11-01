//! Device listing, hotplug and the device manager (FR VHY378).
//!
//! Listing reads the OS device list only; it never opens a meter. The demo
//! device adds one entry per simulator scenario.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError, Weak};
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
use serde::{Deserialize, Serialize};

use crate::app_log::{AppLog, SessionLog};
use crate::firmware_flash::{FlashControl, FlashDone, FlashEnd, FlashPrompt, FlashStage};
use crate::history::{HistoryGap, Series, SeriesBuckets};
use crate::recording::{ExportProgress, RecordingEvent, RecordingSummary};
use crate::stats::StatsSnapshot;
use crate::store::SessionStore;
use crate::ui_error::{ErrorKind, UiError};
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecordingSnapshot {
    /// Rows written so far.
    pub rows: u64,
    /// Time since the recording started, ms.
    pub elapsed_ms: u64,
    /// Its file: Stop asks where to keep it (AC-4PJRVM.2).
    pub path: Option<String>,
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
    /// The PD capture runs (AC-AXHK2K.7).
    pub pd_capturing: bool,
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
    /// A real meter on its vendor bulk interface, with its USB serial
    /// number (to find its CDC port for the settings write).
    Usb(GuardedTransport<BulkTransport>, Option<String>),
    /// A real meter on its CDC serial port.
    Cdc(GuardedTransport<SerialCdcTransport>),
}

impl Transport for AppTransport {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, transport::Error> {
        match self {
            AppTransport::Demo(t) => t.transact(request, timeout),
            AppTransport::Usb(t, _) => t.transact(request, timeout),
            AppTransport::Cdc(t) => t.transact(request, timeout),
        }
    }

    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, transport::Error> {
        match self {
            AppTransport::Demo(t) => t.read(timeout),
            AppTransport::Usb(t, _) => t.read(timeout),
            AppTransport::Cdc(t) => t.read(timeout),
        }
    }

    fn transact_record_table(
        &mut self,
        request: &[u8],
        timeout: Duration,
    ) -> Result<Vec<u8>, transport::Error> {
        match self {
            AppTransport::Demo(t) => t.transact_record_table(request, timeout),
            AppTransport::Usb(t, _) => t.transact_record_table(request, timeout),
            AppTransport::Cdc(t) => t.transact_record_table(request, timeout),
        }
    }

    fn transact_settings(
        &mut self,
        request: &[u8],
        timeout: Duration,
    ) -> Result<Vec<u8>, transport::Error> {
        match self {
            AppTransport::Demo(t) => t.transact_settings(request, timeout),
            AppTransport::Usb(t, _) => t.transact_settings(request, timeout),
            AppTransport::Cdc(t) => t.transact_settings(request, timeout),
        }
    }

    /// The settings write goes over the meter's CDC port \[device\]: the
    /// demo device's CDC persona (sharing its settings), the real meter's
    /// serial port found by its USB serial number. A CDC pipe is that port
    /// itself.
    fn settings_pipe(&mut self) -> Option<Result<Box<dyn Transport>, transport::Error>> {
        match self {
            AppTransport::Demo(t) => Some(Ok(Box::new(t.cdc_twin()))),
            AppTransport::Usb(_, serial) => Some(match serial.as_deref() {
                Some(serial) => {
                    CdcTransport::open(serial).map(|t| Box::new(t) as Box<dyn Transport>)
                }
                None => Err(transport::Error::NotFound),
            }),
            AppTransport::Cdc(_) => None,
        }
    }

    fn recover(&mut self) -> Result<(), transport::Error> {
        match self {
            AppTransport::Demo(t) => t.recover(),
            AppTransport::Usb(t, _) => t.recover(),
            AppTransport::Cdc(t) => t.recover(),
        }
    }

    fn adcnew_send(&mut self, request: AdcnewRequest) -> Result<(), transport::Error> {
        match self {
            AppTransport::Demo(t) => t.adcnew_send(request),
            AppTransport::Usb(t, _) => t.adcnew_send(request),
            AppTransport::Cdc(t) => t.adcnew_send(request),
        }
    }

    fn adcnew_frames(&mut self) -> Result<Vec<AdcnewFrame>, transport::Error> {
        match self {
            AppTransport::Demo(t) => t.adcnew_frames(),
            AppTransport::Usb(t, _) => t.adcnew_frames(),
            AppTransport::Cdc(t) => t.adcnew_frames(),
        }
    }
}

/// Opens what the picker lists: the demo device for a demo entry, the
/// meter [`listed_entry`] finds otherwise.
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
        let listed = discovery::list_devices()?;
        let entry = listed_entry(device, &listed).ok_or(transport::Error::NotFound)?;
        let serial = entry.serial.clone();
        Ok(AppTransport::Usb(BulkTransport::open(entry)?, serial))
    }
}

/// The meter `device` names in what the OS lists now: by serial when it has
/// one, wherever the meter sits (the address can move again between the
/// watcher's look and the open), else by bus and address.
pub fn listed_entry<'a>(
    device: &DeviceListing,
    listed: &'a [DeviceEntry],
) -> Option<&'a DeviceEntry> {
    listed.iter().find(|e| match &device.serial {
        Some(_) => e.serial == device.serial,
        None => device.bus.as_ref() == Some(&e.bus) && device.address == Some(e.address),
    })
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

/// First wait before the next automatic reconnect of a meter that failed to
/// open (FR 4PJRVK); doubles with each failed try. Longer than a few hotplug
/// polls, so a meter held by another app is not hammered.
const FIRST_BACK_OFF_MS: u64 = 2_000;

/// The longest a meter that keeps failing to open waits for its next try.
const MOST_BACK_OFF_MS: u64 = 60_000;

/// Paces a session's automatic reconnects (FR 4PJRVK): every try that does
/// not end in samples pushes the next one further out, up to the cap.
#[derive(Debug, Default)]
struct ReconnectBackOff {
    /// Tries since the meter last delivered samples. Shared with the
    /// session's event sink, which clears it when samples arrive.
    tries: Arc<AtomicU32>,
    /// Earliest time of the next try, once there has been one.
    next_try_ms: u64,
}

impl ReconnectBackOff {
    /// Whether a reconnect may start at `now_ms`.
    fn due(&self, now_ms: u64) -> bool {
        self.tries.load(Ordering::SeqCst) == 0 || now_ms >= self.next_try_ms
    }

    /// A reconnect starts at `now_ms`.
    fn tried(&mut self, now_ms: u64) {
        let earlier = self.tries.fetch_add(1, Ordering::SeqCst);
        let delay = FIRST_BACK_OFF_MS.saturating_mul(1 << earlier.min(16));
        self.next_try_ms = now_ms.saturating_add(delay.min(MOST_BACK_OFF_MS));
    }

    /// Start over: the next try may run at once.
    fn reset(&self) {
        self.tries.store(0, Ordering::SeqCst);
    }
}

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
    /// Counts the PDM sessions opening, open or closing on this meter
    /// ([`crate::trigger::PdmHold`]). Wider than [`Meter::trigger`], which is
    /// empty during the handshake and while a taken session closes.
    pdm_holds: Arc<AtomicUsize>,
    /// Counts the recording starts on this meter that are not settled yet
    /// ([`worker::RecordingStartHold`]).
    recording_starts: Arc<AtomicUsize>,
    /// When the next automatic reconnect may start (FR 4PJRVK).
    back_off: ReconnectBackOff,
    /// The worker threads taken to be closed outside the manager lock; each
    /// holds the meter until it is gone (FR AXHK2F).
    closing_workers: Vec<Weak<()>>,
    /// The PDM counts of the sessions this one replaced, while a PDM session
    /// on them has not finished closing (FR AXHK2F).
    closing_holds: Vec<Arc<AtomicUsize>>,
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

    /// A session of this meter is on its way out of the manager's sight: its
    /// worker was taken and still runs, or a PDM session is held that is not
    /// the one in the slot (opening, or taken and closing), here or on a
    /// session this one replaced. Neither can be released from here, so what
    /// holds the meter is not yet known to be gone.
    fn closing(&self) -> bool {
        // The holds first: a session leaving the slot between the two reads
        // then counts as closing.
        let holds = self.pdm_holds.load(Ordering::SeqCst);
        let in_slot = usize::from(lock(&self.trigger).is_some());
        holds > in_slot
            || self.closing_workers.iter().any(|w| w.strong_count() > 0)
            || self
                .closing_holds
                .iter()
                .any(|h| h.load(Ordering::SeqCst) > 0)
    }

    /// `worker` leaves this meter and still runs: it holds the meter until
    /// its thread is gone.
    fn watch_closing(&mut self, worker: &WorkerHandle) {
        self.closing_workers.retain(|w| w.strong_count() > 0);
        self.closing_workers.push(worker.thread_alive());
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
    /// The meters the firmware update took (FR 1HHT03).
    flash_targets: Arc<Mutex<Vec<String>>>,
    /// The running update, shared with its [`FlashLease`].
    flash_run: Arc<Mutex<Option<FlashRun>>>,
    /// The ended update, kept for one read by a reloaded page (AC-AXHK2M.2);
    /// shared with its [`FlashLease`].
    flash_ended: Arc<Mutex<Option<FlashStatus>>>,
    /// The simulator serials given to second (third, ...) demo meters, by
    /// serial: the demo device id that got each (FR 1HHT03).
    demo_serials: HashMap<String, String>,
    /// Where the session events go (AC-4PJRVM.5).
    log: AppLog,
}

/// Why a request is refused while a firmware update runs.
const FLASH_REFUSAL: &str =
    "A firmware update is running. Wait for it to finish before using the meter.";

/// Why an update does not start while a session it would release is closing.
const FLASH_WHILE_CLOSING: &str =
    "A session on this meter is still closing. Wait a moment, then start the update again.";

/// Why an update does not start past a recording start not settled yet.
const FLASH_WHILE_RECORDING_STARTS: &str =
    "A recording is starting on this meter. Wait a moment, then start the update again.";

/// A recording start registered with the manager and not sent yet: until it
/// settles, a firmware update of that meter is refused (AC-AXHK2F.5).
/// Dropping it gives the start up.
#[derive(Debug)]
pub struct RecordingStart {
    control: worker::WorkerControl,
    hold: worker::RecordingStartHold,
}

impl RecordingStart {
    /// Record to a new file at `path`. The start stays registered past a
    /// timed-out answer, until the worker has applied the request.
    pub fn record_to(self, path: &Path) -> Result<(), UiError> {
        self.control.record_to(path, self.hold)
    }
}

/// Exclusive use of a meter for a firmware update (FR V9SK55, AC .4): while
/// it is held the manager refuses that meter's measurement stream, PD
/// capture, recording and triggering, a second update, and any real meter
/// that may be that meter (AC-4PJRVM.4). Dropping it releases them.
#[derive(Debug)]
pub struct FlashLease {
    device: DeviceListing,
    flashing: Arc<AtomicBool>,
    targets: Arc<Mutex<Vec<String>>>,
    control: Arc<FlashControl>,
    run: Arc<Mutex<Option<FlashRun>>>,
    ended: Arc<Mutex<Option<FlashStatus>>>,
    /// The update reported how it ended.
    finished: bool,
    log: AppLog,
}

/// The running update: its meter and its control.
#[derive(Debug)]
struct FlashRun {
    device: DeviceListing,
    control: Arc<FlashControl>,
}

/// A firmware update as the page sees it: running, or ended and not read yet.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FlashStatus {
    /// The meter being updated.
    pub device_id: String,
    /// The latest stage, `None` before the first.
    pub stage: Option<FlashStage>,
    /// The replug prompt the update waits on, if any.
    pub replug: Option<FlashPrompt>,
    /// How it ended, once it has.
    pub outcome: Option<FlashEnd>,
}

/// A session a firmware update would release, as the prompt names it
/// (FR AXHK2F). The page sends back the ones the user agreed to release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlashHold {
    /// The session's id.
    pub id: String,
    /// Its name for the prompt.
    pub label: String,
    /// It is recording.
    pub recording: bool,
    /// Which recording, so a consent covers that one and no later one on
    /// the same session (AC-AXHK2F.5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recording_id: Option<u64>,
}

impl FlashHold {
    /// Whether the consent in `agreed` covers this session: it names the
    /// session, and names the recording if there is one now.
    fn covered_by(&self, agreed: &[FlashHold]) -> bool {
        agreed
            .iter()
            .any(|a| a.id == self.id && (!self.recording || a.recording_id == self.recording_id))
    }
}

impl FlashLease {
    /// The meter the update was given.
    pub fn device(&self) -> &DeviceListing {
        &self.device
    }

    /// The update's stop request and stage.
    pub fn control(&self) -> &Arc<FlashControl> {
        &self.control
    }

    /// Where the update is logged, naming the meter it was given.
    pub fn session_log(&self) -> SessionLog {
        let device = &self.device;
        SessionLog::new(self.log.clone(), device.id.clone(), device.model.clone())
    }

    /// Release the meter, keeping how the update ended for one
    /// [`flash_status`](DeviceManager::flash_status) read (AC-AXHK2M.2).
    pub fn finish(mut self, done: &Result<FlashDone, UiError>) {
        self.keep_outcome(done);
    }

    fn keep_outcome(&mut self, done: &Result<FlashDone, UiError>) {
        self.finished = true;
        *self.ended.lock().unwrap_or_else(PoisonError::into_inner) = Some(FlashStatus {
            device_id: self.device.id.clone(),
            stage: self.control.stage(),
            replug: None,
            outcome: Some(done.into()),
        });
    }
}

impl Drop for FlashLease {
    fn drop(&mut self) {
        // An update that left without finish() may have stopped anywhere.
        if !self.finished {
            self.keep_outcome(&Err(UiError {
                kind: ErrorKind::Internal,
                message: "The update stopped and did not report how it ended.".to_owned(),
                hint: Some(crate::firmware_flash::RECOVERY_HINT.to_owned()),
                sessions: Vec::new(),
            }));
        }
        *self.run.lock().unwrap_or_else(PoisonError::into_inner) = None;
        // Targets before the flag: a new flash waits until both are clear.
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
        // Read after the swap: a capture that starts or stops meanwhile also
        // reaches the new page as an event, which it applies after this answer.
        let pd_capturing = self.control.pd_capturing().unwrap_or(false);
        Some(AttachedSession {
            device_id,
            model: info.model,
            firmware_version: info.firmware_version,
            serial: Some(info.serial).filter(|s| !s.is_empty()),
            rate_sps: info.rate_sps,
            rate_locked: info.rate_locked,
            session_id: info.session_id,
            recording: recording.map(|(rows, elapsed_ms, path)| RecordingSnapshot {
                rows,
                elapsed_ms,
                path,
            }),
            pd_capturing,
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
#[derive(Debug)]
pub struct TriggerOpener {
    demo: bool,
    serial: Option<String>,
    now_ms: u64,
    /// Covers the handshake, before the session reaches the manager.
    _hold: crate::trigger::PdmHold,
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

/// Wrap `sink` so the session's connect and end, and a recording that fails
/// (FR AXHK2G), reach `log` (AC-4PJRVM.5). `id` and `model` come from the
/// listing; the serial stays out of the log.
fn log_sink(log: AppLog, id: String, model: String, mut sink: impl EventSink) -> impl EventSink {
    let session = SessionLog::new(log, id, model);
    let mut connected = false;
    move |event: BackendEvent| {
        match &event {
            BackendEvent::Status(Status::Connected(_)) => {
                connected = true;
                session.event("connected");
            }
            BackendEvent::Status(Status::Disconnected { error }) => match error {
                None => session.event("disconnected"),
                Some(e) if connected && matches!(e.kind, ErrorKind::Disconnected) => {
                    session.event("unplugged");
                }
                Some(e) if e.is_unplugged() => {
                    session.event("meter gone at open");
                }
                Some(e) => session.failure(e),
            },
            BackendEvent::Recording(RecordingEvent::Failed { error, .. }) => {
                session.failure(error);
            }
            _ => {}
        }
        sink(event);
    }
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
/// (FR TGE839). Samples clear the count of failed reconnects in `tries`
/// (FR 4PJRVK).
fn waiting_sink(
    waiting: Waiting,
    id: String,
    serial: String,
    history: Arc<Mutex<History>>,
    tries: Arc<AtomicU32>,
    mut sink: impl EventSink,
) -> impl EventSink {
    move |event: BackendEvent| {
        if matches!(&event, BackendEvent::Batch(batch) if !batch.samples.is_empty()) {
            tries.store(0, Ordering::SeqCst);
        }
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
            flash_run: Arc::default(),
            flash_ended: Arc::default(),
            demo_serials: HashMap::new(),
            log: AppLog::default(),
        }
    }

    /// Log the events of the sessions opened from now on to `log`.
    pub fn set_app_log(&mut self, log: AppLog) {
        self.log = log;
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

    /// Refuse connecting `device` while a firmware update may hold it
    /// (AC-4PJRVM.4): a real meter with the target's serial, or with no
    /// serial to compare on either side, can be the target at a new address.
    /// So can one that is not in application mode, whatever its serial says.
    fn ensure_not_flash_target(&self, device: &DeviceListing) -> Result<(), UiError> {
        let run = self
            .flash_run
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let Some(target) = run.as_ref().map(|r| &r.device) else {
            return Ok(());
        };
        let real = target.demo_scenario.is_none() && device.demo_scenario.is_none();
        let unsure = target.serial.is_none() || device.serial.is_none();
        let application = device.mode == DeviceMode::Application;
        if real && (unsure || !application || target.serial == device.serial) {
            return Err(UiError::usage(FLASH_REFUSAL));
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

    /// The sessions an update of `device` lets go of, as indices into
    /// [`DeviceManager::meters`]; ended ones are among them, for their waits.
    /// A real session that cannot be told apart from a real target counts:
    /// without a serial on both sides it may be the same meter.
    fn flash_scope(&self, device: &DeviceListing) -> Vec<usize> {
        // The session of that meter (by id, or by serial after a replug).
        let same = |m: &Meter| {
            m.named(&device.id)
                || (device.demo_scenario.is_none()
                    && m.device.demo_scenario.is_none()
                    && device.serial.is_some()
                    && m.device.serial == device.serial)
        };
        let any = self.meters.iter().any(same);
        // No session matched: a real meter with no serial to compare may
        // still be the target, seen at another address.
        let unsure = |m: &Meter| {
            !any && device.demo_scenario.is_none()
                && m.device.demo_scenario.is_none()
                && (device.serial.is_none() || m.device.serial.is_none())
        };
        self.meters
            .iter()
            .enumerate()
            .filter(|(_, m)| same(m) || unsure(m))
            .map(|(i, _)| i)
            .collect()
    }

    /// The running sessions among `scope`: the ones releasing would end.
    fn flash_holds_in(&self, scope: &[usize]) -> Vec<FlashHold> {
        scope
            .iter()
            .filter_map(|&i| {
                let m = &self.meters[i];
                m.running_worker().map(|worker| {
                    let recording_id = worker.recording_id();
                    FlashHold {
                        id: m.id.clone(),
                        label: m.device.label.clone(),
                        recording: recording_id.is_some(),
                        recording_id,
                    }
                })
            })
            .collect()
    }

    /// The sessions an update of the listed device `id` would end, for the
    /// prompt (FR AXHK2F); none for a device that is not listed.
    pub fn flash_holds(&self, id: &str) -> Vec<FlashHold> {
        self.attached(id).map_or_else(
            |_| Vec::new(),
            |d| self.flash_holds_in(&self.flash_scope(&d)),
        )
    }

    /// Take the listed device `id` for a firmware update (FR V9SK55,
    /// AC .4): that meter's trigger session and measurement stream close
    /// first (waiting for the worker to let go of the pipe), its wait for an
    /// unplug ends, and that meter is refused until the lease is dropped; the
    /// others keep running (AC-4PJRVJ.2). Refused, with nothing released or
    /// sent, unless `agreed` covers every session of
    /// [`flash_holds`](Self::flash_holds), or while a session in its scope
    /// is still closing outside the manager lock or has a recording start
    /// not settled yet (FR AXHK2F). A bootloader-mode meter can be taken too.
    pub fn begin_flash(&mut self, id: &str, agreed: &[FlashHold]) -> Result<FlashLease, UiError> {
        self.ensure_not_flashing()?;
        let device = self.attached(id)?;
        let scope = self.flash_scope(&device);
        if scope.iter().any(|&i| self.meters[i].closing()) {
            return Err(UiError::usage(FLASH_WHILE_CLOSING));
        }
        // The starts first: one found settled has its recording id out by now.
        let starting = scope
            .iter()
            .any(|&i| self.meters[i].recording_starts.load(Ordering::SeqCst) > 0);
        let holds = self.flash_holds_in(&scope);
        let covered = holds.iter().all(|hold| hold.covered_by(agreed));
        // A recording in sight is asked about at once: its own start may be
        // the one not settled yet, and the prompt can answer for it.
        if starting && (covered || !holds.iter().any(|hold| hold.recording)) {
            return Err(UiError::usage(FLASH_WHILE_RECORDING_STARTS));
        }
        if !covered {
            return Err(UiError::flash_needs_consent(holds));
        }
        let mut targets: Vec<String> = scope.iter().map(|&i| self.meters[i].id.clone()).collect();
        if !targets.iter().any(|t| t == id) {
            targets.push(id.to_owned());
        }
        // The targets are in place before the flag is raised, so the other
        // meters are never refused, not even for a moment.
        *self
            .flash_targets
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = targets;
        let control = Arc::new(FlashControl::default());
        *self
            .flash_run
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(FlashRun {
            device: device.clone(),
            control: Arc::clone(&control),
        });
        *self
            .flash_ended
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = None;
        self.flashing.store(true, Ordering::SeqCst);
        for i in scope {
            let m = &mut self.meters[i];
            m.release();
            forget_wait(&self.waiting, &m.id);
        }
        Ok(FlashLease {
            device,
            flashing: Arc::clone(&self.flashing),
            targets: Arc::clone(&self.flash_targets),
            control,
            run: Arc::clone(&self.flash_run),
            ended: Arc::clone(&self.flash_ended),
            finished: false,
            log: self.log.clone(),
        })
    }

    /// The running firmware update, else the ended one not read yet, which
    /// this read takes (AC-AXHK2M.2).
    pub fn flash_status(&self) -> Option<FlashStatus> {
        let run = self
            .flash_run
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        match run.as_ref() {
            Some(r) => Some(FlashStatus {
                device_id: r.device.id.clone(),
                stage: r.control.stage(),
                replug: r.control.replug(),
                outcome: None,
            }),
            None => self
                .flash_ended
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take(),
        }
    }

    /// Ask the running firmware update to stop (AC-4PJRVJ.1); refused when
    /// none runs or its erase has begun.
    pub fn cancel_flash(&self) -> Result<(), UiError> {
        let run = self
            .flash_run
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        match run.as_ref() {
            Some(r) => r.control.cancel(),
            None => Err(UiError::usage("No firmware update is running.")),
        }
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

    /// Arm a wait for `serial` on session `id`, as an unplug does.
    #[cfg(test)]
    pub(crate) fn arm_wait(&self, id: &str, serial: &str) {
        self.waiting
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((id.to_owned(), serial.to_owned()));
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
    /// its own. A meter whose reconnect failed is tried again only once its
    /// back-off has passed, or at once when it left the list and came back
    /// (FR 4PJRVK). A firmware update holds its target only: the meters
    /// that cannot be told from it keep waiting, the others come back
    /// (AC-AXHK2M.3).
    pub fn devices_changed(&mut self, devices: &[DeviceListing]) -> bool {
        let waits = self
            .waiting
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let now_ms = self.clock.now_ms();
        let mut started = false;
        for (id, serial) in waits {
            let Some(index) = self.meters.iter().position(|m| m.id == id) else {
                continue;
            };
            let listed = devices
                .iter()
                .find(|d| d.demo_scenario.is_none() && d.serial.as_deref() == Some(serial.as_str()))
                .cloned();
            // The same checks as a connect by hand, before anything is opened.
            let held = self.ensure_not_flashing_for(&id).is_err()
                || listed.as_ref().is_some_and(|d| {
                    self.ensure_not_flashing_for(&d.id).is_err()
                        || self.ensure_not_flash_target(d).is_err()
                });
            let meter = &mut self.meters[index];
            if meter.running_worker().is_some() || meter.stop.load(Ordering::SeqCst) {
                // Still running, or the page that waited is gone.
                continue;
            }
            let Some(device) = listed else {
                meter.back_off.reset();
                continue;
            };
            if held || !meter.back_off.due(now_ms) {
                continue;
            }
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
                meter.watch_closing(&old);
                old.retire();
            }
            // The id the meter last had, which is not `id` after an earlier
            // replug moved it.
            let old_id = &meter.device.id;
            let moved = if *old_id == device.id {
                old_id.clone()
            } else {
                format!("{old_id} to {}", device.id)
            };
            self.log.line(&format!("replug {moved} ({})", device.model));
            meter.device = device;
            meter.back_off.tried(now_ms);
            self.open_session(index, rate);
            started = true;
        }
        started
    }

    /// Every attached meter plus the demo entries; never opens a device.
    pub fn list_devices(&self) -> Vec<DeviceListing> {
        list_devices_from((self.discover)())
    }

    /// The listed device `id`, refused when it is not attached any more.
    fn attached(&self, id: &str) -> Result<DeviceListing, UiError> {
        self.list_devices()
            .into_iter()
            .find(|d| d.id == id)
            .ok_or_else(|| UiError::usage("That meter is no longer attached."))
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
        self.ensure_not_flashing_for(id)?;
        if self.meter(id).is_some_and(|m| m.running_worker().is_some()) {
            return Err(UiError::usage(
                "This meter is already connected. Disconnect it first.",
            ));
        }
        let rate = supported_rate(rate_sps)?;
        let mut device = self.attached(id)?;
        self.ensure_not_flash_target(&device)?;
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
            // One still closing stays visible to a firmware update's start;
            // a new session of the same meter inherits what is closing below.
            (adding && waited || m.closing()) && !same_meter
        };
        let (kept, ended): (Vec<Meter>, Vec<Meter>) =
            std::mem::take(&mut self.meters).into_iter().partition(keep);
        self.meters = kept;
        let mut closing_workers = Vec::new();
        let mut closing_holds = Vec::new();
        for mut m in ended {
            self.stop_waiting_for(&m.id);
            m.release();
            closing_workers.append(&mut m.closing_workers);
            closing_holds.append(&mut m.closing_holds);
            // Its slot is empty now: what it still counts is on the way out.
            closing_holds.push(m.pdm_holds);
        }
        closing_holds.retain(|h| h.load(Ordering::SeqCst) > 0);
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
            pdm_holds: Arc::default(),
            recording_starts: Arc::default(),
            back_off: ReconnectBackOff::default(),
            closing_workers,
            closing_holds,
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
        // The id the meter has now: a replug can move it off `m.id`.
        let session_log =
            SessionLog::new(self.log.clone(), device.id.clone(), device.model.clone());
        let sink = log_sink(
            self.log.clone(),
            device.id.clone(),
            device.model.clone(),
            trigger_sink(
                Arc::clone(&m.trigger),
                history_sink(
                    Arc::clone(&history),
                    listing_sink(device.clone(), live_sink(Arc::clone(&m.live))),
                ),
            ),
        );
        let sink: Box<dyn FnMut(BackendEvent) + Send> = match waits_for {
            Some(serial) => Box::new(waiting_sink(
                waiting,
                m.id.clone(),
                serial,
                history,
                Arc::clone(&m.back_off.tries),
                sink,
            )),
            None => Box::new(sink),
        };
        let pdm_holds = Arc::clone(&m.pdm_holds);
        m.worker = Some(worker::spawn_watching_pdm(
            move || opener.open_source(&device, source),
            clock,
            config,
            store,
            sink,
            Arc::clone(&m.stop),
            Arc::new(move || pdm_holds.load(Ordering::SeqCst) > 0),
            session_log,
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

    /// [`DeviceManager::rate_control`] of meter `id`. A refusal adds its line
    /// to the log (FR AXHK2G): the worker never hears of it.
    pub fn rate_control_for(
        &self,
        id: &str,
        rate_sps: u32,
    ) -> Result<(worker::WorkerControl, Rate), UiError> {
        self.checked_rate_control(id, rate_sps)
            .inspect_err(|error| self.session_log_for(id).failure(error))
    }

    /// The control and rate of a change meter `id` may make now.
    fn checked_rate_control(
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

    /// The log as meter `id`'s session writes to it, for what fails before a
    /// request reaches its worker (FR AXHK2G). It names the meter by the id
    /// it has now; one the manager never saw goes by the id it was asked for.
    pub fn session_log_for(&self, id: &str) -> SessionLog {
        let (id, model) = self.meter(id).map_or((id, "unknown model"), |m| {
            (m.device.id.as_str(), m.device.model.as_str())
        });
        SessionLog::new(self.log.clone(), id.to_owned(), model.to_owned())
    }

    /// Register a recording start on meter `id`, so a caller can wait for the
    /// worker's answer after releasing its lock on the manager. Taken under
    /// the lock a firmware update's start takes, so either the update sees
    /// the start or the start is refused (AC-AXHK2F.5).
    pub fn recording_start_for(&self, id: &str) -> Result<RecordingStart, UiError> {
        let control = self.control_for(id)?;
        let hold = self
            .meter(id)
            .map(|m| worker::RecordingStartHold::new(&m.recording_starts))
            .ok_or_else(|| UiError::usage(NOT_CONNECTED))?;
        Ok(RecordingStart { control, hold })
    }

    /// Record the open session's stream to a new file at `path`.
    pub fn start_recording(&self, path: &Path) -> Result<(), UiError> {
        self.start_recording_for(&self.current_id(), path)
    }

    /// Record meter `id`'s stream to a new file at `path`.
    pub fn start_recording_for(&self, id: &str, path: &Path) -> Result<(), UiError> {
        self.recording_start_for(id)?.record_to(path)
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
            .and_then(|m| {
                let worker = m.worker.take()?;
                m.watch_closing(&worker);
                Some(worker)
            })
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
                // that failed to open, gets another try (FR TGE839), paced by
                // the session's back-off (FR 4PJRVK).
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
        worker.hold_until_closed(crate::trigger::PdmHold::new(&meter.pdm_holds));
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
        // The trigger picks a serial port by serial, or the only one without.
        self.ensure_not_flash_target(&meter.device)?;
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
            _hold: crate::trigger::PdmHold::new(&meter.pdm_holds),
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

    // ---- FR AXHK2F: a firmware update never ends a session unasked ----

    /// Frame types of the safe command set (`docs/protocol.md`).
    const SAFE_TYPES: [u8; 8] = [0x02, 0x03, 0x0C, 0x0E, 0x0F, 0x40, 0x44, 0x4C];

    const AT_4: &str = "usb:1:4";
    const AT_5: &str = "usb:1:5";
    const AT_6: &str = "usb:1:6";

    /// A real meter at `address`; `None` is one that reports no serial. Every
    /// serial here is synthetic.
    fn entry(address: u8, serial: Option<&str>) -> DeviceEntry {
        DeviceEntry {
            model: Model::Km003c,
            pid: 0x0063,
            serial: serial.map(str::to_owned),
            product: Some("POWER-Z KM003C".to_owned()),
            interface_string: Some("APP".to_owned()),
            bus: "1".to_owned(),
            address,
        }
    }

    /// Stalls a fake meter's pipe, so its worker answers no command.
    #[derive(Default)]
    struct Stall {
        /// The meters whose pipe waits when used.
        meters: Mutex<Vec<String>>,
        /// How many workers are waiting on a pipe now.
        waiting: AtomicUsize,
    }

    impl Stall {
        fn stalls(&self, id: &str) -> bool {
            self.meters.lock().unwrap().iter().any(|held| held == id)
        }

        /// Let every stalled worker go on.
        fn end(&self) {
            self.meters.lock().unwrap().clear();
        }
    }

    /// The simulator behind a plug, logging the type of every request.
    struct FakeMeter {
        sim: SimDevice,
        id: String,
        pulled: Arc<Mutex<Vec<String>>>,
        sent: Arc<Mutex<Vec<u8>>>,
        stall: Arc<Stall>,
        /// How many of the next queue polls the meter refuses, still on the
        /// line.
        refused_polls: Arc<AtomicUsize>,
    }

    impl FakeMeter {
        fn check_plug(&self) -> Result<(), transport::Error> {
            if self.stall.stalls(&self.id) {
                self.stall.waiting.fetch_add(1, Ordering::SeqCst);
                // Bounded: an update that wrongly starts joins this worker,
                // and must fail its test rather than hang it.
                let deadline = std::time::Instant::now() + Duration::from_secs(10);
                while self.stall.stalls(&self.id) {
                    if std::time::Instant::now() >= deadline {
                        self.stall.end();
                    }
                    std::thread::sleep(Duration::from_millis(1));
                }
                self.stall.waiting.fetch_sub(1, Ordering::SeqCst);
            }
            if self.pulled.lock().unwrap().contains(&self.id) {
                return Err(transport::Error::Disconnected);
            }
            Ok(())
        }
    }

    impl Transport for FakeMeter {
        fn transact(
            &mut self,
            request: &[u8],
            timeout: Duration,
        ) -> Result<Vec<u8>, transport::Error> {
            self.check_plug()?;
            if let Some(first) = request.first() {
                self.sent.lock().unwrap().push(first & 0x7F);
            }
            let queue_poll = powerz_protocol::frame::get_data_mask(request).is_some_and(|m| {
                m.bits() & powerz_protocol::frame::Attribute::ADC_QUEUE.bits() != 0
            });
            if queue_poll
                && self
                    .refused_polls
                    .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
                    .is_ok()
            {
                return Ok(vec![0x06, request[1], 0x00, 0x00]);
            }
            self.sim.transact(request, timeout)
        }

        fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, transport::Error> {
            self.check_plug()?;
            self.sim.read(timeout)
        }
    }

    struct FakeOpener {
        clock: powerz_sim::SimClock,
        pulled: Arc<Mutex<Vec<String>>>,
        sent: Arc<Mutex<Vec<u8>>>,
        stall: Arc<Stall>,
        refused_polls: Arc<AtomicUsize>,
    }

    impl Opener for FakeOpener {
        type Transport = FakeMeter;
        fn open(&self, device: &DeviceListing) -> Result<FakeMeter, transport::Error> {
            Ok(FakeMeter {
                sim: SimDevice::with_clock(Persona::Bulk, Scenario::Load5v1a, self.clock.clone()),
                id: device.id.clone(),
                pulled: Arc::clone(&self.pulled),
                sent: Arc::clone(&self.sent),
                stall: Arc::clone(&self.stall),
                refused_polls: Arc::clone(&self.refused_polls),
            })
        }
    }

    /// A manager over fake real meters; the demo device is listed as usual.
    struct Rig {
        m: DeviceManager<FakeOpener, powerz_sim::SimClock>,
        pulled: Arc<Mutex<Vec<String>>>,
        sent: Arc<Mutex<Vec<u8>>>,
        stall: Arc<Stall>,
        refused_polls: Arc<AtomicUsize>,
        files: Vec<std::path::PathBuf>,
    }

    impl Rig {
        fn new(entries: Vec<DeviceEntry>) -> Rig {
            let clock = powerz_sim::SimClock::new(1_790_000_000_000);
            let pulled: Arc<Mutex<Vec<String>>> = Arc::default();
            let sent: Arc<Mutex<Vec<u8>>> = Arc::default();
            let stall: Arc<Stall> = Arc::default();
            let refused_polls: Arc<AtomicUsize> = Arc::default();
            let m = DeviceManager::new(
                move || Ok(entries.clone()),
                FakeOpener {
                    clock: clock.clone(),
                    pulled: Arc::clone(&pulled),
                    sent: Arc::clone(&sent),
                    stall: Arc::clone(&stall),
                    refused_polls: Arc::clone(&refused_polls),
                },
                clock,
            );
            Rig {
                m,
                pulled,
                sent,
                stall,
                refused_polls,
                files: Vec::new(),
            }
        }

        /// Connect meter `id` and wait for its first samples.
        fn stream(&mut self, id: &str) -> std::sync::mpsc::Receiver<BackendEvent> {
            let (tx, rx) = std::sync::mpsc::channel();
            self.m
                .connect(id, 50, move |e| {
                    let _ = tx.send(e);
                })
                .unwrap_or_else(|e| panic!("connect {id}: {e:?}"));
            wait_for(
                &rx,
                |e| matches!(e, BackendEvent::Batch(b) if !b.samples.is_empty()),
            );
            rx
        }

        /// A fresh recording file, removed with the rig.
        fn fresh_file(&mut self) -> std::path::PathBuf {
            let name = format!(
                "open-powerz-axhk2f-{}-{:?}-{}.csv",
                std::process::id(),
                std::thread::current().id(),
                self.files.len()
            );
            let path = std::env::temp_dir().join(name);
            self.files.push(path.clone());
            path
        }

        /// Start a recording on meter `id`, to a fresh file.
        fn record(&mut self, id: &str) {
            let path = self.fresh_file();
            self.m
                .start_recording_for(id, &path)
                .unwrap_or_else(|e| panic!("record on {id}: {e:?}"));
        }

        /// Pull meter `id`'s cable: its pipe fails from now on.
        fn unplug(&self, id: &str) {
            self.pulled.lock().unwrap().push(id.to_owned());
        }

        /// Hang meter `id`'s worker on its pipe until [`Stall::end`]; from
        /// the return on it applies no command.
        fn stall(&self, id: &str) {
            self.stall.meters.lock().unwrap().push(id.to_owned());
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            while self.stall.waiting.load(Ordering::SeqCst) == 0 {
                assert!(std::time::Instant::now() < deadline, "{id} never stalled");
                std::thread::sleep(Duration::from_millis(1));
            }
        }

        /// Open a trigger session on meter `id`.
        fn open_trigger(&mut self, id: &str) {
            let clock = powerz_sim::SimClock::new(1_790_000_000_000);
            let pdm = powerz_sim::SimPdm::new(powerz_sim::SimCharger::Epr140W, clock.clone());
            let worker = crate::trigger::TriggerWorker::spawn(pdm.open_port(), clock)
                .expect("the trigger session opens");
            self.m
                .install_trigger_for(id, worker)
                .expect("the trigger session is installed");
        }

        /// Give meter `id` an ended worker whose thread is still alive, as
        /// when it is still delivering its last events to a slow page. The
        /// thread goes on when the returned sender is dropped.
        fn linger_after_the_end(&mut self, id: &str) -> std::sync::mpsc::Sender<()> {
            let (thread_ends, gate) = std::sync::mpsc::channel::<()>();
            let clock = powerz_sim::SimClock::new(1_790_000_000_000);
            let lingering = worker::spawn(
                || -> Result<FakeMeter, transport::Error> { Err(transport::Error::Disconnected) },
                clock,
                WorkerConfig::new(Rate::Sps50, 1),
                move |event: BackendEvent| {
                    if matches!(event, BackendEvent::Status(Status::Disconnected { .. })) {
                        let _ = gate.recv();
                    }
                },
            );
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            while !lingering.is_finished() {
                assert!(std::time::Instant::now() < deadline, "it never ended");
                std::thread::sleep(Duration::from_millis(1));
            }
            let meter = self.m.meter_mut(id).expect("the meter has a session");
            // The ended worker shares the page's stop flag, as in a replug.
            if let Some(ended) = meter.worker.replace(lingering) {
                ended.retire();
            }
            thread_ends
        }

        /// What the `disconnect` command does under the manager lock: both
        /// are taken here and finish closing later, without the lock.
        fn take_to_close(&mut self, id: &str) -> Closing {
            let trigger = self.m.take_trigger_for(id);
            let session = self.m.take_session_for(id);
            Closing { trigger, session }
        }

        /// The manager holds no trace of an update, for the meters in `ids`.
        fn assert_no_update(&self, ids: &[&str]) {
            assert!(!self.m.flashing(), "the update flag stayed up");
            let status = self.m.flash_status();
            assert!(
                status.is_none_or(|s| s.outcome.is_some()),
                "an update is still reported"
            );
            for id in ids {
                assert!(
                    self.m.ensure_not_flashing_for(id).is_ok(),
                    "{id} is still refused for an update"
                );
            }
        }
    }

    impl Drop for Rig {
        fn drop(&mut self) {
            self.stall.end();
            for id in self.m.connected_ids() {
                let _ = self.m.disconnect_for(&id);
            }
            for file in &self.files {
                let _ = std::fs::remove_file(file);
            }
        }
    }

    /// A session the `disconnect` command has taken and not finished closing.
    struct Closing {
        trigger: Option<crate::trigger::TriggerWorker>,
        session: Result<WorkerHandle, UiError>,
    }

    impl Closing {
        /// Back to 5 V and `pdm close`, as the command does first.
        fn close_trigger(&mut self) {
            if let Some(trigger) = self.trigger.take() {
                let _ = trigger.close();
            }
        }

        /// The worker's part alone, with the trigger left closing.
        fn close_session(&mut self) {
            let taken = std::mem::replace(&mut self.session, Err(UiError::usage(NOT_CONNECTED)));
            if let Ok(session) = taken {
                session.disconnect();
            }
        }

        /// The rest of the command, outside the manager lock.
        fn finish(mut self) {
            self.close_trigger();
            if let Ok(session) = self.session {
                session.disconnect();
            }
        }
    }

    /// The refusal of a start that met a session still closing: said in plain
    /// words, and not as a consent question, which no prompt could answer.
    fn assert_refused_while_closing(e: &UiError, what: &str) {
        assert_eq!(e.kind, ErrorKind::Usage, "{what}: {e:?}");
        assert!(e.sessions.is_empty(), "{what}: {e:?}");
        assert!(e.message.ends_with('.'), "{what}: a sentence: {e:?}");
        assert_ne!(e.message, FLASH_REFUSAL, "{what}: no update runs: {e:?}");
        assert_ne!(e.message, NOT_CONNECTED, "{what}: {e:?}");
    }

    /// Receive events until `want` matches one.
    fn wait_for(
        rx: &std::sync::mpsc::Receiver<BackendEvent>,
        mut want: impl FnMut(&BackendEvent) -> bool,
    ) {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            let event = rx.recv_timeout(left).expect("the event never arrived");
            if want(&event) {
                return;
            }
        }
    }

    // ---- FR AXHK2G: the app log records errors and firmware updates ----

    /// A fresh `app.log` path of its own for test `tag`.
    fn fresh_log(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("open-powerz-axhk2g-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create the log folder");
        dir.join("app.log")
    }

    fn log_lines(path: &Path) -> Vec<String> {
        std::fs::read_to_string(path)
            .map(|text| text.lines().map(str::to_owned).collect())
            .unwrap_or_default()
    }

    /// Regression (AC-AXHK2G.1): a meter still on the line refuses a poll
    /// while PDM is open. The session goes on, and until this fix the error
    /// was dropped without a trace.
    #[test]
    fn a_stream_error_that_keeps_the_session_reaches_the_log() {
        let path = fresh_log("stream-error");
        let mut rig = Rig::new(vec![entry(4, Some("SERIAL-AXHK2G"))]);
        rig.m
            .set_app_log(AppLog::open(&path).expect("open app.log"));
        let rx = rig.stream(AT_4);
        rig.open_trigger(AT_4);

        rig.refused_polls.store(1, Ordering::SeqCst);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let is_error = |l: &String| l.contains("protocol");
        while !log_lines(&path).iter().any(is_error) {
            assert!(
                std::time::Instant::now() < deadline,
                "the stream error never reached the log ({} polls still to refuse): {:#?}",
                rig.refused_polls.load(Ordering::SeqCst),
                log_lines(&path)
            );
            std::thread::sleep(Duration::from_millis(10));
        }

        assert_eq!(
            rig.refused_polls.load(Ordering::SeqCst),
            0,
            "the meter refused the poll"
        );
        let found: Vec<String> = log_lines(&path).into_iter().filter(is_error).collect();
        assert_eq!(found.len(), 1, "one line for one error: {found:#?}");
        assert!(found[0].contains(AT_4), "names the meter: {}", found[0]);
        assert!(found[0].contains("KM003C"), "names the model: {}", found[0]);

        // The session is still there, and still streaming.
        assert!(rig.m.connected_ids().iter().any(|id| id == AT_4));
        while rx.try_recv().is_ok() {}
        wait_for(&rx, |e| {
            assert!(
                !matches!(e, BackendEvent::Status(Status::Disconnected { .. })),
                "the error ended the session"
            );
            matches!(e, BackendEvent::Batch(b) if !b.samples.is_empty())
        });
    }

    /// AC-AXHK2G.2: a recording that fails ends through the sink, however it
    /// failed (a write, the last flush, the end of the session).
    #[test]
    fn a_failed_recording_write_adds_one_line_naming_the_meter_and_the_kind() {
        let path = fresh_log("recording");
        let log = AppLog::open(&path).expect("open app.log");
        let got = Arc::new(Mutex::new(Vec::new()));
        let keep = Arc::clone(&got);
        let mut sink = log_sink(log, AT_4.to_owned(), "KM003C".to_owned(), move |e| {
            keep.lock().unwrap().push(e);
        });
        sink(BackendEvent::Recording(
            crate::recording::RecordingEvent::Failed {
                rows: 7,
                error: crate::recording::io_error(&std::io::Error::from(
                    std::io::ErrorKind::PermissionDenied,
                )),
            },
        ));
        let lines = log_lines(&path);
        assert_eq!(lines.len(), 1, "one line for one failure: {lines:#?}");
        assert!(lines[0].contains(AT_4), "names the meter: {}", lines[0]);
        assert!(lines[0].contains("KM003C"), "names the model: {}", lines[0]);
        assert!(
            lines[0].contains("permission_denied"),
            "names the kind: {}",
            lines[0]
        );
        assert_eq!(got.lock().unwrap().len(), 1, "the page still hears of it");
    }

    /// The error of an update start that must be refused.
    fn refused(result: Result<FlashLease, UiError>) -> UiError {
        match result {
            Err(e) => e,
            Ok(lease) => panic!("the update started on {}", lease.device().id),
        }
    }

    /// The sessions a consent refusal names, as the page receives them.
    fn consent_sessions(e: &UiError) -> Vec<(String, bool)> {
        assert_eq!(e.kind, ErrorKind::FlashNeedsConsent, "{e:?}");
        let sent = serde_json::to_value(e).expect("serialises");
        assert_eq!(sent["kind"], "flash_needs_consent");
        sent["sessions"]
            .as_array()
            .expect("the refusal carries its sessions")
            .iter()
            .map(|s| {
                let id = s["id"].as_str().expect("an id").to_owned();
                assert!(s["label"].as_str().is_some_and(|l| !l.is_empty()), "{s}");
                (id, s["recording"].as_bool().expect("a recording flag"))
            })
            .collect()
    }

    fn named(sessions: &[(&str, bool)]) -> Vec<(String, bool)> {
        sessions
            .iter()
            .map(|(id, recording)| ((*id).to_owned(), *recording))
            .collect()
    }

    /// Wait until the session's end has reached the page and its worker is gone.
    fn wait_ended(rig: &Rig, rx: &std::sync::mpsc::Receiver<BackendEvent>, id: &str) {
        wait_for(rx, |e| {
            matches!(e, BackendEvent::Status(Status::Disconnected { .. }))
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while rig.m.connected_ids().iter().any(|c| c == id) {
            assert!(std::time::Instant::now() < deadline, "{id} kept running");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn flash_refuses_an_unsure_recording_session_it_was_not_told_to_end() {
        let mut rig = Rig::new(vec![entry(4, None), entry(5, None)]);
        let _rx = rig.stream(AT_4);
        rig.record(AT_4);

        let e = refused(rig.m.begin_flash(AT_5, &[]));

        assert_eq!(consent_sessions(&e), named(&[(AT_4, true)]));
        assert_eq!(rig.m.running_ids(), vec![AT_4.to_owned()]);
        rig.assert_no_update(&[AT_4, AT_5]);
        rig.m
            .stop_recording_for(AT_4)
            .expect("the recording kept running");
    }

    // ---- AC-AXHK2F.2 ----

    #[test]
    fn flash_holds_names_each_session_an_update_would_release() {
        let mut rig = Rig::new(vec![
            entry(4, Some("SERIAL-A")),
            entry(5, Some("SERIAL-A")),
            entry(6, Some("SERIAL-B")),
        ]);
        let _a = rig.stream(AT_4);
        let _b = rig.stream(AT_6);
        rig.record(AT_4);

        let holds = rig.m.flash_holds(AT_5);

        let named: Vec<_> = holds
            .iter()
            .map(|h| (h.id.as_str(), h.label.as_str(), h.recording))
            .collect();
        assert_eq!(
            named,
            vec![(AT_4, "KM003C — SERIAL-A", true)],
            "the same serial at another address, and not the other meter"
        );
        assert!(rig.m.flash_holds(AT_6).iter().all(|h| !h.recording));
    }

    #[test]
    fn a_session_of_the_target_itself_needs_consent_even_when_it_only_streams() {
        let mut rig = Rig::new(vec![entry(4, Some("SERIAL-A"))]);
        let _rx = rig.stream(AT_4);

        let e = refused(rig.m.begin_flash(AT_4, &[]));

        assert_eq!(consent_sessions(&e), named(&[(AT_4, false)]));
        assert_eq!(rig.m.running_ids(), vec![AT_4.to_owned()]);
        rig.assert_no_update(&[AT_4]);
    }

    #[test]
    fn a_demo_session_of_the_target_needs_consent_too() {
        let demo = Scenario::ALL[0].name();
        let id = format!("demo:{demo}");
        let mut rig = Rig::new(Vec::new());
        let _rx = rig.stream(&id);

        let e = refused(rig.m.begin_flash(&id, &[]));

        assert_eq!(consent_sessions(&e), named(&[(id.as_str(), false)]));
        assert_eq!(rig.m.running_ids(), vec![id]);
    }

    #[test]
    fn a_refused_start_sends_nothing_outside_the_safe_set_and_leaves_no_lock() {
        let mut rig = Rig::new(vec![entry(4, None), entry(5, None)]);
        let _rx = rig.stream(AT_4);

        let _ = refused(rig.m.begin_flash(AT_5, &[]));
        std::thread::sleep(Duration::from_millis(50));

        let sent = rig.sent.lock().unwrap().clone();
        assert!(
            sent.iter().all(|ty| SAFE_TYPES.contains(ty)),
            "an unsafe frame left the host: {sent:02X?}"
        );
        rig.m
            .control_for(AT_4)
            .expect("the session still takes requests");
        rig.m
            .set_rate_for(AT_4, 10)
            .expect("the session still takes requests");
    }

    #[test]
    fn consent_to_some_of_the_sessions_releases_none_of_them() {
        let mut rig = Rig::new(vec![entry(4, None), entry(5, None), entry(6, None)]);
        let _a = rig.stream(AT_4);
        let _b = rig.stream(AT_5);
        let holds = rig.m.flash_holds(AT_6);
        assert_eq!(holds.len(), 2, "{holds:?}");

        let e = refused(rig.m.begin_flash(AT_6, &holds[..1]));

        assert_eq!(
            consent_sessions(&e),
            named(&[(AT_4, false), (AT_5, false)]),
            "the refusal lists all of them, so the prompt can ask once"
        );
        assert_eq!(rig.m.running_ids().len(), 2, "both keep running");
        rig.assert_no_update(&[AT_4, AT_5, AT_6]);
    }

    #[test]
    fn consent_to_every_session_starts_the_update_and_releases_them() {
        let mut rig = Rig::new(vec![entry(4, None), entry(5, None)]);
        let rx = rig.stream(AT_4);
        let holds = rig.m.flash_holds(AT_5);

        let lease = rig
            .m
            .begin_flash(AT_5, &holds)
            .expect("every session agreed");

        assert_eq!(lease.device().id, AT_5);
        wait_ended(&rig, &rx, AT_4);
        assert!(rig.m.flashing());
    }

    #[test]
    fn a_meter_with_no_session_to_release_starts_at_once() {
        let mut rig = Rig::new(vec![entry(4, Some("SERIAL-A"))]);
        assert_eq!(rig.m.flash_holds(AT_4), Vec::new());
        let lease = rig.m.begin_flash(AT_4, &[]).expect("nothing to agree to");
        assert_eq!(lease.device().id, AT_4);
    }

    // ---- AC-AXHK2F.3 ----

    #[test]
    fn an_idle_session_that_may_be_the_target_needs_the_same_consent() {
        for (what, session, target) in [
            ("no serial on the session", None, Some("SERIAL-T")),
            ("no serial on the target", Some("SERIAL-S"), None),
            ("no serial on either side", None, None),
        ] {
            let mut rig = Rig::new(vec![entry(4, session), entry(5, target)]);
            let _rx = rig.stream(AT_4);

            let e = refused(rig.m.begin_flash(AT_5, &[]));

            assert_eq!(consent_sessions(&e), named(&[(AT_4, false)]), "{what}");
            assert_eq!(rig.m.running_ids(), vec![AT_4.to_owned()], "{what}");

            let holds = rig.m.flash_holds(AT_5);
            rig.m.begin_flash(AT_5, &holds).expect("agreed");
        }
    }

    #[test]
    fn a_recording_unsure_session_with_a_serial_on_one_side_needs_consent() {
        for (what, session, target) in [
            ("no serial on the session", None, Some("SERIAL-T")),
            ("no serial on the target", Some("SERIAL-S"), None),
        ] {
            let mut rig = Rig::new(vec![entry(4, session), entry(5, target)]);
            let _rx = rig.stream(AT_4);
            rig.record(AT_4);

            let e = refused(rig.m.begin_flash(AT_5, &[]));

            assert_eq!(consent_sessions(&e), named(&[(AT_4, true)]), "{what}");
            assert_eq!(rig.m.running_ids(), vec![AT_4.to_owned()], "{what}");
            rig.assert_no_update(&[AT_4, AT_5]);
            rig.m
                .stop_recording_for(AT_4)
                .unwrap_or_else(|e| panic!("{what}: the recording ended: {e:?}"));
        }
    }

    #[test]
    fn a_session_that_cannot_be_the_target_is_neither_named_nor_released() {
        let mut rig = Rig::new(vec![entry(4, Some("SERIAL-A")), entry(5, Some("SERIAL-B"))]);
        let _rx = rig.stream(AT_4);
        rig.record(AT_4);

        assert_eq!(rig.m.flash_holds(AT_5), Vec::new());
        let _lease = rig.m.begin_flash(AT_5, &[]).expect("another meter");
        assert_eq!(rig.m.running_ids(), vec![AT_4.to_owned()]);
    }

    /// AC-AXHK2M.3: a trigger opens the meter's serial port, so it needs the
    /// same check as a connect.
    #[test]
    fn a_trigger_is_refused_on_a_meter_that_may_be_the_update_target() {
        let mut rig = Rig::new(vec![entry(4, Some("SERIAL-A")), entry(5, Some("SERIAL-B"))]);
        let _rx = rig.stream(AT_4);
        let _lease = rig.m.begin_flash(AT_5, &[]).expect("another meter");
        assert!(
            rig.m.trigger_opener_for(AT_4).is_ok(),
            "told from the target"
        );

        if let Some(run) = rig.m.flash_run.lock().unwrap().as_mut() {
            run.device.serial = None;
        }
        let e = rig
            .m
            .trigger_opener_for(AT_4)
            .expect_err("it may be the target");
        assert_eq!(e.message, FLASH_REFUSAL);
    }

    #[test]
    fn a_demo_session_is_never_taken_for_a_real_target() {
        let demo = format!("demo:{}", Scenario::ALL[0].name());
        let mut rig = Rig::new(vec![entry(4, None)]);
        let _rx = rig.stream(&demo);

        assert_eq!(rig.m.flash_holds(AT_4), Vec::new());
        let _lease = rig.m.begin_flash(AT_4, &[]).expect("a demo is no meter");
        assert_eq!(rig.m.running_ids(), vec![demo]);
    }

    // ---- AC-AXHK2F.5 ----

    #[test]
    fn a_recording_that_starts_after_the_consent_is_not_covered_by_it() {
        let mut rig = Rig::new(vec![entry(4, None), entry(5, None)]);
        let _rx = rig.stream(AT_4);
        let consent = rig.m.flash_holds(AT_5);
        assert_eq!(consent.len(), 1);
        assert!(!consent[0].recording);

        rig.record(AT_4);
        let e = refused(rig.m.begin_flash(AT_5, &consent));

        assert_eq!(consent_sessions(&e), named(&[(AT_4, true)]));
        assert_eq!(rig.m.running_ids(), vec![AT_4.to_owned()]);
        rig.assert_no_update(&[AT_4, AT_5]);
        rig.m
            .stop_recording_for(AT_4)
            .expect("the recording kept running");
    }

    #[test]
    fn consent_given_while_recording_still_covers_a_recording_stopped_since() {
        let mut rig = Rig::new(vec![entry(4, None), entry(5, None)]);
        let _rx = rig.stream(AT_4);
        rig.record(AT_4);
        let consent = rig.m.flash_holds(AT_5);
        rig.m.stop_recording_for(AT_4).expect("stops");

        rig.m
            .begin_flash(AT_5, &consent)
            .expect("the page ends the recording, then starts the update");
    }

    #[test]
    fn consent_to_a_recording_still_running_covers_it() {
        let mut rig = Rig::new(vec![entry(4, None), entry(5, None)]);
        let _rx = rig.stream(AT_4);
        rig.record(AT_4);
        let consent = rig.m.flash_holds(AT_5);

        rig.m
            .begin_flash(AT_5, &consent)
            .expect("the user agreed to end this recording");
    }

    #[test]
    fn consent_to_a_recording_does_not_cover_the_next_one_on_that_session() {
        let mut rig = Rig::new(vec![entry(4, None), entry(5, None)]);
        let _rx = rig.stream(AT_4);
        rig.record(AT_4);
        let consent = rig.m.flash_holds(AT_5);
        assert!(consent[0].recording);

        rig.m.stop_recording_for(AT_4).expect("stops");
        rig.record(AT_4);
        let e = refused(rig.m.begin_flash(AT_5, &consent));

        assert_eq!(consent_sessions(&e), named(&[(AT_4, true)]));
        assert_eq!(rig.m.running_ids(), vec![AT_4.to_owned()]);
        rig.assert_no_update(&[AT_4, AT_5]);

        let again = rig.m.flash_holds(AT_5);
        rig.m
            .begin_flash(AT_5, &again)
            .expect("consent to the new recording covers it");
    }

    /// The refusal of a start that met a recording start not finished yet.
    fn assert_refused_while_recording_starts(e: &UiError, what: &str) {
        assert_refused_while_closing(e, what);
        assert!(
            e.message.to_lowercase().contains("recording"),
            "{what}: says what it waits for: {e:?}"
        );
    }

    #[test]
    fn a_registered_recording_start_refuses_an_update_with_nothing_released() {
        for target in [AT_4, AT_5] {
            let mut rig = Rig::new(vec![entry(4, None), entry(5, None)]);
            let _rx = rig.stream(AT_4);
            let consent = rig.m.flash_holds(target);
            assert_eq!(consent.len(), 1, "{consent:?}");

            let start = rig.m.recording_start_for(AT_4).expect("registered");

            for agreed in [&consent[..], &[]] {
                let e = refused(rig.m.begin_flash(target, agreed));
                assert_refused_while_recording_starts(&e, target);
            }
            assert_eq!(rig.m.running_ids(), vec![AT_4.to_owned()], "{target}");
            rig.assert_no_update(&[AT_4, AT_5]);
            let sent = rig.sent.lock().unwrap().clone();
            assert!(
                sent.iter().all(|ty| SAFE_TYPES.contains(ty)),
                "an unsafe frame left the host: {sent:02X?}"
            );
            drop(start);
        }
    }

    #[test]
    fn a_completed_recording_start_needs_consent_as_usual() {
        let mut rig = Rig::new(vec![entry(4, None), entry(5, None)]);
        let _rx = rig.stream(AT_4);
        let before = rig.m.flash_holds(AT_5);
        let path = rig.fresh_file();

        let start = rig.m.recording_start_for(AT_4).expect("registered");
        start.record_to(&path).expect("the recording runs");

        let e = refused(rig.m.begin_flash(AT_5, &before));
        assert_eq!(consent_sessions(&e), named(&[(AT_4, true)]));
        assert_eq!(rig.m.running_ids(), vec![AT_4.to_owned()]);
        rig.assert_no_update(&[AT_4, AT_5]);

        let holds = rig.m.flash_holds(AT_5);
        assert!(holds[0].recording, "{holds:?}");
        rig.m
            .begin_flash(AT_5, &holds)
            .expect("the user agreed to end this recording");
    }

    #[test]
    fn an_update_can_proceed_after_a_recording_start_failed() {
        let mut rig = Rig::new(vec![entry(4, None), entry(5, None)]);
        let _rx = rig.stream(AT_4);
        let consent = rig.m.flash_holds(AT_5);

        let start = rig.m.recording_start_for(AT_4).expect("registered");
        let folder = std::env::temp_dir();
        let failed = start.record_to(&folder);

        assert!(failed.is_err(), "a folder takes no recording");
        assert_eq!(rig.m.flash_holds(AT_5), consent);
        rig.m
            .begin_flash(AT_5, &consent)
            .expect("no recording started");
    }

    #[test]
    fn an_update_can_proceed_after_a_recording_start_was_given_up_unsent() {
        let mut rig = Rig::new(vec![entry(4, None), entry(5, None)]);
        let _rx = rig.stream(AT_4);
        let consent = rig.m.flash_holds(AT_5);

        let start = rig.m.recording_start_for(AT_4).expect("registered");
        drop(start);

        rig.m
            .begin_flash(AT_5, &consent)
            .expect("no recording was asked for");
    }

    #[test]
    fn a_recording_start_the_worker_has_not_applied_stays_registered_past_its_timeout() {
        let mut rig = Rig::new(vec![entry(4, None), entry(5, None)]);
        let _rx = rig.stream(AT_4);
        let consent = rig.m.flash_holds(AT_5);
        let path = rig.fresh_file();
        rig.stall(AT_4);

        let start = rig.m.recording_start_for(AT_4).expect("registered");
        let late = start.record_to(&path);
        assert_eq!(late.map_err(|e| e.kind), Err(ErrorKind::Timeout));

        let e = refused(rig.m.begin_flash(AT_5, &consent));
        assert_refused_while_recording_starts(&e, "asked for and not applied yet");
        assert_eq!(rig.m.running_ids(), vec![AT_4.to_owned()]);
        rig.assert_no_update(&[AT_4, AT_5]);

        rig.stall.end();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !rig.m.flash_holds(AT_5)[0].recording {
            assert!(std::time::Instant::now() < deadline, "it never ran");
            std::thread::sleep(Duration::from_millis(5));
        }
        let e = refused(rig.m.begin_flash(AT_5, &consent));
        assert_eq!(consent_sessions(&e), named(&[(AT_4, true)]));
    }

    #[test]
    fn a_recording_start_on_a_meter_outside_the_update_does_not_hold_it_back() {
        let mut rig = Rig::new(vec![entry(4, Some("SERIAL-A")), entry(5, Some("SERIAL-B"))]);
        let _rx = rig.stream(AT_4);

        let start = rig.m.recording_start_for(AT_4).expect("registered");
        let lease = rig.m.begin_flash(AT_5, &[]).expect("another meter");

        assert_eq!(lease.device().id, AT_5);
        drop(start);
    }

    // ---- AC-AXHK2F.6: each order of the race ----

    #[test]
    fn the_worker_gone_after_the_consent_still_starts_cleanly() {
        let mut rig = Rig::new(vec![entry(4, None), entry(5, None)]);
        let rx = rig.stream(AT_4);
        let consent = rig.m.flash_holds(AT_5);

        rig.unplug(AT_4);
        wait_ended(&rig, &rx, AT_4);
        let lease = rig.m.begin_flash(AT_5, &consent).expect("nothing left");
        assert!(rig.m.connected_ids().is_empty());

        drop(lease);
        rig.assert_no_update(&[AT_4, AT_5]);
    }

    #[test]
    fn a_disconnect_after_the_consent_still_starts_cleanly() {
        let mut rig = Rig::new(vec![entry(4, None), entry(5, None)]);
        let _rx = rig.stream(AT_4);
        let consent = rig.m.flash_holds(AT_5);

        rig.m.disconnect_for(AT_4).expect("connected");
        let lease = rig.m.begin_flash(AT_5, &consent).expect("nothing left");

        drop(lease);
        rig.assert_no_update(&[AT_4, AT_5]);
        let _again = rig.stream(AT_4);
    }

    #[test]
    fn a_detached_page_ending_the_session_after_the_consent_still_starts_cleanly() {
        let mut rig = Rig::new(vec![entry(4, None), entry(5, None)]);
        let rx = rig.stream(AT_4);
        let consent = rig.m.flash_holds(AT_5);

        let attacher = rig.m.attacher_for(AT_4).expect("running");
        attacher.stop().store(true, Ordering::SeqCst);
        wait_ended(&rig, &rx, AT_4);
        let lease = rig.m.begin_flash(AT_5, &consent).expect("nothing left");

        drop(lease);
        rig.assert_no_update(&[AT_4, AT_5]);
    }

    #[test]
    fn an_ended_session_needs_no_consent_and_its_wait_still_ends() {
        let mut rig = Rig::new(vec![entry(4, Some("SERIAL-A"))]);
        let rx = rig.stream(AT_4);
        rig.unplug(AT_4);
        wait_for(&rx, |e| {
            matches!(e, BackendEvent::Status(Status::Waiting { .. }))
        });
        assert_eq!(rig.m.waiting_for().as_deref(), Some("SERIAL-A"));

        assert_eq!(rig.m.flash_holds(AT_4), Vec::new());
        let _lease = rig.m.begin_flash(AT_4, &[]).expect("nothing to release");

        assert_eq!(rig.m.waiting_for(), None, "the wait ended with the update");
    }

    #[test]
    fn a_start_is_refused_while_a_session_that_may_be_the_target_finishes_closing() {
        for (what, session, target, target_id) in [
            ("the target's own", Some("SERIAL-A"), None, AT_4),
            (
                "the same serial elsewhere",
                Some("SERIAL-A"),
                Some("SERIAL-A"),
                AT_5,
            ),
            ("no serial on the session", None, Some("SERIAL-T"), AT_5),
            ("no serial on the target", Some("SERIAL-S"), None, AT_5),
            ("no serial on either side", None, None, AT_5),
        ] {
            let entries = if target_id == AT_4 {
                vec![entry(4, session)]
            } else {
                vec![entry(4, session), entry(5, target)]
            };
            let mut rig = Rig::new(entries);
            let _rx = rig.stream(AT_4);
            let consent = rig.m.flash_holds(target_id);

            let closing = rig.take_to_close(AT_4);
            let e = refused(rig.m.begin_flash(target_id, &consent));

            assert_refused_while_closing(&e, what);
            rig.assert_no_update(&[AT_4, AT_5]);

            closing.finish();
            let lease = rig
                .m
                .begin_flash(target_id, &[])
                .unwrap_or_else(|e| panic!("{what}: closed, yet refused: {e:?}"));
            drop(lease);
            rig.assert_no_update(&[AT_4, AT_5]);
        }
    }

    #[test]
    fn a_refused_start_during_a_close_releases_no_other_session() {
        let mut rig = Rig::new(vec![entry(4, None), entry(5, None), entry(6, None)]);
        let _closing = rig.stream(AT_4);
        let _other = rig.stream(AT_6);
        rig.record(AT_6);
        let consent = rig.m.flash_holds(AT_5);
        assert_eq!(consent.len(), 2, "{consent:?}");

        let closing = rig.take_to_close(AT_4);
        let e = refused(rig.m.begin_flash(AT_5, &consent));

        assert_refused_while_closing(&e, "another session in scope");
        assert_eq!(rig.m.running_ids(), vec![AT_6.to_owned()]);
        rig.assert_no_update(&[AT_4, AT_5, AT_6]);
        let sent = rig.sent.lock().unwrap().clone();
        assert!(
            sent.iter().all(|ty| SAFE_TYPES.contains(ty)),
            "an unsafe frame left the host: {sent:02X?}"
        );
        rig.m
            .stop_recording_for(AT_6)
            .expect("the recording kept running");

        closing.finish();
        let holds = rig.m.flash_holds(AT_5);
        rig.m
            .begin_flash(AT_5, &holds)
            .expect("the close is over and the rest agreed");
    }

    #[test]
    fn a_start_waits_for_the_trigger_and_the_session_of_a_close() {
        let mut rig = Rig::new(vec![entry(4, None), entry(5, None)]);
        let _rx = rig.stream(AT_4);
        rig.open_trigger(AT_4);
        let consent = rig.m.flash_holds(AT_5);

        let mut closing = rig.take_to_close(AT_4);
        assert!(closing.trigger.is_some(), "the session had a trigger open");
        let e = refused(rig.m.begin_flash(AT_5, &consent));
        assert_refused_while_closing(&e, "the trigger is still open");
        rig.assert_no_update(&[AT_4, AT_5]);

        closing.close_trigger();
        let e = refused(rig.m.begin_flash(AT_5, &consent));
        assert_refused_while_closing(&e, "the worker is still stopping");
        rig.assert_no_update(&[AT_4, AT_5]);

        closing.finish();
        rig.m.begin_flash(AT_5, &[]).expect("all closed");
    }

    #[test]
    fn a_start_is_refused_until_the_worker_a_replug_retired_has_finished() {
        let mut rig = Rig::new(vec![entry(4, Some("SERIAL-A")), entry(5, Some("SERIAL-A"))]);
        let rx = rig.stream(AT_4);
        rig.unplug(AT_4);
        wait_for(&rx, |e| {
            matches!(e, BackendEvent::Status(Status::Waiting { .. }))
        });
        let thread_ends = rig.linger_after_the_end(AT_4);

        let back: Vec<DeviceListing> = rig
            .m
            .list_devices()
            .into_iter()
            .filter(|d| d.id == AT_5)
            .collect();
        assert!(rig.m.devices_changed(&back), "the replug reconnects");
        let consent = rig.m.flash_holds(AT_5);
        assert_eq!(consent.len(), 1, "the new session: {consent:?}");

        let e = refused(rig.m.begin_flash(AT_5, &consent));

        assert_refused_while_closing(&e, "the retired worker still runs");
        assert_eq!(rig.m.running_ids(), vec![AT_5.to_owned()]);
        rig.assert_no_update(&[AT_4, AT_5]);

        drop(thread_ends);
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let lease = loop {
            match rig.m.begin_flash(AT_5, &consent) {
                Ok(lease) => break lease,
                Err(e) => {
                    assert_refused_while_closing(&e, "the retired worker is finishing");
                    assert!(std::time::Instant::now() < deadline, "still refused: {e:?}");
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        };
        assert_eq!(lease.device().id, AT_5);
    }

    #[test]
    fn a_connect_over_a_closing_trigger_leaves_the_start_refused_until_it_has_closed() {
        let mut rig = Rig::new(vec![entry(4, Some("SERIAL-A"))]);
        let _first = rig.stream(AT_4);
        rig.open_trigger(AT_4);

        let mut closing = rig.take_to_close(AT_4);
        assert!(closing.trigger.is_some(), "the session had a trigger open");
        closing.close_session();
        let _second = rig.stream(AT_4);
        let consent = rig.m.flash_holds(AT_4);
        assert_eq!(consent.len(), 1, "the new session: {consent:?}");

        let e = refused(rig.m.begin_flash(AT_4, &consent));

        assert_refused_while_closing(&e, "the replaced entry's trigger is still open");
        assert_eq!(rig.m.running_ids(), vec![AT_4.to_owned()]);
        rig.assert_no_update(&[AT_4]);

        closing.close_trigger();
        rig.m
            .begin_flash(AT_4, &consent)
            .expect("the trigger has closed");
    }

    #[test]
    fn a_meter_outside_the_update_closing_does_not_hold_the_start_back() {
        let mut rig = Rig::new(vec![entry(4, Some("SERIAL-A")), entry(5, Some("SERIAL-B"))]);
        let _rx = rig.stream(AT_4);
        rig.open_trigger(AT_4);

        let closing = rig.take_to_close(AT_4);
        let lease = rig
            .m
            .begin_flash(AT_5, &[])
            .expect("that meter is not the target");

        assert_eq!(lease.device().id, AT_5);
        assert!(rig.m.flashing());
        closing.finish();
    }

    #[test]
    fn the_lease_dropped_before_the_disconnect_leaves_a_known_state() {
        let mut rig = Rig::new(vec![entry(4, None), entry(5, None)]);
        let rx = rig.stream(AT_4);
        let holds = rig.m.flash_holds(AT_5);
        let lease = rig.m.begin_flash(AT_5, &holds).expect("agreed");
        wait_ended(&rig, &rx, AT_4);

        drop(lease);
        let late = rig.m.disconnect_for(AT_4);

        assert_eq!(late.map_err(|e| e.kind), Err(ErrorKind::Usage));
        assert!(rig.m.connected_ids().is_empty());
        rig.assert_no_update(&[AT_4, AT_5]);
        let _again = rig.stream(AT_4);
    }

    #[test]
    fn the_disconnect_before_the_lease_drops_leaves_a_known_state() {
        let mut rig = Rig::new(vec![entry(4, None), entry(5, None)]);
        let rx = rig.stream(AT_4);
        let holds = rig.m.flash_holds(AT_5);
        let lease = rig.m.begin_flash(AT_5, &holds).expect("agreed");
        wait_ended(&rig, &rx, AT_4);

        let late = rig.m.disconnect_for(AT_4);
        assert_eq!(late.map_err(|e| e.kind), Err(ErrorKind::Usage));
        assert!(rig.m.flashing(), "the update still holds its meters");

        drop(lease);
        rig.assert_no_update(&[AT_4, AT_5]);
        let _again = rig.stream(AT_4);
    }
}
