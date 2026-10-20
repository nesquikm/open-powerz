//! Session worker: one thread per connected meter, streaming backend events
//! to the UI (FR VHY378).

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use powerz_protocol::decode::{QueueRecord, Rate};
use powerz_protocol::session::{self, Clock, Session, Streamer};
use powerz_protocol::transport::{self, Transport, guard::GuardedTransport};
use serde::Serialize;

use crate::batch::{Batch, Batcher, Sample};
use crate::ui_error::{self, UiError};

/// Worker start-up settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkerConfig {
    /// Requested queue rate.
    pub rate: Rate,
    /// Session history this connection appends to.
    pub session_id: u64,
}

/// What the `connected` status reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConnectedInfo {
    /// Model name, e.g. `KM003C`.
    pub model: String,
    /// Firmware version, e.g. `2.0.6`.
    pub firmware_version: String,
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

/// A span of device time without samples.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Gap {
    /// Kind of gap.
    pub kind: GapKind,
    /// Last device time before the gap, ms.
    pub from_ms: u64,
    /// First device time after the gap, ms.
    pub to_ms: u64,
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
    rate: Rate,
    rate_locked: bool,
    session_id: u64,
) -> ConnectedInfo {
    ConnectedInfo {
        model: info.model.clone(),
        firmware_version: info.firmware_version.clone(),
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

/// CONNECT, read the device info, authenticate at level 1 and start the
/// queue at `config.rate`. When the meter refuses the auth or the queue
/// start, the session goes on with the rate locked to the meter's own.
fn open_session<T: Transport, C: Clock>(
    transport: T,
    clock: C,
    config: WorkerConfig,
) -> Result<(Session<GuardedTransport<T>, C>, ConnectedInfo), UiError> {
    let mut session = Session::with_clock(GuardedTransport::new(transport), clock);
    let mut run = || -> Result<(session::DeviceInfo, bool), session::Error> {
        session.connect()?;
        let info = session.device_info()?;
        let started = session
            .authenticate(1)
            .and_then(|_| session.start_stream(config.rate));
        match started {
            Ok(()) => Ok((info, false)),
            Err(e) if is_rate_refusal(&e) => Ok((info, true)),
            Err(e) => Err(e),
        }
    };
    let (info, locked) = run().map_err(|e| session_error(&e))?;
    let info = connected_info(&info, config.rate, locked, config.session_id);
    Ok((session, info))
}

/// A running session over transport `T` on clock `C`.
pub struct Worker<T: Transport, C: Clock> {
    session: Option<Session<GuardedTransport<T>, C>>,
    clock: C,
    rate: Rate,
    /// The rate cannot be changed (auth or queue start refused).
    rate_locked: bool,
    /// The locked rate as last measured from record timestamps.
    measured_sps: Option<u32>,
    /// Device time of the last record seen, for measuring across polls.
    last_record_ms: Option<u32>,
    /// Wall-clock time of the last ADC snapshot request, ms.
    last_snapshot_ms: Option<u64>,
    /// Latest chip temperature, °C; `None` when invalid or not read yet.
    temp_c: Option<f64>,
    /// Groups records into UI batches flushed at 20–60 Hz.
    batcher: Batcher,
    /// Wall-clock time the next queue poll is due, ms.
    next_poll_ms: u64,
    sink: Sink,
}

/// How often the worker reads an ADC snapshot between queue polls; well
/// under the 1 s the UI needs for a fresh temperature.
const SNAPSHOT_PERIOD_MS: u64 = 500;

impl<T: Transport, C: Clock> std::fmt::Debug for Worker<T, C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Worker")
            .field("open", &self.session.is_some())
            .field("rate", &self.rate)
            .finish_non_exhaustive()
    }
}

impl<T: Transport, C: Clock + Clone> Worker<T, C> {
    /// Connect, authenticate and start the queue.
    pub fn start(
        transport: T,
        clock: C,
        config: WorkerConfig,
        sink: impl EventSink,
    ) -> Result<Worker<T, C>, UiError> {
        let (session, info) = open_session(transport, clock.clone(), config)?;
        Ok(Worker::running(
            session,
            clock,
            config,
            info,
            Box::new(sink),
        ))
    }

    /// A worker over an open session; emits the `connected` status.
    fn running(
        session: Session<GuardedTransport<T>, C>,
        mut clock: C,
        config: WorkerConfig,
        info: ConnectedInfo,
        sink: Sink,
    ) -> Worker<T, C> {
        let now = clock.now_ms();
        let mut worker = Worker {
            session: Some(session),
            clock,
            rate: config.rate,
            rate_locked: info.rate_locked,
            measured_sps: None,
            last_record_ms: None,
            last_snapshot_ms: None,
            temp_c: None,
            batcher: Batcher::new(now),
            next_poll_ms: now,
            sink,
        };
        worker.emit(BackendEvent::Status(Status::Connected(info)));
        worker
    }

    fn emit(&mut self, event: BackendEvent) {
        (self.sink)(event);
    }

    /// Read one ADC snapshot when the last one is due, after a queue poll
    /// so requests stay strictly one at a time.
    fn snapshot_if_due(&mut self) -> Result<(), session::Error> {
        let now = self.clock.now_ms();
        if self
            .last_snapshot_ms
            .is_some_and(|t| now.saturating_sub(t) < SNAPSHOT_PERIOD_MS)
        {
            return Ok(());
        }
        let Some(session) = self.session.as_mut() else {
            return Ok(());
        };
        self.last_snapshot_ms = Some(now);
        let adc = session.read_adc()?;
        self.temp_c = adc.temp_c();
        Ok(())
    }

    /// Send pending samples out now.
    fn flush_batch(&mut self) {
        let now = self.clock.now_ms();
        if let Some(batch) = self.batcher.flush(now) {
            self.emit(BackendEvent::Batch(batch));
        }
    }

    /// Hand the records of `poll` to the batcher; a stream gap flushes the
    /// samples before it and goes out as its own event.
    fn queue_records(&mut self, poll: &session::StreamPoll) {
        let mut gaps = poll.gaps.iter().peekable();
        for r in &poll.records {
            while let Some(gap) = gaps.next_if(|g| g.to_ms <= r.time_ms()) {
                self.flush_batch();
                self.emit(BackendEvent::Gap(Gap {
                    kind: GapKind::Stream,
                    from_ms: u64::from(gap.from_ms),
                    to_ms: u64::from(gap.to_ms),
                }));
            }
            self.batcher.push(sample(r));
        }
    }

    /// Run one cycle: poll the queue when due, send a batch when due, then
    /// sleep until the next of the two.
    pub fn step(&mut self) -> Step {
        if self.session.is_none() {
            return Step::Ended;
        }
        let now = self.clock.now_ms();
        if now >= self.next_poll_ms {
            if let Step::Ended = self.poll_queue() {
                return Step::Ended;
            }
            self.next_poll_ms = now.saturating_add(
                u64::try_from(Streamer::poll_period(self.rate).as_millis()).unwrap_or(u64::MAX),
            );
        }
        self.batcher.set_temp(self.temp_c);
        let now = self.clock.now_ms();
        if let Some(batch) = self.batcher.poll(now) {
            self.emit(BackendEvent::Batch(batch));
        }
        let wake = self.next_poll_ms.min(self.batcher.next_due_ms());
        let wait = wake.saturating_sub(now).max(1);
        self.clock.sleep(Duration::from_millis(wait));
        Step::Continue
    }

    /// Poll the queue (and the snapshot when due) once.
    fn poll_queue(&mut self) -> Step {
        let Some(session) = self.session.as_mut() else {
            return Step::Ended;
        };
        let polled = session.poll_stream();
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
            Err(e) => {
                self.session = None;
                self.flush_batch();
                self.emit(disconnected(Some(session_error(&e))));
                Step::Ended
            }
        }
    }

    /// Change the queue rate.
    pub fn set_rate(&mut self, rate: Rate) -> Result<(), UiError> {
        if self.rate_locked {
            return Err(UiError::usage(
                "Changing the rate needs authentication, which the meter refused.",
            ));
        }
        let Some(session) = self.session.as_mut() else {
            return Err(UiError::usage("The meter is not connected."));
        };
        session.start_stream(rate).map_err(|e| session_error(&e))?;
        self.rate = rate;
        self.emit(BackendEvent::Status(Status::Rate {
            rate_sps: rate.sps(),
            rate_locked: false,
        }));
        Ok(())
    }

    /// Stop the queue and close the pipe.
    pub fn stop(&mut self) {
        if let Some(mut session) = self.session.take() {
            // Best effort: the pipe closes either way.
            let _ = session.stop_stream();
            drop(session);
            self.flush_batch();
            self.emit(disconnected(None));
        }
    }
}

/// Requests the UI sends to a running worker, applied between steps.
enum Command {
    SetRate(Rate, mpsc::Sender<Result<(), UiError>>),
}

/// How long a request to the worker may take before the UI gives up.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(3);

/// Sends rate changes to a running worker.
#[derive(Debug, Clone)]
pub struct RateControl {
    commands: mpsc::Sender<Command>,
}

impl RateControl {
    /// Restart the running queue at `rate` and wait for the worker's answer.
    pub fn set_rate(&self, rate: Rate) -> Result<(), UiError> {
        let gone = || UiError::usage("The meter is not connected.");
        let (reply, answer) = mpsc::channel();
        self.commands
            .send(Command::SetRate(rate, reply))
            .map_err(|_| gone())?;
        answer.recv_timeout(COMMAND_TIMEOUT).map_err(|_| gone())?
    }
}

/// Handle to a worker running on its own thread. Dropping it stops the
/// worker without waiting for it.
#[derive(Debug)]
pub struct WorkerHandle {
    stop: Arc<AtomicBool>,
    commands: mpsc::Sender<Command>,
    thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Command::SetRate(rate, _) => f.debug_tuple("SetRate").field(rate).finish(),
        }
    }
}

impl WorkerHandle {
    /// The worker thread has ended (session closed or failed).
    pub fn is_finished(&self) -> bool {
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }

    /// Restart the running queue at `rate`.
    pub fn set_rate(&self, rate: Rate) -> Result<(), UiError> {
        self.rate_control().set_rate(rate)
    }

    /// A rate changer that works without this handle, so a caller can wait
    /// for the answer without holding the lock that guards the handle.
    pub fn rate_control(&self) -> RateControl {
        RateControl {
            commands: self.commands.clone(),
        }
    }

    /// Stop the worker and wait for it to close the pipe.
    pub fn disconnect(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
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
    let flag = Arc::clone(&stop);
    let (commands, inbox) = mpsc::channel();
    // Shared so a panicking worker can still report that the session ended.
    let sink: Arc<Mutex<Sink>> = Arc::new(Mutex::new(Box::new(sink)));
    let worker_sink = Arc::clone(&sink);
    let thread = std::thread::spawn(move || {
        let ran = catch_unwind(AssertUnwindSafe(|| {
            run_worker(open, clock, config, &flag, &inbox, worker_sink)
        }));
        if let Err(payload) = ran {
            let mut sink = sink.lock().unwrap_or_else(PoisonError::into_inner);
            sink(disconnected(Some(ui_error::from_panic(payload.as_ref()))));
        }
    });
    WorkerHandle {
        stop,
        commands,
        thread: Some(thread),
    }
}

/// The worker thread's body: open, then step until stopped or ended.
fn run_worker<T, C, O>(
    open: O,
    clock: C,
    config: WorkerConfig,
    stop: &AtomicBool,
    inbox: &mpsc::Receiver<Command>,
    sink: Arc<Mutex<Sink>>,
) where
    T: Transport,
    C: Clock + Clone,
    O: FnOnce() -> Result<T, transport::Error>,
{
    let forward: Sink = Box::new(move |event| {
        let mut sink = sink.lock().unwrap_or_else(PoisonError::into_inner);
        sink(event);
    });
    let opened = open()
        .map_err(|e| ui_error::from_transport(&e, std::env::consts::OS))
        .and_then(|transport| open_session(transport, clock.clone(), config));
    let mut worker = match opened {
        Ok((session, info)) => Worker::running(session, clock, config, info, forward),
        Err(error) => {
            let mut forward = forward;
            forward(disconnected(Some(error)));
            return;
        }
    };
    'run: while !stop.load(Ordering::SeqCst) {
        loop {
            match inbox.try_recv() {
                Ok(Command::SetRate(rate, reply)) => {
                    let _ = reply.send(worker.set_rate(rate));
                }
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
