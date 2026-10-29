//! Session worker: one thread per connected meter, streaming backend events
//! to the UI (FR VHY378).

use std::io::Write;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use powerz_protocol::adcnew::Content;
use powerz_protocol::decode::pd_packet::CaptureClock;
use powerz_protocol::decode::{QueueRecord, Rate};
use powerz_protocol::firmware::DeviceMode;
use powerz_protocol::pdcap;
use powerz_protocol::session::{
    self, Clock, PdOptions, PdSniffer, Session, SnapshotCadence, Streamer,
};
use powerz_protocol::transport::{self, Transport, guard::GuardedTransport};
use serde::{Deserialize, Serialize};

use crate::batch::{Batch, Batcher, Sample};
use crate::history::{Series, SeriesBuckets};
use crate::pd_capture::{PdCapture, PdDetails, PdRow, PdStatusSample};
use crate::recording::{self, ExportProgress, Recorder, RecordingEvent, RecordingSummary};
use crate::stats::StatsSnapshot;
use crate::store::{SessionStore, temp_at};
use crate::ui_error::{self, ErrorKind, UiError};

/// Where the live measurements come from (FR 1HHT02), named on the wire
/// `queue` / `cdc`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MeasurementSource {
    /// The ADC queue polled over the vendor bulk pipe (CONNECT, auth).
    #[default]
    Queue,
    /// The ADCNEW push stream on the CDC serial port: no CONNECT and no
    /// sign-in; it carries no device info and no temperature.
    Cdc,
}

/// Worker start-up settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkerConfig {
    /// Requested queue rate.
    pub rate: Rate,
    /// Session history this connection appends to.
    pub session_id: u64,
    /// Device mode the `connected` status reports, from the listing.
    pub mode: DeviceMode,
    /// Where the measurements come from; the queue by default.
    pub source: MeasurementSource,
}

impl WorkerConfig {
    /// Settings for a meter in application mode, polling the queue.
    pub fn new(rate: Rate, session_id: u64) -> Self {
        Self {
            rate,
            session_id,
            mode: DeviceMode::Application,
            source: MeasurementSource::Queue,
        }
    }
}

/// What the `connected` status reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConnectedInfo {
    /// Model name, e.g. `KM003C`.
    pub model: String,
    /// Firmware version, e.g. `2.0.6`.
    pub firmware_version: String,
    /// Device mode name, from the listing (`unknown` when the interface
    /// string did not say).
    pub mode: &'static str,
    /// Bootloader version, e.g. `2.5`; empty when the block is blank.
    pub bootloader_version: String,
    /// Firmware warning codes (`too_old`, `outdated`, `update_required`).
    pub warnings: Vec<&'static str>,
    /// The HEAD reply asked for a firmware update; `None` (JSON `null`)
    /// when HEAD could not be read.
    pub update_required: Option<bool>,
    /// What could not be read while getting the firmware status, one line
    /// per failed step (an info block or HEAD).
    pub firmware_problems: Vec<String>,
    /// Serial number.
    pub serial: String,
    /// Queue rate in samples per second, when known.
    pub rate_sps: Option<u32>,
    /// The rate cannot be changed (authentication failed).
    pub rate_locked: bool,
    /// Session history id.
    pub session_id: u64,
}

/// Connection status changes.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Status {
    /// The session is up.
    Connected(ConnectedInfo),
    /// The measured queue rate.
    Rate {
        /// Samples per second.
        rate_sps: u32,
        /// The rate cannot be changed.
        rate_locked: bool,
    },
    /// The session ended.
    Disconnected {
        /// Why, when it was not asked for.
        error: Option<UiError>,
    },
    /// The meter went away without being asked to; the app waits for the
    /// meter with this serial to come back (FR TGE839).
    Waiting {
        /// Serial of the meter the app waits for.
        serial: String,
    },
}

/// Why samples are missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GapKind {
    /// Records the meter's queue dropped.
    Stream,
    /// The meter was unplugged between two connections.
    Outage,
}

/// A span of session time without samples.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Gap {
    /// Kind of gap.
    pub kind: GapKind,
    /// Last session time before the gap, ms.
    pub from_ms: u64,
    /// First session time after the gap, ms.
    pub to_ms: u64,
}

/// The PD capture started or stopped (FR TGE83C).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PdState {
    /// The PD sniffer runs.
    pub capturing: bool,
}

/// A batch of PD table rows (FR TGE83C).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PdRows {
    /// Rows in capture order.
    pub rows: Vec<PdRow>,
}

/// A batch of PD status blocks for the chart (FR TGE83D).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PdStatusBatch {
    /// Status blocks in time order.
    pub samples: Vec<PdStatusSample>,
}

/// The meter's settings as the panel shows them, in display order (FR
/// 1HHT01); each row serializes as `{key, label, value, tag}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MeterSettingsView {
    /// Every decoded field.
    pub fields: Vec<powerz_protocol::decode::settings::Row>,
}

impl From<&powerz_protocol::decode::settings::MeterSettings> for MeterSettingsView {
    fn from(settings: &powerz_protocol::decode::settings::MeterSettings) -> Self {
        Self {
            fields: settings.rows(),
        }
    }
}

/// One offline record as the Records view lists it (FR 1HHT00).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RecordEntryView {
    /// 1-based position in the meter's list.
    pub index: usize,
    /// The record's name.
    pub name: String,
    /// Seconds between samples.
    pub interval_s: u8,
    /// Number of samples.
    pub points: u16,
    /// Recorded time in seconds.
    pub duration_s: u32,
    /// Charge in Ah as stored: signed, negative for one current direction
    /// (the Records view shows the magnitude, as the meter does).
    pub charge_ah: f64,
    /// Energy in Wh as stored: signed like the charge.
    pub energy_wh: f64,
    /// Offset of the samples in the record area.
    pub offset: u32,
}

impl RecordEntryView {
    fn new(index: usize, e: &powerz_protocol::decode::records::RecordEntry) -> Self {
        Self {
            index,
            name: e.name.clone(),
            interval_s: e.interval_s,
            points: e.points,
            duration_s: e.duration_s,
            charge_ah: e.charge_ah(),
            energy_wh: e.energy_wh(),
            offset: e.offset,
        }
    }
}

/// The meter's offline-record list (FR 1HHT00).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RecordListView {
    /// Records in the meter's order.
    pub records: Vec<RecordEntryView>,
}

/// The result of an offline-record delete (FR JKWT3V): the list as read
/// again and where the backup went.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RecordDeleteView {
    /// The meter's records after the delete.
    pub records: Vec<RecordEntryView>,
    /// Folder holding the CSV backup.
    pub backup_dir: String,
    /// The backup CSV files, one per deleted record.
    pub backups: Vec<String>,
}

/// One sample of a downloaded offline record.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RecordSampleView {
    /// Seconds from the record start.
    pub t_s: f64,
    /// VBUS in V.
    pub vbus_v: f64,
    /// IBUS in A.
    pub ibus_a: f64,
    /// Charge so far in Ah.
    pub charge_ah: f64,
    /// Energy so far in Wh.
    pub energy_wh: f64,
}

impl RecordSampleView {
    fn new(
        entry: &powerz_protocol::decode::records::RecordEntry,
        index: usize,
        s: &powerz_protocol::decode::records::RecordSample,
    ) -> Self {
        Self {
            t_s: entry.sample_time_s(index) as f64,
            vbus_v: s.vbus_v(),
            ibus_a: s.ibus_a(),
            charge_ah: s.charge_ah(),
            energy_wh: s.energy_wh(),
        }
    }
}

/// A downloaded offline record with its samples (FR 1HHT00).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OfflineRecordView {
    /// 1-based position in the meter's list.
    pub index: usize,
    /// The record's name.
    pub name: String,
    /// Seconds between samples.
    pub interval_s: u8,
    /// Samples in time order.
    pub samples: Vec<RecordSampleView>,
}

/// Every event the backend sends to the UI.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum BackendEvent {
    /// A batch of samples.
    Batch(Batch),
    /// Missing samples.
    Gap(Gap),
    /// Connection status.
    Status(Status),
    /// An error.
    Error(UiError),
    /// Recording state.
    Recording(RecordingEvent),
    /// PD capture state.
    PdState(PdState),
    /// PD table rows.
    PdRows(PdRows),
    /// PD status blocks.
    PdStatus(PdStatusBatch),
}

/// Where the worker sends its events.
pub trait EventSink: FnMut(BackendEvent) + Send + 'static {}

impl<F: FnMut(BackendEvent) + Send + 'static> EventSink for F {}

/// Result of one worker step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// The session goes on.
    Continue,
    /// The session ended.
    Ended,
}

type Sink = Box<dyn FnMut(BackendEvent) + Send>;

/// The wall time a worker spent blocked inside its own calls: the sink
/// hand-off and channel send, the session store's lock, meter I/O and
/// recording writes (FR KQF5GM). Clones share one total.
#[derive(Debug, Clone, Default)]
pub struct BlockedMeter(Arc<AtomicU64>);

impl BlockedMeter {
    /// A meter at zero.
    pub fn new() -> BlockedMeter {
        BlockedMeter::default()
    }

    /// Add `d` to the total.
    pub fn add(&self, d: Duration) {
        let nanos = u64::try_from(d.as_nanos()).unwrap_or(u64::MAX);
        self.0.fetch_add(nanos, Ordering::Relaxed);
    }

    /// The time added so far.
    pub fn total(&self) -> Duration {
        Duration::from_nanos(self.0.load(Ordering::Relaxed))
    }
}

/// Run `f`, adding its wall time to `meter` when there is one.
fn blocked<R>(meter: Option<&BlockedMeter>, f: impl FnOnce() -> R) -> R {
    match meter {
        Some(meter) => {
            let started = Instant::now();
            let r = f();
            meter.add(started.elapsed());
            r
        }
        None => f(),
    }
}

/// Runs a task in order with the events sent so far: at once on a worker
/// without a forwarder, on the forwarder thread behind every queued event
/// otherwise.
type InOrder = Box<dyn FnMut(Box<dyn FnOnce() + Send>) + Send>;

/// What the worker thread hands its sink forwarder (FR X2SPDW).
enum Forward {
    Event(BackendEvent),
    Run(Box<dyn FnOnce() + Send>),
}

/// A session error in plain language for the OS the app runs on.
fn session_error(err: &session::Error) -> UiError {
    ui_error::from_session(err, std::env::consts::OS)
}

/// The `disconnected` status, with `error` when the end was not asked for.
fn disconnected(error: Option<UiError>) -> BackendEvent {
    BackendEvent::Status(Status::Disconnected { error })
}

/// The `connected` status details from the meter's device info. A locked
/// rate is unknown until measured from record timestamps.
fn connected_info(
    info: &session::DeviceInfo,
    fw: &session::FirmwareStatus,
    config: &WorkerConfig,
    rate_locked: bool,
) -> ConnectedInfo {
    let WorkerConfig {
        rate,
        session_id,
        mode,
        ..
    } = *config;
    ConnectedInfo {
        model: info.model.clone(),
        firmware_version: info.firmware_version.clone(),
        mode: mode.name(),
        bootloader_version: fw
            .bootloader
            .as_ref()
            .map_or_else(String::new, |b| b.version().to_owned()),
        warnings: fw.warnings().into_iter().map(|w| w.code()).collect(),
        update_required: fw.update_required,
        firmware_problems: fw.problems.iter().map(|p| p.text.clone()).collect(),
        serial: info.serial.clone(),
        rate_sps: (!rate_locked).then(|| rate.sps()),
        rate_locked,
        session_id,
    }
}

/// The meter refused authentication or the queue start: stream at its own
/// rate instead of failing.
fn is_rate_refusal(err: &session::Error) -> bool {
    matches!(
        err,
        session::Error::AuthFailed | session::Error::Rejected { .. }
    )
}

/// Samples per second implied by the spacing of the record times `times`
/// (device-time order), when there are at least two of them.
fn measured_sps(times: &[u32]) -> Option<u32> {
    let (first, last) = (times.first()?, times.last()?);
    let n = u32::try_from(times.len()).ok()?.checked_sub(1)?;
    let span = last.checked_sub(*first)?;
    let interval = span.checked_div(n).filter(|&i| i > 0)?;
    Some((1000 + interval / 2) / interval)
}

fn sample(r: &QueueRecord) -> Sample {
    Sample {
        t_ms: r.time_ms(),
        vbus_v: r.vbus_v(),
        ibus_a: r.ibus_a(),
        cc1_v: r.cc1_v(),
        cc2_v: r.cc2_v(),
        dp_v: r.dp_v(),
        dm_v: r.dm_v(),
    }
}

/// The `connected` status of a CDC stream session: the stream carries no
/// device info, so model, firmware and serial are left blank for the
/// caller (which knows the listing) to fill in.
fn cdc_connected_info(config: &WorkerConfig) -> ConnectedInfo {
    ConnectedInfo {
        model: String::new(),
        firmware_version: String::new(),
        mode: config.mode.name(),
        bootloader_version: String::new(),
        warnings: Vec::new(),
        update_required: None,
        firmware_problems: Vec::new(),
        serial: String::new(),
        rate_sps: Some(config.rate.sps()),
        rate_locked: false,
        session_id: config.session_id,
    }
}

/// Open the session `config.source` asks for: the queue (see
/// [`open_queue_session`]) or the CDC ADCNEW stream, started at
/// `config.rate` with ADC content only and nothing sent before it (no
/// CONNECT, whose byte means something else there, and no sign-in).
fn open_session<T: Transport, C: Clock>(
    transport: T,
    clock: C,
    config: WorkerConfig,
) -> Result<(Session<GuardedTransport<T>, C>, ConnectedInfo), UiError> {
    match config.source {
        MeasurementSource::Queue => open_queue_session(transport, clock, config),
        MeasurementSource::Cdc => {
            let mut session = Session::with_clock(GuardedTransport::new(transport), clock);
            session
                .start_adcnew(Content::Adc, config.rate)
                .map_err(|e| session_error(&e))?;
            Ok((session, cdc_connected_info(&config)))
        }
    }
}

/// CONNECT, read the device info, authenticate at level 1 and start the
/// queue at `config.rate`. When the meter refuses the auth or the queue
/// start, the session goes on with the rate locked to the meter's own.
fn open_queue_session<T: Transport, C: Clock>(
    transport: T,
    clock: C,
    config: WorkerConfig,
) -> Result<(Session<GuardedTransport<T>, C>, ConnectedInfo), UiError> {
    let mut session = Session::with_clock(GuardedTransport::new(transport), clock);
    let mut run =
        || -> Result<(session::DeviceInfo, session::FirmwareStatus, bool), session::Error> {
            // CONNECT, then both info blocks and HEAD (read-only, FR V9SK52).
            session.connect()?;
            // A HEAD or info block that timed out or did not decode only
            // costs the firmware status; an unplugged or refused meter
            // still fails the connect.
            let fw = session.read_firmware_status_or_unknown()?;
            let info = session.device_info()?;
            let started = session.authenticate(1).and_then(|_| {
                match session.start_stream(config.rate) {
                    // A queue started by an earlier session still runs [device]:
                    // stop it and start again, once.
                    Err(session::Error::Rejected { .. }) => {
                        session.stop_stream()?;
                        session.start_stream(config.rate)
                    }
                    other => other,
                }
            });
            match started {
                Ok(()) => Ok((info, fw, false)),
                Err(e) if is_rate_refusal(&e) => Ok((info, fw, true)),
                Err(e) => Err(e),
            }
        };
    let (info, fw, locked) = run().map_err(|e| session_error(&e))?;
    let info = connected_info(&info, &fw, &config, locked);
    Ok((session, info))
}

/// A running session over transport `T` on clock `C`.
pub struct Worker<T: Transport, C: Clock> {
    session: Option<Session<GuardedTransport<T>, C>>,
    clock: C,
    /// Where the measurements come from.
    source: MeasurementSource,
    rate: Rate,
    /// The rate cannot be changed (auth or queue start refused).
    rate_locked: bool,
    /// The locked rate as last measured from record timestamps.
    measured_sps: Option<u32>,
    /// Device time of the last record seen, for measuring across polls.
    last_record_ms: Option<u32>,
    /// When the next ADC snapshot is due (wall clock).
    snapshots: SnapshotCadence,
    /// Latest chip temperature, °C; `None` when invalid or not read yet.
    temp_c: Option<f64>,
    /// Wall-clock time `temp_c` was read, ms.
    temp_read_ms: Option<u64>,
    /// Groups records into UI batches flushed at 20–60 Hz.
    batcher: Batcher,
    /// Wall-clock time the next queue poll is due, ms.
    next_poll_ms: u64,
    /// The session's history and statistics, shared with the other
    /// connections to this meter and with the commands that query them.
    store: SessionStore,
    /// The open recording, if any.
    recording: Option<Recorder>,
    /// The file the open recording writes, when it was opened by path.
    recording_path: Option<PathBuf>,
    sink: Sink,
    in_order: InOrder,
    /// Adds up the time blocked in the worker's own calls, when given.
    meter: Option<BlockedMeter>,
    /// The running PD sniffer, if any.
    pd_run: Option<PdRun>,
    /// The PD records captured so far and the rows waiting to go out.
    pd: PdCapture,
    /// Meter model and firmware version for a saved PD capture's header;
    /// never the serial number (FR TGE83E).
    pd_meter: (String, String),
    /// Holds on the queue polling (FR DGSQ6S): while any is held, no
    /// GET_DATA goes out, but the session stays connected.
    poll_holds: u32,
}

/// A running PD sniffer.
#[derive(Debug)]
struct PdRun {
    /// Turns record times into ms since the capture start.
    clock: CaptureClock,
    /// Wall-clock time the next PD_PACKET poll is due, ms.
    next_poll_ms: u64,
}

impl<T: Transport, C: Clock> std::fmt::Debug for Worker<T, C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Worker")
            .field("open", &self.session.is_some())
            .field("rate", &self.rate)
            .finish_non_exhaustive()
    }
}

impl<T: Transport, C: Clock + Clone> Worker<T, C> {
    /// Connect, authenticate and start the queue, with a session of its own.
    pub fn start(
        transport: T,
        clock: C,
        config: WorkerConfig,
        sink: impl EventSink,
    ) -> Result<Worker<T, C>, UiError> {
        Worker::start_with_store(transport, clock, config, SessionStore::new(), sink)
    }

    /// [`Worker::start`] appending to the session in `store`, as a
    /// reconnect of the same meter does.
    pub fn start_with_store(
        transport: T,
        clock: C,
        config: WorkerConfig,
        store: SessionStore,
        sink: impl EventSink,
    ) -> Result<Worker<T, C>, UiError> {
        let (session, info) = open_session(transport, clock.clone(), config)?;
        Ok(Worker::running(
            session,
            clock,
            config,
            info,
            store,
            Box::new(sink),
            Box::new(|task: Box<dyn FnOnce() + Send>| task()),
            None,
        ))
    }

    /// A worker over an open session; emits the `connected` status.
    #[allow(clippy::too_many_arguments)]
    fn running(
        session: Session<GuardedTransport<T>, C>,
        mut clock: C,
        config: WorkerConfig,
        info: ConnectedInfo,
        store: SessionStore,
        sink: Sink,
        in_order: InOrder,
        meter: Option<BlockedMeter>,
    ) -> Worker<T, C> {
        let now = clock.now_ms();
        blocked(meter.as_ref(), || store.start_run());
        let mut worker = Worker {
            session: Some(session),
            clock,
            source: config.source,
            rate: config.rate,
            rate_locked: info.rate_locked,
            measured_sps: None,
            last_record_ms: None,
            snapshots: SnapshotCadence::default(),
            temp_c: None,
            temp_read_ms: None,
            batcher: Batcher::new(now),
            next_poll_ms: now,
            store,
            recording: None,
            recording_path: None,
            sink,
            in_order,
            meter,
            pd_run: None,
            pd: PdCapture::new(),
            pd_meter: (info.model.clone(), info.firmware_version.clone()),
            poll_holds: 0,
        };
        worker.emit(BackendEvent::Status(Status::Connected(info)));
        worker
    }

    fn emit(&mut self, event: BackendEvent) {
        (self.sink)(event);
    }

    /// The temperature the live readout shows: the latest valid one while it
    /// is current, as recordings and exports count it (AC-CWWKJ6.4).
    fn live_temp(&mut self) -> Option<f64> {
        let now = i64::try_from(self.clock.now_ms()).unwrap_or(i64::MAX);
        let read = i64::try_from(self.temp_read_ms?).unwrap_or(i64::MAX);
        self.temp_c
            .filter(|_| now <= SnapshotCadence::expiry_ms(read))
    }

    /// Read one ADC snapshot when the last one is due, after a queue poll
    /// so requests stay strictly one at a time.
    fn snapshot_if_due(&mut self) -> Result<(), session::Error> {
        let Some(session) = self.session.as_mut() else {
            return Ok(());
        };
        // The CDC stream is read without requests in between; it has no
        // temperature to add.
        if self.source == MeasurementSource::Cdc || !self.snapshots.take(self.clock.now_ms()) {
            return Ok(());
        }
        let adc = blocked(self.meter.as_ref(), || session.read_adc())?;
        self.temp_c = adc.temp_c();
        if let Some(temp) = self.temp_c {
            let now = self.clock.now_ms();
            self.temp_read_ms = Some(now);
            let store = &self.store;
            blocked(self.meter.as_ref(), || store.set_temp(temp, now));
        }
        Ok(())
    }

    /// Send `batch` with the statistics up to and including it.
    fn emit_batch(&mut self, mut batch: Batch) {
        let store = &self.store;
        batch.stats = blocked(self.meter.as_ref(), || store.stats());
        self.emit(BackendEvent::Batch(batch));
    }

    /// Send pending samples out now.
    fn flush_batch(&mut self) {
        let now = self.clock.now_ms();
        if let Some(batch) = self.batcher.flush(now) {
            self.emit_batch(batch);
        }
    }

    /// Hand the records of `poll` to the batcher; a stream gap flushes the
    /// samples before it and goes out as its own event.
    /// Every record is moved onto the session timeline (device time can go
    /// backwards), then goes to the session history, the statistics, the
    /// batcher and the open recording; a failed write ends the recording.
    fn queue_records(&mut self, poll: &session::StreamPoll) {
        let now = self.clock.now_ms();
        let store = self.store.clone();
        let meter = self.meter.clone();
        // Held across the records, let go whenever an event goes out.
        let mut data = None;
        let mut gaps = poll.gaps.iter().peekable();
        let mut write_err = None;
        for r in &poll.records {
            // A poll's records and gaps come in device-time order; a gap
            // ends at the record after the hole.
            let mut gaps_due = 0;
            while gaps.next_if(|g| g.to_ms <= r.time_ms()).is_some() {
                gaps_due += 1;
            }
            if gaps_due > 0 {
                // The samples before the gap go out with their statistics.
                data = None;
                self.flush_batch();
            }
            let added = data
                .get_or_insert_with(|| blocked(meter.as_ref(), || store.lock()))
                .append(r, now);
            let r = added.record;
            if gaps_due > 0 {
                data = None;
                let to_ms = u64::from(r.time_ms());
                let gap = Gap {
                    kind: GapKind::Stream,
                    from_ms: added.before_ms.map_or(to_ms, u64::from),
                    to_ms,
                };
                for _ in 0..gaps_due {
                    if let Some(rec) = self.recording.as_mut() {
                        rec.gap(gap.clone());
                    }
                    self.emit(BackendEvent::Gap(gap.clone()));
                }
            }
            self.batcher.push(sample(&r));
            if write_err.is_none()
                && let Some(rec) = self.recording.as_mut()
            {
                if added.new_run {
                    rec.break_run();
                }
                if let Err(e) =
                    blocked(meter.as_ref(), || rec.push(&r, added.unix_ms, added.temp_c))
                {
                    write_err = Some(e);
                }
            }
        }
        drop(data);
        if let Some(e) = write_err {
            self.fail_recording(recording::io_error(&e));
        }
    }

    /// End the open recording after a write error, keeping its rows.
    fn fail_recording(&mut self, error: UiError) {
        if let Some(rec) = self.recording.take() {
            // Rows still waiting in the buffer never reached the file.
            let rows = rec.kept_rows();
            // Closing the file is all that is left to do; the error is the news.
            drop(rec);
            self.emit(BackendEvent::Recording(RecordingEvent::Failed {
                rows,
                error,
            }));
        }
    }

    /// Flush the open recording and report its progress when due.
    fn tick_recording(&mut self) {
        let now = self.clock.now_ms();
        let Some(rec) = self.recording.as_mut() else {
            return;
        };
        match blocked(self.meter.as_ref(), || rec.tick(now)) {
            Ok(Some(event)) => self.emit(BackendEvent::Recording(event)),
            Ok(None) => {}
            Err(e) => self.fail_recording(recording::io_error(&e)),
        }
    }

    /// Close the open recording because the session ended; `unplugged` when
    /// the meter went away rather than being disconnected on request.
    fn end_recording(&mut self, unplugged: bool) {
        if let Some(rec) = self.recording.take() {
            let event = match rec.finish(unplugged) {
                Ok(summary) => RecordingEvent::Stopped(summary),
                Err((rows, error)) => RecordingEvent::Failed { rows, error },
            };
            self.emit(BackendEvent::Recording(event));
        }
    }

    /// Rows written and elapsed ms of the open recording, if any.
    pub fn recording_progress(&mut self) -> Option<(u64, u64)> {
        let now = self.clock.now_ms();
        self.recording.as_ref().map(|rec| rec.progress(now))
    }

    /// The stream is being recorded.
    pub fn is_recording(&self) -> bool {
        self.recording.is_some()
    }

    /// Start recording every following record to `out`: the header is
    /// written and flushed now, rows as they arrive.
    pub fn start_recording(&mut self, out: Box<dyn Write + Send>) -> Result<(), UiError> {
        if self.session.is_none() {
            return Err(not_connected());
        }
        if self.recording.is_some() {
            return Err(UiError::usage("A recording is already running."));
        }
        let now = self.clock.now_ms();
        let rec = Recorder::start(out, now).map_err(|e| recording::io_error(&e))?;
        self.recording = Some(rec);
        self.emit(BackendEvent::Recording(RecordingEvent::Started));
        Ok(())
    }

    /// Create the file at `path` and record to it. The checks run first, so a
    /// refused start never touches the path; a file this call created is
    /// removed again if the header cannot be written.
    pub fn record_to(&mut self, path: &Path) -> Result<(), UiError> {
        if self.session.is_none() {
            return Err(not_connected());
        }
        if self.recording.is_some() {
            return Err(UiError::usage("A recording is already running."));
        }
        let existed = path.exists();
        let file = recording::create_file(path)?;
        match self.start_recording(Box::new(file)) {
            Ok(()) => {
                // Canonical, so the export guard sees through symlinks.
                self.recording_path =
                    Some(std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned()));
                Ok(())
            }
            Err(e) => {
                if !existed {
                    let _ = std::fs::remove_file(path);
                }
                Err(e)
            }
        }
    }

    /// Create the file at `path` and export the session to it, unless it is
    /// the file being recorded to.
    pub fn export_to(&mut self, path: &Path, progress: ProgressFn) -> Result<ExportJob, UiError> {
        let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
        if self.recording.is_some() && self.recording_path.as_deref() == Some(target.as_path()) {
            return Err(UiError::usage(
                "That file is being recorded to; pick another name for the export.",
            ));
        }
        let file = recording::create_file(path)?;
        Ok(self.export(Box::new(file), progress))
    }

    /// Stop the recording: flush and close the file.
    pub fn stop_recording(&mut self) -> Result<RecordingSummary, UiError> {
        let rec = self
            .recording
            .take()
            .ok_or_else(|| UiError::usage("Nothing is being recorded."))?;
        rec.finish(false).map_err(|(_, error)| error)
    }

    /// Export every record of this session to `out` on a thread of its own,
    /// while the stream goes on; `progress` is called on that thread.
    pub fn export(
        &mut self,
        out: Box<dyn Write + Send>,
        progress: impl FnMut(ExportProgress) + Send + 'static,
    ) -> JoinHandle<Result<u64, UiError>> {
        let now = self.clock.now_ms();
        export_store(&self.store, out, now, progress)
    }

    /// Number of records in the session history.
    pub fn history_len(&self) -> usize {
        self.store.history_len()
    }

    /// Min / max buckets of the session history over `t0..=t1`.
    pub fn history_range(
        &self,
        series: &[Series],
        t0: u64,
        t1: u64,
        buckets: usize,
    ) -> Vec<SeriesBuckets> {
        self.store.history_range(series, t0, t1, buckets)
    }

    /// The gaps between measuring runs of the session history in the open
    /// range `t0..t1`.
    pub fn history_gaps(&self, t0: u64, t1: u64) -> Vec<crate::history::HistoryGap> {
        self.store.history_gaps(t0, t1)
    }

    /// Discard the session history. A running export works on its own copy
    /// and a recording writes its own file, so neither is affected.
    pub fn clear_history(&mut self) {
        self.store.clear_history();
    }

    /// Zero the session statistics; they count again from the next record.
    pub fn reset_stats(&mut self) {
        self.store.reset_stats();
    }

    /// The session statistics so far.
    pub fn stats(&self) -> StatsSnapshot {
        self.store.stats()
    }

    /// Run one cycle: poll the queue when due, send a batch when due, then
    /// sleep until the next of the two.
    pub fn step(&mut self) -> Step {
        if self.session.is_none() {
            return Step::Ended;
        }
        let now = self.clock.now_ms();
        if self.poll_holds > 0 {
            // Held: the next poll waits a period, the session stays open.
            if now >= self.next_poll_ms {
                self.next_poll_ms = now.saturating_add(
                    u64::try_from(Streamer::poll_period(self.rate).as_millis()).unwrap_or(u64::MAX),
                );
            }
        } else if now >= self.next_poll_ms {
            if let Step::Ended = self.poll_queue() {
                return Step::Ended;
            }
            self.tick_recording();
            self.next_poll_ms = now.saturating_add(
                u64::try_from(Streamer::poll_period(self.rate).as_millis()).unwrap_or(u64::MAX),
            );
        }
        if let Step::Ended = self.poll_pd() {
            return Step::Ended;
        }
        let live = self.live_temp();
        self.batcher.set_temp(live);
        let now = self.clock.now_ms();
        if let Some(batch) = self.batcher.poll(now) {
            self.emit_batch(batch);
        }
        if let Some(rows) = self.pd.take_due(now) {
            self.emit(BackendEvent::PdRows(PdRows { rows }));
        }
        if let Some(samples) = self.pd.take_status_due(now) {
            self.emit(BackendEvent::PdStatus(PdStatusBatch { samples }));
        }
        let mut wake = self.next_poll_ms.min(self.batcher.next_due_ms());
        if let Some(run) = &self.pd_run {
            wake = wake.min(run.next_poll_ms);
        }
        if let Some(due) = self.pd.next_due_ms() {
            wake = wake.min(due);
        }
        if let Some(due) = self.pd.next_status_due_ms() {
            wake = wake.min(due);
        }
        let wait = wake.saturating_sub(now).max(1);
        self.clock.sleep(Duration::from_millis(wait));
        Step::Continue
    }

    /// Hold (`true`) or release (`false`) the queue polling; holds nest.
    /// A release that lets the last hold go polls at once.
    pub fn hold_polling(&mut self, hold: bool) {
        if hold {
            self.poll_holds = self.poll_holds.saturating_add(1);
        } else if self.poll_holds > 0 {
            self.poll_holds -= 1;
            if self.poll_holds == 0 {
                self.next_poll_ms = self.clock.now_ms();
            }
        }
    }

    /// Poll PD_PACKET when the sniffer runs and a poll is due.
    fn poll_pd(&mut self) -> Step {
        if self.poll_holds > 0 {
            return Step::Continue;
        }
        let now = self.clock.now_ms();
        let (Some(session), Some(run)) = (self.session.as_mut(), self.pd_run.as_mut()) else {
            return Step::Continue;
        };
        if now < run.next_poll_ms {
            return Step::Continue;
        }
        let period = PdSniffer::<GuardedTransport<T>, C>::POLL_PERIOD;
        run.next_poll_ms =
            now.saturating_add(u64::try_from(period.as_millis()).unwrap_or(u64::MAX));
        let clock = &mut run.clock;
        let polled = blocked(self.meter.as_ref(), || {
            session.poll_pd_sniffer(PdOptions::default(), clock)
        });
        match polled {
            Ok(poll) => {
                // PD times go onto the samples' session time.
                let store = &self.store;
                let offset = blocked(self.meter.as_ref(), || store.offset_ms());
                self.pd.set_session_offset(offset);
                self.pd.push(&poll.records, poll.status);
                Step::Continue
            }
            Err(e) => self.lose_session(&e),
        }
    }

    /// The pipe failed: drop the session, end the recording and a running
    /// PD capture, then report the disconnect.
    fn lose_session(&mut self, e: &session::Error) -> Step {
        self.session = None;
        self.end_recording(true);
        self.flush_batch();
        if self.pd_run.take().is_some() {
            self.emit_pd_stopped();
        }
        self.emit(disconnected(Some(session_error(e))));
        Step::Ended
    }

    /// Send every waiting PD row now, then report the capture stopped.
    fn emit_pd_stopped(&mut self) {
        if let Some(rows) = self.pd.take_all() {
            self.emit(BackendEvent::PdRows(PdRows { rows }));
        }
        if let Some(samples) = self.pd.take_all_status() {
            self.emit(BackendEvent::PdStatus(PdStatusBatch { samples }));
        }
        self.emit(BackendEvent::PdState(PdState { capturing: false }));
    }

    /// Records and the PD sniffer use requests that were never checked on
    /// the CDC pipe (memory reads, auth, 0x10), so a CDC session refuses them.
    fn needs_queue_source(&self) -> Result<(), UiError> {
        match self.source {
            MeasurementSource::Queue => Ok(()),
            MeasurementSource::Cdc => Err(UiError::usage(
                "Not available with the serial stream; reconnect with the USB queue source.",
            )),
        }
    }

    /// Start the PD sniffer (0x10, mode 1) and poll PD_PACKET from now on.
    pub fn start_pd(&mut self) -> Result<(), UiError> {
        self.needs_queue_source()?;
        let Some(session) = self.session.as_mut() else {
            return Err(not_connected());
        };
        if self.pd_run.is_some() {
            return Ok(());
        }
        blocked(self.meter.as_ref(), || session.start_pd_sniffer())
            .map_err(|e| session_error(&e))?;
        self.pd.start_run();
        let now = self.clock.now_ms();
        self.pd_run = Some(PdRun {
            clock: CaptureClock::new(),
            next_poll_ms: now,
        });
        self.emit(BackendEvent::PdState(PdState { capturing: true }));
        Ok(())
    }

    /// Stop the PD sniffer (0x11); the captured records stay.
    pub fn stop_pd(&mut self) -> Result<(), UiError> {
        if self.pd_run.take().is_none() {
            return Ok(());
        }
        let result = match self.session.as_mut() {
            Some(session) => blocked(self.meter.as_ref(), || session.stop_pd_sniffer())
                .map_err(|e| session_error(&e)),
            None => Err(not_connected()),
        };
        self.emit_pd_stopped();
        result
    }

    /// The raw bytes and field tree of captured record `index`.
    pub fn pd_details(&mut self, index: u64) -> Result<PdDetails, UiError> {
        self.pd
            .details(index)
            .ok_or_else(|| UiError::usage("That PD record is not in the capture."))
    }

    /// Drop every captured PD record; a running capture goes on. Returns
    /// the index the next record gets.
    pub fn clear_pd(&mut self) -> Result<u64, UiError> {
        Ok(self.pd.clear())
    }

    /// Read the meter's SETTINGS block (GET_DATA 0x008 only) for the
    /// read-only settings panel (FR 1HHT01); the stream goes on afterwards.
    pub fn read_settings(&mut self) -> Result<MeterSettingsView, UiError> {
        let Some(session) = self.session.as_mut() else {
            return Err(not_connected());
        };
        let settings = blocked(self.meter.as_ref(), || session.read_meter_settings())
            .map_err(|e| session_error(&e))?;
        Ok(MeterSettingsView::from(&settings))
    }

    /// List the meter's offline records (GET_DATA 0x200 only) for the
    /// Records view (FR 1HHT00); the stream goes on afterwards.
    pub fn list_records(&mut self) -> Result<RecordListView, UiError> {
        self.needs_queue_source()?;
        let Some(session) = self.session.as_mut() else {
            return Err(not_connected());
        };
        let entries = blocked(self.meter.as_ref(), || session.list_records())
            .map_err(|e| session_error(&e))?;
        Ok(RecordListView {
            records: entries
                .iter()
                .enumerate()
                .map(|(i, e)| RecordEntryView::new(i + 1, e))
                .collect(),
        })
    }

    /// Download offline record `index` (1-based): a fresh record list, then
    /// that record's window as consecutive memory reads of at most 256 bytes
    /// (FR 1HHT00).
    pub fn download_record(&mut self, index: usize) -> Result<OfflineRecordView, UiError> {
        self.needs_queue_source()?;
        let Some(session) = self.session.as_mut() else {
            return Err(not_connected());
        };
        let meter = self.meter.as_ref();
        let entries = blocked(meter, || session.list_records()).map_err(|e| session_error(&e))?;
        let entry = index
            .checked_sub(1)
            .and_then(|i| entries.get(i))
            .ok_or_else(|| UiError::usage("That record is not on the meter."))?;
        if entry.points == 0 {
            return Err(UiError::usage("That record has no samples."));
        }
        let samples =
            blocked(meter, || session.download_record(entry)).map_err(|e| session_error(&e))?;
        Ok(OfflineRecordView {
            index,
            name: entry.name.clone(),
            interval_s: entry.interval_s,
            samples: samples
                .iter()
                .enumerate()
                .map(|(k, s)| RecordSampleView::new(entry, k, s))
                .collect(),
        })
    }

    /// Delete offline record `target` (1-based) or, for `None`, every
    /// record (FR JKWT3V). `confirm` must be the record's name (or "all").
    /// The affected records are first downloaded to `NN_<name>.csv` files
    /// in `backup_dir` (created if missing); a failed backup deletes
    /// nothing. Then the record table is rewritten once and read again; a
    /// failure after the write names the backup folder and is not retried.
    pub fn delete_records(
        &mut self,
        target: Option<usize>,
        confirm: &str,
        backup_dir: &Path,
    ) -> Result<RecordDeleteView, UiError> {
        self.needs_queue_source()?;
        let Some(session) = self.session.as_mut() else {
            return Err(not_connected());
        };
        let meter = self.meter.as_ref();
        let entries = blocked(meter, || session.list_records()).map_err(|e| session_error(&e))?;
        let (positions, expected) = match target {
            Some(index) => {
                let entry = index
                    .checked_sub(1)
                    .and_then(|i| entries.get(i))
                    .ok_or_else(|| UiError::usage("That record is not on the meter."))?;
                (vec![index - 1], entry.name.as_str())
            }
            None => ((0..entries.len()).collect(), "all"),
        };
        if confirm != expected {
            return Err(UiError::usage(
                "The typed confirmation does not match; nothing was deleted.",
            ));
        }
        let backup_failed = |detail: String| UiError {
            kind: ErrorKind::Internal,
            message: format!("The backup failed, so nothing was deleted: {detail}"),
            hint: None,
        };
        std::fs::create_dir_all(backup_dir)
            .map_err(|e| backup_failed(format!("cannot create {}: {e}", backup_dir.display())))?;
        let mut backups = Vec::with_capacity(positions.len());
        for &p in &positions {
            let entry = &entries[p];
            let samples = if entry.points == 0 {
                Vec::new()
            } else {
                blocked(meter, || session.download_record(entry)).map_err(|e| {
                    let ui = session_error(&e);
                    UiError {
                        message: format!(
                            "The backup download failed, so nothing was deleted. {}",
                            ui.message
                        ),
                        ..ui
                    }
                })?
            };
            let path = backup_dir.join(powerz_record::offline_record_file_name(p + 1, &entry.name));
            std::fs::write(&path, powerz_record::offline_record_csv(entry, &samples))
                .map_err(|e| backup_failed(format!("cannot write {}: {e}", path.display())))?;
            backups.push(path.display().to_string());
        }
        let after =
            blocked(meter, || session.delete_records(&entries, &positions)).map_err(|e| {
                let ui = session_error(&e);
                UiError {
                    message: format!("{} Backup: {}", ui.message, backup_dir.display()),
                    ..ui
                }
            })?;
        Ok(RecordDeleteView {
            records: after
                .iter()
                .enumerate()
                .map(|(i, e)| RecordEntryView::new(i + 1, e))
                .collect(),
            backup_dir: backup_dir.display().to_string(),
            backups,
        })
    }

    /// The PD capture so far, to be saved as a pdcap file (FR TGE83E) off
    /// this thread.
    pub fn pd_save(&self) -> PdSave {
        let (model, firmware_version) = self.pd_meter.clone();
        PdSave {
            header: pdcap::Header::new(env!("CARGO_PKG_VERSION"), model, firmware_version),
            entries: self.pd.to_pdcap(),
        }
    }

    /// Poll the queue (and the snapshot when due) once.
    fn poll_queue(&mut self) -> Step {
        let Some(session) = self.session.as_mut() else {
            return Step::Ended;
        };
        let polled = blocked(self.meter.as_ref(), || match self.source {
            MeasurementSource::Queue => session.poll_stream(),
            MeasurementSource::Cdc => session.poll_adcnew(),
        });
        match polled {
            Ok(poll) => {
                let times: Vec<u32> = self
                    .last_record_ms
                    .into_iter()
                    .chain(poll.records.iter().map(QueueRecord::time_ms))
                    .collect();
                self.last_record_ms = times.last().copied();
                if self.rate_locked
                    && let Some(sps) = measured_sps(&times)
                    && self.measured_sps != Some(sps)
                {
                    self.measured_sps = Some(sps);
                    if let Some(rate) = Rate::from_sps(sps) {
                        self.rate = rate;
                    }
                    self.emit(BackendEvent::Status(Status::Rate {
                        rate_sps: sps,
                        rate_locked: true,
                    }));
                }
                self.queue_records(&poll);
                // A failed snapshot only costs the temperature reading; a
                // dead pipe fails the next queue poll and ends the session.
                if self.snapshot_if_due().is_err() {
                    self.temp_c = None;
                }
                Step::Continue
            }
            Err(e) => self.lose_session(&e),
        }
    }

    /// Change the queue rate.
    pub fn set_rate(&mut self, rate: Rate) -> Result<(), UiError> {
        if self.rate_locked {
            return Err(UiError::usage(
                "Changing the rate needs authentication, which the meter refused.",
            ));
        }
        // A queue restart can restart the device clock, which a recording
        // cannot follow; the file keeps one rate.
        if self.recording.is_some() {
            return Err(UiError::usage("Stop the recording to change the rate."));
        }
        let Some(session) = self.session.as_mut() else {
            return Err(not_connected());
        };
        match self.source {
            MeasurementSource::Queue => session.start_stream(rate),
            MeasurementSource::Cdc => session.start_adcnew(Content::Adc, rate),
        }
        .map_err(|e| session_error(&e))?;
        // The interval across the restart was not measured.
        let store = &self.store;
        blocked(self.meter.as_ref(), || store.start_run());
        self.rate = rate;
        self.emit(BackendEvent::Status(Status::Rate {
            rate_sps: rate.sps(),
            rate_locked: false,
        }));
        Ok(())
    }

    /// Stop the queue and close the pipe.
    pub fn stop(&mut self) {
        // The sniffer stops before the queue and the pipe close.
        let _ = self.stop_pd();
        if let Some(mut session) = self.session.take() {
            // Best effort: the pipe closes either way.
            let _ = match self.source {
                MeasurementSource::Queue => session.stop_stream(),
                MeasurementSource::Cdc => session.stop_adcnew(),
            };
            drop(session);
            self.end_recording(false);
            self.flush_batch();
            self.emit(disconnected(None));
        }
    }

    /// Carry out one request from the UI and send back the answer.
    fn apply(&mut self, command: Command) {
        // A requester that gave up waiting has dropped its receiver.
        match command {
            Command::SetRate(rate, reply) => {
                let _ = reply.send(self.set_rate(rate));
            }
            Command::RecordTo(path, reply) => {
                let _ = reply.send(self.record_to(&path));
            }
            Command::ExportTo(path, progress, reply) => {
                let _ = reply.send(self.export_to(&path, progress));
            }
            Command::StopRecording(reply) => {
                let _ = reply.send(self.stop_recording());
            }
            Command::StartPd(reply) => {
                let _ = reply.send(self.start_pd());
            }
            Command::StopPd(reply) => {
                let _ = reply.send(self.stop_pd());
            }
            Command::PdDetails(index, reply) => {
                let _ = reply.send(self.pd_details(index));
            }
            Command::ClearPd(reply) => {
                let _ = reply.send(self.clear_pd());
            }
            Command::PdSave(reply) => {
                let _ = reply.send(self.pd_save());
            }
            Command::ReadSettings(reply) => {
                let _ = reply.send(self.read_settings());
            }
            Command::ListRecords(reply) => {
                let _ = reply.send(self.list_records());
            }
            Command::DownloadRecord(index, reply) => {
                let _ = reply.send(self.download_record(index));
            }
            Command::DeleteRecords(target, confirm, dir, reply) => {
                let _ = reply.send(self.delete_records(target, &confirm, &dir));
            }
            Command::HoldPolling(hold, reply) => {
                self.hold_polling(hold);
                let _ = reply.send(());
            }
            Command::Handover(swap, reply) => {
                // Read here and swapped behind the events already sent, so
                // the new sink gets exactly the events after this answer.
                let progress = self.recording_progress();
                (self.in_order)(Box::new(move || {
                    swap();
                    let _ = reply.send(progress);
                }));
            }
        }
    }
}

/// Requests the UI sends to a running worker, applied between steps.
/// History and statistics queries do not go through the worker: they run
/// on the caller's thread against the [`SessionStore`].
enum Command {
    SetRate(Rate, mpsc::Sender<Result<(), UiError>>),
    RecordTo(PathBuf, mpsc::Sender<Result<(), UiError>>),
    ExportTo(
        PathBuf,
        ProgressFn,
        mpsc::Sender<Result<ExportJob, UiError>>,
    ),
    StopRecording(mpsc::Sender<Result<RecordingSummary, UiError>>),
    StartPd(mpsc::Sender<Result<(), UiError>>),
    StopPd(mpsc::Sender<Result<(), UiError>>),
    PdDetails(u64, mpsc::Sender<Result<PdDetails, UiError>>),
    ClearPd(mpsc::Sender<Result<u64, UiError>>),
    PdSave(mpsc::Sender<PdSave>),
    ReadSettings(mpsc::Sender<Result<MeterSettingsView, UiError>>),
    ListRecords(mpsc::Sender<Result<RecordListView, UiError>>),
    DownloadRecord(usize, mpsc::Sender<Result<OfflineRecordView, UiError>>),
    DeleteRecords(
        Option<usize>,
        String,
        PathBuf,
        mpsc::Sender<Result<RecordDeleteView, UiError>>,
    ),
    /// Hold or release the queue polling; answered between steps, so no
    /// poll is in flight once a hold is answered.
    HoldPolling(bool, mpsc::Sender<()>),
    /// Run the sink swap of an attach, then answer the open recording's
    /// progress: both on the worker thread, which sends every recording
    /// event, so none can fall between them (FR VEDDFD).
    Handover(Box<dyn FnOnce() + Send>, mpsc::Sender<Option<(u64, u64)>>),
}

impl std::fmt::Debug for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Command::SetRate(rate, _) => f.debug_tuple("SetRate").field(rate).finish(),
            Command::RecordTo(path, _) => f.debug_tuple("RecordTo").field(path).finish(),
            Command::ExportTo(path, ..) => f.debug_tuple("ExportTo").field(path).finish(),
            Command::StopRecording(_) => f.write_str("StopRecording"),
            Command::StartPd(_) => f.write_str("StartPd"),
            Command::StopPd(_) => f.write_str("StopPd"),
            Command::PdDetails(index, _) => f.debug_tuple("PdDetails").field(index).finish(),
            Command::ClearPd(_) => f.write_str("ClearPd"),
            Command::PdSave(_) => f.write_str("PdSave"),
            Command::ReadSettings(_) => f.write_str("ReadSettings"),
            Command::ListRecords(_) => f.write_str("ListRecords"),
            Command::DownloadRecord(index, _) => {
                f.debug_tuple("DownloadRecord").field(index).finish()
            }
            Command::DeleteRecords(target, _, dir, _) => f
                .debug_tuple("DeleteRecords")
                .field(target)
                .field(dir)
                .finish(),
            Command::HoldPolling(hold, _) => f.debug_tuple("HoldPolling").field(hold).finish(),
            Command::Handover(..) => f.write_str("Handover"),
        }
    }
}

type ProgressFn = Box<dyn FnMut(ExportProgress) + Send>;

/// Export every record of `store` to `out` on a thread of its own; the
/// export works on a snapshot that shares the stored samples. `now_ms` is
/// the `Unix` origin when the history is empty.
fn export_store(
    store: &SessionStore,
    out: Box<dyn Write + Send>,
    now_ms: u64,
    progress: impl FnMut(ExportProgress) + Send + 'static,
) -> ExportJob {
    let (history, start_unix_ms, temps) = store.export_source(now_ms);
    std::thread::spawn(move || {
        let mut cursor = 0;
        recording::export_iter(
            history.len() as u64,
            history.iter_runs().enumerate().map(|(i, (r, new_run))| {
                (r, new_run, temp_at(&temps, &mut cursor, i, r.time_ms()))
            }),
            out,
            start_unix_ms,
            progress,
        )
    })
}

/// Exports a session: through its worker while one runs, so the file being
/// recorded to is refused; straight from its store once the meter is gone.
#[derive(Debug, Clone)]
pub struct Exporter {
    control: Option<WorkerControl>,
    store: SessionStore,
    now_ms: u64,
}

impl Exporter {
    /// Export through `control` when the session is open, else from `store`;
    /// `now_ms` is the `Unix` origin of an empty history.
    pub(crate) fn new(
        control: Option<WorkerControl>,
        store: SessionStore,
        now_ms: u64,
    ) -> Exporter {
        Exporter {
            control,
            store,
            now_ms,
        }
    }

    /// Create the file at `path` and export the session to it.
    pub fn export_to(
        &self,
        path: &Path,
        progress: impl FnMut(ExportProgress) + Send + 'static,
    ) -> Result<ExportJob, UiError> {
        match &self.control {
            Some(control) => control.export_to(path, progress),
            None => {
                let file = recording::create_file(path)?;
                Ok(export_store(
                    &self.store,
                    Box::new(file),
                    self.now_ms,
                    progress,
                ))
            }
        }
    }
}

/// Write the file at `path` through `write`, atomically: into a new temp
/// file next to it, synced to disk, then renamed over `path`. On an error
/// only the temp file is removed, so an existing file at `path` stays whole.
pub(crate) fn write_file_atomically(
    path: &Path,
    write: impl FnOnce(&mut std::io::BufWriter<std::fs::File>) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let (tmp, file) = create_temp_beside(path)?;
    let attempt = || -> std::io::Result<()> {
        let mut out = std::io::BufWriter::new(file);
        write(&mut out)?;
        out.into_inner().map_err(|e| e.into_error())?.sync_all()?;
        std::fs::rename(&tmp, path)
    };
    attempt().inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })?;
    sync_parent(path);
    Ok(())
}

/// A new, empty hidden file in `path`'s folder (the rename into place must
/// not cross file systems), named after `path`.
fn create_temp_beside(path: &Path) -> std::io::Result<(PathBuf, std::fs::File)> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = match path.parent() {
        Some(d) if !d.as_os_str().is_empty() => d,
        _ => Path::new("."),
    };
    let name = path
        .file_name()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidFilename, "no file name"))?;
    loop {
        let mut tmp_name = std::ffi::OsString::from(".");
        tmp_name.push(name);
        tmp_name.push(format!(
            ".{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let tmp = dir.join(tmp_name);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
        {
            Ok(file) => return Ok((tmp, file)),
            // Left over from a crashed run with the same pid: next number.
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
}

/// Best effort: sync `path`'s folder so the rename itself is on disk.
fn sync_parent(path: &Path) {
    #[cfg(unix)]
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty())
        && let Ok(dir) = std::fs::File::open(dir)
    {
        let _ = dir.sync_all();
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// A PD capture to save (FR TGE83E): the copy the worker takes between two
/// polls, written to disk on the caller's thread so polling never waits
/// for the disk.
#[derive(Debug, Clone)]
pub struct PdSave {
    header: pdcap::Header,
    entries: Vec<pdcap::Entry>,
}

impl PdSave {
    /// Write the capture to `path` as a pdcap file, atomically; returns the
    /// number of records written. A failed save leaves an existing file at
    /// `path` as it was.
    pub fn write_to(&self, path: &Path) -> Result<u64, UiError> {
        let records = self
            .entries
            .iter()
            .filter(|e| matches!(e, pdcap::Entry::Record { .. }))
            .count() as u64;
        write_file_atomically(path, |out| {
            let mut w = pdcap::Writer::new(out, &self.header)?;
            for e in &self.entries {
                w.write(e)?;
            }
            w.finish().map(drop)
        })
        .map(|()| records)
        .map_err(|e| recording::io_error(&e))
    }
}

/// A running export: joins to the rows written.
pub type ExportJob = JoinHandle<Result<u64, UiError>>;

/// How long a request to the worker may take before the UI gives up.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(3);

/// How long the UI waits for one offline-record download.
const RECORD_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30);

/// How long a delete may take: the backup downloads every affected record
/// (up to the whole record area) before the table write.
const RECORD_DELETE_TIMEOUT: Duration = Duration::from_secs(300);

/// Sends rate, recording and export requests to a running worker and
/// answers history and statistics queries from its session store. It works
/// without the [`WorkerHandle`], so a caller can wait for the answer without
/// holding the lock that guards the handle.
#[derive(Debug, Clone)]
pub struct WorkerControl {
    commands: mpsc::Sender<Command>,
    store: SessionStore,
}

/// The answer when the worker is gone.
fn not_connected() -> UiError {
    UiError::usage("The meter is not connected.")
}

/// The answer when the worker does not reply in time; the request may still
/// be carried out.
fn busy() -> UiError {
    UiError {
        kind: ErrorKind::Timeout,
        message: "The meter is busy. Try again.".to_owned(),
        hint: None,
    }
}

impl WorkerControl {
    /// Send `command` and wait (up to [`COMMAND_TIMEOUT`]) for its answer.
    fn ask<R>(&self, command: Command, answer: &mpsc::Receiver<R>) -> Result<R, UiError> {
        self.ask_within(command, answer, COMMAND_TIMEOUT)
    }

    /// Send `command` and wait up to `timeout` for its answer.
    fn ask_within<R>(
        &self,
        command: Command,
        answer: &mpsc::Receiver<R>,
        timeout: Duration,
    ) -> Result<R, UiError> {
        self.commands.send(command).map_err(|_| not_connected())?;
        answer.recv_timeout(timeout).map_err(|e| match e {
            mpsc::RecvTimeoutError::Timeout => busy(),
            mpsc::RecvTimeoutError::Disconnected => not_connected(),
        })
    }

    /// Restart the running queue at `rate` and wait for the worker's answer.
    pub fn set_rate(&self, rate: Rate) -> Result<(), UiError> {
        let (reply, answer) = mpsc::channel();
        self.ask(Command::SetRate(rate, reply), &answer)?
    }

    /// Record to a new file at `path`. The worker checks first and creates
    /// the file only when it is going to record, so a refused start (or a
    /// timed-out answer) never truncates or deletes anything.
    pub fn record_to(&self, path: &Path) -> Result<(), UiError> {
        let (reply, answer) = mpsc::channel();
        self.ask(Command::RecordTo(path.to_owned(), reply), &answer)?
    }

    /// Stop the recording and return what it wrote.
    pub fn stop_recording(&self) -> Result<RecordingSummary, UiError> {
        let (reply, answer) = mpsc::channel();
        self.ask(Command::StopRecording(reply), &answer)?
    }

    /// Run `swap` on the worker thread between its steps and answer the
    /// open recording's rows and elapsed ms then (`None` when nothing is
    /// being recorded): no recording event is sent between the two. On an
    /// error `swap` has not run and never will.
    pub fn hand_over(&self, swap: Box<dyn FnOnce() + Send>) -> Result<Option<(u64, u64)>, UiError> {
        self.hand_over_within(swap, COMMAND_TIMEOUT)
    }

    fn hand_over_within(
        &self,
        swap: Box<dyn FnOnce() + Send>,
        timeout: Duration,
    ) -> Result<Option<(u64, u64)>, UiError> {
        // `None` while waiting; `Some(true)` once the swap ran, `Some(false)`
        // once the caller gave up, so a late command does nothing.
        let gate = Arc::new(Mutex::new(None::<bool>));
        let worker_gate = Arc::clone(&gate);
        let guarded = Box::new(move || {
            let mut state = worker_gate.lock().unwrap_or_else(PoisonError::into_inner);
            if state.is_none() {
                swap();
                *state = Some(true);
            }
        });
        let (reply, answer) = mpsc::channel();
        match self.ask_within(Command::Handover(guarded, reply), &answer, timeout) {
            Err(e) if e.kind == ErrorKind::Timeout => {
                let mut state = gate.lock().unwrap_or_else(PoisonError::into_inner);
                if *state == Some(true) {
                    drop(state);
                    // The swap ran just as the wait ended: its answer follows at once.
                    return answer.recv().map_err(|_| not_connected());
                }
                *state = Some(false);
                Err(e)
            }
            other => other,
        }
    }

    /// Hold (`true`) or release (`false`) the queue polling without closing
    /// the session; holds nest. Once a hold is answered no poll is in flight.
    pub fn hold_polling(&self, hold: bool) -> Result<(), UiError> {
        let (reply, answer) = mpsc::channel();
        self.ask(Command::HoldPolling(hold, reply), &answer)
    }

    /// Start the PD capture.
    pub fn start_pd(&self) -> Result<(), UiError> {
        let (reply, answer) = mpsc::channel();
        self.ask(Command::StartPd(reply), &answer)?
    }

    /// Stop the PD capture.
    pub fn stop_pd(&self) -> Result<(), UiError> {
        let (reply, answer) = mpsc::channel();
        self.ask(Command::StopPd(reply), &answer)?
    }

    /// The raw bytes and field tree of captured PD record `index`.
    pub fn pd_details(&self, index: u64) -> Result<PdDetails, UiError> {
        let (reply, answer) = mpsc::channel();
        self.ask(Command::PdDetails(index, reply), &answer)?
    }

    /// Drop every captured PD record; returns the index the next one gets.
    pub fn clear_pd(&self) -> Result<u64, UiError> {
        let (reply, answer) = mpsc::channel();
        self.ask(Command::ClearPd(reply), &answer)?
    }

    /// Read the meter's settings for the read-only panel.
    pub fn read_settings(&self) -> Result<MeterSettingsView, UiError> {
        let (reply, answer) = mpsc::channel();
        self.ask(Command::ReadSettings(reply), &answer)?
    }

    /// List the meter's offline records.
    pub fn list_records(&self) -> Result<RecordListView, UiError> {
        let (reply, answer) = mpsc::channel();
        self.ask(Command::ListRecords(reply), &answer)?
    }

    /// Download offline record `index` (1-based). A full record area is
    /// about 2 MB, so the wait is longer than for other commands.
    pub fn download_record(&self, index: usize) -> Result<OfflineRecordView, UiError> {
        let (reply, answer) = mpsc::channel();
        self.ask_within(
            Command::DownloadRecord(index, reply),
            &answer,
            RECORD_DOWNLOAD_TIMEOUT,
        )?
    }

    /// Delete offline record `target` (1-based) or every record (`None`)
    /// after a CSV backup into `backup_dir`; `confirm` is the typed record
    /// name (or "all"). Backing up every record can take a while, so the
    /// wait is longer than for a single download.
    pub fn delete_records(
        &self,
        target: Option<usize>,
        confirm: &str,
        backup_dir: &Path,
    ) -> Result<RecordDeleteView, UiError> {
        let (reply, answer) = mpsc::channel();
        self.ask_within(
            Command::DeleteRecords(target, confirm.to_owned(), backup_dir.to_owned(), reply),
            &answer,
            RECORD_DELETE_TIMEOUT,
        )?
    }

    /// Save the PD capture to `path` as a pdcap file; returns the number of
    /// records written. The worker only copies the capture; the file is
    /// written and synced on this thread.
    pub fn save_pd(&self, path: &Path) -> Result<u64, UiError> {
        let (reply, answer) = mpsc::channel();
        self.ask(Command::PdSave(reply), &answer)?.write_to(path)
    }

    /// The session statistics so far.
    pub fn stats(&self) -> Result<StatsSnapshot, UiError> {
        Ok(self.store.stats())
    }

    /// Zero the session statistics.
    pub fn reset_stats(&self) -> Result<(), UiError> {
        self.store.reset_stats();
        Ok(())
    }

    /// Discard the session history.
    pub fn clear_history(&self) -> Result<(), UiError> {
        self.store.clear_history();
        Ok(())
    }

    /// Min / max buckets of the session history over `t0..=t1`.
    pub fn history_range(
        &self,
        series: &[Series],
        t0: u64,
        t1: u64,
        buckets: usize,
    ) -> Result<Vec<SeriesBuckets>, UiError> {
        Ok(self.store.history_range(series, t0, t1, buckets))
    }

    /// Create the file at `path` and export the session to it.
    pub fn export_to(
        &self,
        path: &Path,
        progress: impl FnMut(ExportProgress) + Send + 'static,
    ) -> Result<ExportJob, UiError> {
        let (reply, answer) = mpsc::channel();
        self.ask(
            Command::ExportTo(path.to_owned(), Box::new(progress), reply),
            &answer,
        )?
    }
}

/// Handle to a worker running on its own thread. Dropping it stops the
/// worker without waiting for it.
#[derive(Debug)]
pub struct WorkerHandle {
    stop: Arc<AtomicBool>,
    /// Set when the `disconnected` status has been sent.
    ended: Arc<AtomicBool>,
    commands: mpsc::Sender<Command>,
    store: SessionStore,
    thread: Option<JoinHandle<()>>,
}

impl WorkerHandle {
    /// The session has ended (closed or failed): its `disconnected` status
    /// is out, or its thread has returned.
    pub fn is_finished(&self) -> bool {
        self.ended.load(Ordering::SeqCst)
            || self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }

    /// Restart the running queue at `rate`.
    pub fn set_rate(&self, rate: Rate) -> Result<(), UiError> {
        self.control().set_rate(rate)
    }

    /// A rate, recording and export control that works without this handle.
    pub fn control(&self) -> WorkerControl {
        WorkerControl {
            commands: self.commands.clone(),
            store: self.store.clone(),
        }
    }

    /// Stop the worker and wait for it to close the pipe.
    pub fn disconnect(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }

    /// Let go of a session that has already ended, without raising the stop
    /// flag it shares with the page (a reconnect hands that flag on) and
    /// without waiting for its thread, which finishes on its own.
    pub fn retire(mut self) {
        self.stop = Arc::new(AtomicBool::new(true));
        self.thread = None;
    }

    /// Stop the worker and wait up to `timeout` for it to stop the queue and
    /// close the pipe; past that it finishes on its own. `true` when it did
    /// within the time.
    pub fn disconnect_within(mut self, timeout: Duration) -> bool {
        self.stop.store(true, Ordering::SeqCst);
        let Some(thread) = self.thread.take() else {
            return true;
        };
        let deadline = std::time::Instant::now() + timeout;
        while !thread.is_finished() {
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let _ = thread.join();
        true
    }
}

impl Drop for WorkerHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// Open the pipe with `open` on a dedicated thread and run a worker there.
pub fn spawn<T, C, O>(open: O, clock: C, config: WorkerConfig, sink: impl EventSink) -> WorkerHandle
where
    T: Transport,
    C: Clock + Clone + Send + 'static,
    O: FnOnce() -> Result<T, transport::Error> + Send + 'static,
{
    spawn_with_stop(open, clock, config, sink, Arc::new(AtomicBool::new(false)))
}

/// [`spawn`] with a stop flag the caller also holds: setting it ends the
/// session, as when the UI channel the events go to has closed.
pub fn spawn_with_stop<T, C, O>(
    open: O,
    clock: C,
    config: WorkerConfig,
    sink: impl EventSink,
    stop: Arc<AtomicBool>,
) -> WorkerHandle
where
    T: Transport,
    C: Clock + Clone + Send + 'static,
    O: FnOnce() -> Result<T, transport::Error> + Send + 'static,
{
    spawn_with_store(open, clock, config, SessionStore::new(), sink, stop)
}

/// [`spawn_with_stop`] appending to the session in `store`, as a reconnect
/// of the same meter does.
pub fn spawn_with_store<T, C, O>(
    open: O,
    clock: C,
    config: WorkerConfig,
    store: SessionStore,
    sink: impl EventSink,
    stop: Arc<AtomicBool>,
) -> WorkerHandle
where
    T: Transport,
    C: Clock + Clone + Send + 'static,
    O: FnOnce() -> Result<T, transport::Error> + Send + 'static,
{
    spawn_inner(open, clock, config, store, sink, stop, None)
}

/// [`spawn_with_store`] adding the wall time the worker spends blocked in
/// its own calls to `meter` (FR KQF5GM).
pub fn spawn_metered<T, C, O>(
    open: O,
    clock: C,
    config: WorkerConfig,
    store: SessionStore,
    sink: impl EventSink,
    stop: Arc<AtomicBool>,
    meter: BlockedMeter,
) -> WorkerHandle
where
    T: Transport,
    C: Clock + Clone + Send + 'static,
    O: FnOnce() -> Result<T, transport::Error> + Send + 'static,
{
    spawn_inner(open, clock, config, store, sink, stop, Some(meter))
}

fn spawn_inner<T, C, O>(
    open: O,
    clock: C,
    config: WorkerConfig,
    store: SessionStore,
    sink: impl EventSink,
    stop: Arc<AtomicBool>,
    meter: Option<BlockedMeter>,
) -> WorkerHandle
where
    T: Transport,
    C: Clock + Clone + Send + 'static,
    O: FnOnce() -> Result<T, transport::Error> + Send + 'static,
{
    let flag = Arc::clone(&stop);
    let (commands, inbox) = mpsc::channel();
    let ended = Arc::new(AtomicBool::new(false));
    let ended_by_sink = Arc::clone(&ended);
    let mut sink = sink;
    // The session is over the moment its `disconnected` status goes out, so
    // the UI may reconnect at once even though this thread is still unwinding.
    let marking = move |event: BackendEvent| {
        if let BackendEvent::Status(Status::Disconnected { .. }) = event {
            ended_by_sink.store(true, Ordering::SeqCst);
        }
        sink(event);
    };
    // A slow sink (a throttled UI) must never hold up the polls: the meter
    // keeps only ~60 ms of records at 1000 SPS. The worker queues its
    // events; a forwarder thread delivers them in order (FR X2SPDW).
    let (forward, queued) = mpsc::channel::<Forward>();
    let depth = Arc::new(AtomicUsize::new(0));
    let delivered = Arc::clone(&depth);
    let stop_by_forwarder = Arc::clone(&stop);
    let ended_by_forwarder = Arc::clone(&ended);
    let forwarder = std::thread::spawn(move || {
        let mut marking = marking;
        for item in queued {
            let ran = catch_unwind(AssertUnwindSafe(|| match item {
                Forward::Event(event) => {
                    let batch = matches!(event, BackendEvent::Batch(_));
                    marking(event);
                    if batch {
                        delivered.fetch_sub(1, Ordering::SeqCst);
                    }
                }
                Forward::Run(task) => task(),
            }));
            if ran.is_err() {
                // Nobody hears this session any more: end it.
                ended_by_forwarder.store(true, Ordering::SeqCst);
                stop_by_forwarder.store(true, Ordering::SeqCst);
                return;
            }
        }
    });
    // Kept so a panicking worker can still report that the session ended.
    let sink = forward.clone();
    let worker_sink = forward;
    let worker_store = store.clone();
    let thread = std::thread::spawn(move || {
        crate::activity::raise_poll_thread_qos();
        let ran = catch_unwind(AssertUnwindSafe(|| {
            run_worker(
                open,
                clock,
                config,
                worker_store,
                &flag,
                &inbox,
                worker_sink,
                depth,
                meter,
            )
        }));
        if let Err(payload) = ran {
            let _ = sink.send(Forward::Event(disconnected(Some(ui_error::from_panic(
                payload.as_ref(),
            )))));
        }
        // Every event is out before the thread ends, so a reconnect or an
        // attach after `disconnect` never sees this session's last ones.
        drop(sink);
        let _ = forwarder.join();
    });
    WorkerHandle {
        stop,
        ended,
        commands,
        store,
        thread: Some(thread),
    }
}

/// Most sample batches waiting for a slow sink (about 10 to 25 s of them);
/// status, gap and recording events are always queued.
const MAX_QUEUED_BATCHES: usize = 512;

/// The worker thread's body: open, then step until stopped or ended.
#[allow(clippy::too_many_arguments)]
fn run_worker<T, C, O>(
    open: O,
    clock: C,
    config: WorkerConfig,
    store: SessionStore,
    stop: &AtomicBool,
    inbox: &mpsc::Receiver<Command>,
    sink: mpsc::Sender<Forward>,
    depth: Arc<AtomicUsize>,
    meter: Option<BlockedMeter>,
) where
    T: Transport,
    C: Clock + Clone,
    O: FnOnce() -> Result<T, transport::Error>,
{
    let ordered = sink.clone();
    let ordered_meter = meter.clone();
    let in_order: InOrder = Box::new(move |task| {
        // The forwarder outlives every sender, so this cannot fail.
        blocked(ordered_meter.as_ref(), || {
            let _ = ordered.send(Forward::Run(task));
        });
    });
    let forward_meter = meter.clone();
    let forward: Sink = Box::new(move |event| {
        if let BackendEvent::Batch(_) = event {
            // A sink stalled for this long gets no more live batches until it
            // catches up; the history and any recording keep every sample.
            if depth.load(Ordering::SeqCst) >= MAX_QUEUED_BATCHES {
                return;
            }
            depth.fetch_add(1, Ordering::SeqCst);
        }
        blocked(forward_meter.as_ref(), || {
            let _ = sink.send(Forward::Event(event));
        });
    });
    let opened = open()
        .map_err(|e| ui_error::from_transport(&e, std::env::consts::OS))
        .and_then(|transport| open_session(transport, clock.clone(), config));
    // Held until this thread is done with the meter (FR X2SPDW).
    let _activity = opened
        .is_ok()
        .then(|| crate::activity::hold("Streaming from a POWER-Z meter"));
    let mut worker = match opened {
        Ok((session, info)) => Worker::running(
            session, clock, config, info, store, forward, in_order, meter,
        ),
        Err(error) => {
            let mut forward = forward;
            forward(disconnected(Some(error)));
            return;
        }
    };
    'run: while !stop.load(Ordering::SeqCst) {
        loop {
            match inbox.try_recv() {
                Ok(command) => worker.apply(command),
                Err(mpsc::TryRecvError::Empty) => break,
                // Every handle is gone: nobody can stop or steer this session.
                Err(mpsc::TryRecvError::Disconnected) => break 'run,
            }
        }
        if let Step::Ended = worker.step() {
            return;
        }
    }
    worker.stop();
}

/// A stream control whose worker answers the first polling hold only after
/// its caller gave up waiting ([`COMMAND_TIMEOUT`] has passed), then the
/// rest at once; every hold (`true`) / release (`false`) is logged in the
/// order the worker applies them.
#[cfg(test)]
pub(crate) fn late_hold_control() -> (WorkerControl, Arc<Mutex<Vec<bool>>>) {
    let (commands, inbox) = mpsc::channel::<Command>();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    std::thread::spawn(move || {
        let mut late = true;
        while let Ok(command) = inbox.recv() {
            if let Command::HoldPolling(hold, reply) = command {
                if late {
                    late = false;
                    std::thread::sleep(COMMAND_TIMEOUT + Duration::from_millis(300));
                }
                log.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(hold);
                let _ = reply.send(());
            }
        }
    });
    let control = WorkerControl {
        commands,
        store: SessionStore::new(),
    };
    (control, seen)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn control() -> (WorkerControl, mpsc::Receiver<Command>) {
        let (commands, inbox) = mpsc::channel();
        let control = WorkerControl {
            commands,
            store: SessionStore::new(),
        };
        (control, inbox)
    }

    /// A fresh scratch directory for one test.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("open-powerz-worker-{}", std::process::id()))
            .join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    fn listing(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .expect("list scratch dir")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_failed_overwrite_keeps_the_old_file_and_leaves_no_temp_file() {
        let dir = scratch("atomic-fail");
        let path = dir.join("capture.pdcap");
        std::fs::write(&path, b"the good old capture").expect("old file");
        let result = write_file_atomically(&path, |out| {
            out.write_all(b"half a new cap")?;
            Err(std::io::Error::other("disk full"))
        });
        assert!(result.is_err());
        assert_eq!(
            std::fs::read(&path).expect("the old file is still there"),
            b"the good old capture"
        );
        assert_eq!(listing(&dir), ["capture.pdcap"], "no temp file left");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_good_overwrite_replaces_the_file_and_leaves_no_temp_file() {
        let dir = scratch("atomic-ok");
        let path = dir.join("capture.pdcap");
        std::fs::write(&path, b"old").expect("old file");
        write_file_atomically(&path, |out| out.write_all(b"new capture")).expect("saved");
        assert_eq!(std::fs::read(&path).expect("file"), b"new capture");
        assert_eq!(listing(&dir), ["capture.pdcap"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_new_file_leaves_nothing_behind() {
        let dir = scratch("atomic-new");
        let path = dir.join("capture.pdcap");
        let result = write_file_atomically(&path, |_| Err(std::io::Error::other("nope")));
        assert!(result.is_err());
        assert!(listing(&dir).is_empty(), "{:?}", listing(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_save_takes_only_a_copy_from_the_worker_and_writes_on_the_callers_thread() {
        let dir = scratch("save-off-worker");
        let path = dir.join("capture.pdcap");
        let (control, inbox) = control();
        let caller = std::thread::current().id();
        // The worker answers with the copy and is free at once; it never
        // sees the path or touches the file.
        let worker = std::thread::spawn(move || match inbox.recv().expect("a command") {
            Command::PdSave(reply) => {
                let save = PdSave {
                    header: pdcap::Header::new("0.0.0", "KM003C".into(), "1.0".into()),
                    entries: Vec::new(),
                };
                reply.send(save).expect("the caller waits");
                std::thread::current().id()
            }
            other => panic!("the worker was asked for {other:?}"),
        });
        assert_eq!(control.save_pd(&path), Ok(0));
        let worker_thread = worker.join().expect("worker thread");
        assert_ne!(worker_thread, caller);
        let loaded = pdcap::load(std::io::BufReader::new(
            std::fs::File::open(&path).expect("the caller wrote the file"),
        ));
        assert!(loaded.header.is_some() && loaded.error.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn run(command: Command) {
        if let Command::Handover(swap, reply) = command {
            swap();
            let _ = reply.send(None);
        }
    }

    #[test]
    fn a_handover_given_up_never_swaps_later() {
        let (control, inbox) = control();
        let swaps = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&swaps);
        let got = control.hand_over_within(
            Box::new(move || {
                counted.fetch_add(1, Ordering::SeqCst);
            }),
            Duration::from_millis(10),
        );
        assert_eq!(got.map_err(|e| e.kind), Err(ErrorKind::Timeout));
        // The worker gets to the queued command only now.
        run(inbox.recv().expect("the handover was not sent"));
        assert_eq!(
            swaps.load(Ordering::SeqCst),
            0,
            "a late handover swapped the sink"
        );
    }

    #[test]
    fn a_handover_answers_once_the_worker_ran_it() {
        let (control, inbox) = control();
        let worker = std::thread::spawn(move || run(inbox.recv().expect("no handover")));
        let swaps = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&swaps);
        let got = control.hand_over(Box::new(move || {
            counted.fetch_add(1, Ordering::SeqCst);
        }));
        assert_eq!(got, Ok(None));
        assert_eq!(swaps.load(Ordering::SeqCst), 1);
        worker.join().expect("worker thread");
    }

    /// AC-KQF5GM.2: the time the worker waits for the session store's lock
    /// goes into its blocked-time meter. Real time, the demo simulator.
    #[test]
    fn a_store_lock_held_elsewhere_adds_to_the_blocked_meter() {
        use powerz_protocol::session::SystemClock;
        use powerz_sim::{Persona, Scenario, SimDevice};

        let store = SessionStore::new();
        let meter = BlockedMeter::new();
        let handle = spawn_metered(
            || Ok(SimDevice::new(Persona::Bulk, Scenario::Load5v1a)),
            SystemClock,
            WorkerConfig::new(Rate::Sps1000, 1),
            store.clone(),
            |_event: BackendEvent| {},
            Arc::new(AtomicBool::new(false)),
            meter.clone(),
        );
        // CI macOS runners have overslept a single sleep by up to 265 ms
        // (FR Notes, run 39747964247). Hold the lock long enough that a
        // worker waking that late still finds it held, and forgive at most
        // that much of the hold.
        const HOLD: Duration = Duration::from_millis(1000);
        const TAIL: Duration = Duration::from_millis(150);
        const MAX_OVERSLEEP: Duration = Duration::from_millis(300);

        // Let the stream settle, then measure a window with the lock free.
        std::thread::sleep(Duration::from_millis(300));
        let before = meter.total();
        std::thread::sleep(HOLD + TAIL);
        let free = meter.total().saturating_sub(before);
        assert!(store.history_len() > 0, "the worker never streamed");

        // An equal window with the lock held for HOLD of it.
        let before = meter.total();
        let started = Instant::now();
        {
            let _held = store.lock();
            std::thread::sleep(HOLD);
        }
        std::thread::sleep(TAIL);
        let held = meter.total().saturating_sub(before);
        let window = started.elapsed();
        handle.disconnect();

        assert!(
            held >= free + HOLD.saturating_sub(MAX_OVERSLEEP),
            "{HOLD:?} of a held store lock added only {:?} to the meter ({free:?} with the lock free)",
            held.saturating_sub(free)
        );
        assert!(
            held <= window + free,
            "the meter grew {held:?} in a {window:?} window"
        );
    }
}
