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

use powerz_protocol::decode::Rate;
use powerz_protocol::session::Clock;
use powerz_protocol::transport::bulk::BulkTransport;
use powerz_protocol::transport::discovery::{self, DeviceEntry, Model};
use powerz_protocol::transport::guard::GuardedTransport;
use powerz_protocol::transport::{self, Transport};
use powerz_sim::{Persona, Scenario, SimDevice};
use serde::Serialize;

use crate::history::{HistoryGap, Series, SeriesBuckets};
use crate::recording::{ExportProgress, RecordingSummary};
use crate::stats::StatsSnapshot;
use crate::store::SessionStore;
use crate::ui_error::UiError;
use crate::worker::{
    self, BackendEvent, ConnectedInfo, EventSink, Gap, GapKind, Status, WorkerConfig, WorkerHandle,
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
    let label = match &entry.serial {
        Some(serial) => format!("{model} — {serial}"),
        None => format!("{model} — bus {} address {}", entry.bus, entry.address),
    };
    DeviceListing {
        id: format!("usb:{}:{}", entry.bus, entry.address),
        label,
        model,
        serial: entry.serial.clone(),
        bus: Some(entry.bus.clone()),
        address: Some(entry.address),
        demo_scenario: None,
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
}

type Discover = Arc<dyn Fn() -> Result<Vec<DeviceEntry>, transport::Error> + Send + Sync>;

/// The pipe the app opens: the demo device or a real meter over bulk.
#[derive(Debug)]
pub enum AppTransport {
    /// The simulated KM003C.
    Demo(Box<SimDevice>),
    /// A real meter on its vendor bulk interface.
    Usb(GuardedTransport<BulkTransport>),
}

impl Transport for AppTransport {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, transport::Error> {
        match self {
            AppTransport::Demo(t) => t.transact(request, timeout),
            AppTransport::Usb(t) => t.transact(request, timeout),
        }
    }

    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, transport::Error> {
        match self {
            AppTransport::Demo(t) => t.read(timeout),
            AppTransport::Usb(t) => t.read(timeout),
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
        if let Some(name) = &device.demo_scenario {
            let scenario = Scenario::from_name(name).ok_or(transport::Error::NotFound)?;
            return Ok(AppTransport::Demo(Box::new(SimDevice::new(
                Persona::Bulk,
                scenario,
            ))));
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

/// Owns every meter connection.
pub struct DeviceManager<O, C> {
    discover: Discover,
    opener: Arc<O>,
    clock: C,
    worker: Option<WorkerHandle>,
    next_session_id: u64,
    /// Session per meter serial (or device id when it has none): what the
    /// outage gap needs, and the data every connection appends to.
    histories: HashMap<String, (Arc<Mutex<History>>, SessionStore)>,
    /// The data of the most recent session, kept readable after it ends.
    last_store: Option<SessionStore>,
    /// The open session's page: where its events go now, and what a new
    /// page needs to pick it up.
    live: Option<Arc<Mutex<Live>>>,
    /// The open session's stop flag.
    stop: Option<Arc<AtomicBool>>,
}

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
            worker: None,
            next_session_id: 1,
            histories: HashMap::new(),
            last_store: None,
            live: None,
            stop: None,
        }
    }

    /// Every attached meter plus the demo entries; never opens a device.
    pub fn list_devices(&self) -> Vec<DeviceListing> {
        list_devices_from((self.discover)())
    }

    /// Connect to the device with id `id` at `rate_sps` on a dedicated worker
    /// thread; events go to `sink`. Refused while a session is open.
    pub fn connect(
        &mut self,
        id: &str,
        rate_sps: u32,
        sink: impl EventSink,
    ) -> Result<(), UiError> {
        self.connect_with_stop(id, rate_sps, sink, Arc::new(AtomicBool::new(false)))
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
        if self.running_worker().is_some() {
            return Err(UiError::usage(
                "A meter is already connected. Disconnect it first.",
            ));
        }
        let rate = supported_rate(rate_sps)?;
        let device = self
            .list_devices()
            .into_iter()
            .find(|d| d.id == id)
            .ok_or_else(|| UiError::usage("That meter is no longer attached."))?;
        if let Some(old) = self.worker.take() {
            old.disconnect();
        }
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
        let (history, store) = (Arc::clone(history), store.clone());
        self.last_store = Some(store.clone());
        let session_id = history
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .session_id;
        let opener = Arc::clone(&self.opener);
        let config = WorkerConfig { rate, session_id };
        let live = Arc::new(Mutex::new(Live {
            device_id: device.id.clone(),
            info: None,
            sink: Box::new(sink),
        }));
        self.live = Some(Arc::clone(&live));
        self.stop = Some(Arc::clone(&stop));
        self.worker = Some(worker::spawn_with_store(
            move || opener.open(&device),
            self.clock.clone(),
            config,
            store,
            history_sink(history, live_sink(live)),
            stop,
        ));
        Ok(())
    }

    /// Restart the open session's queue at `rate_sps`.
    pub fn set_rate(&mut self, rate_sps: u32) -> Result<(), UiError> {
        let (control, rate) = self.rate_control(rate_sps)?;
        control.set_rate(rate)
    }

    /// What [`DeviceManager::set_rate`] needs, so a caller can wait for the
    /// worker's answer after releasing its lock on the manager.
    pub fn rate_control(&self, rate_sps: u32) -> Result<(worker::WorkerControl, Rate), UiError> {
        let rate = supported_rate(rate_sps)?;
        Ok((self.control()?, rate))
    }

    /// Recording and export control of the open session, so a caller can
    /// wait for the worker's answer after releasing its lock on the manager.
    pub fn control(&self) -> Result<worker::WorkerControl, UiError> {
        self.running_worker()
            .map(WorkerHandle::control)
            .ok_or_else(|| UiError::usage("No meter is connected."))
    }

    /// The open session's worker, unless it has ended by itself.
    fn running_worker(&self) -> Option<&WorkerHandle> {
        self.worker.as_ref().filter(|w| !w.is_finished())
    }

    /// Record the open session's stream to a new file at `path`.
    pub fn start_recording(&self, path: &Path) -> Result<(), UiError> {
        self.control()?.record_to(path)
    }

    /// Stop the recording and return what it wrote.
    pub fn stop_recording(&self) -> Result<RecordingSummary, UiError> {
        self.control()?.stop_recording()
    }

    /// The data of the open session, or of the last one once the meter is
    /// gone (unplugging keeps the data). Refused only before any connect.
    pub fn session_store(&self) -> Result<SessionStore, UiError> {
        self.last_store
            .clone()
            .ok_or_else(|| UiError::usage("No meter is connected."))
    }

    /// What [`DeviceManager::export_session`] needs, so a caller can wait
    /// for the worker's answer after releasing its lock on the manager.
    pub fn exporter(&self) -> Result<worker::Exporter, UiError> {
        let store = self.session_store()?;
        let control = self.control().ok();
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
        self.worker
            .take()
            .ok_or_else(|| UiError::usage("No meter is connected."))
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
        let mut clock = self.clock.clone();
        let thread = std::thread::spawn(move || {
            let mut hotplug = Hotplug::new();
            while !flag.load(Ordering::SeqCst) {
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
        let worker = self.running_worker()?;
        Some(Attacher {
            control: worker.control(),
            live: Arc::clone(self.live.as_ref()?),
            stop: self.stop.clone().unwrap_or_default(),
        })
    }

    /// Stop the queue and close the open session.
    pub fn disconnect(&mut self) -> Result<(), UiError> {
        self.take_session()?.disconnect();
        Ok(())
    }
}

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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::batch::{Batch, Sample};

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
