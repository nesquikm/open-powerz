//! The trigger panel's backend (FR DGSQ6S): the shared trigger engine
//! (`powerz_trigger`, FR W8SHXC) run on a thread of its own next to the
//! measurement stream. Every command above 5 V needs the user's explicit
//! confirmation, and the engine is the final gate. Closing, dropping,
//! disconnecting or quitting returns the charger to 5 V where the protocol
//! allows it and sends `pdm close`.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use powerz_protocol::pdm::{self, detect};
use powerz_protocol::session::Clock;
use powerz_trigger::{Session, TriggerError, TriggerErrorKind, set_log_sink};

pub use powerz_trigger::{
    ArmedEnd, CC_GONE_V, CableRead, CableView, Confirm, ConsoleDirection, ConsoleLine, DetectEntry,
    Detection, Engine, LimitsView, LoggedPort, MessageSop, OfferView, PdEmulation,
    PdMessageRequest, PdSendReply, ProtocolSection, RiskClass, SharedLog, TriggerProtocol,
    TriggerReply, TriggerRequest, VBUS_GONE_V, VBUS_LOW_SPELL, VBUS_RISE_WAIT, VbusReading,
    cable_view, is_cable_read, log_shared,
};

// The engine's helpers the trigger tests below check directly.
#[cfg(test)]
use powerz_protocol::pdm::message::{self, CableIdentity};
#[cfg(test)]
use powerz_trigger::{command_text, lib_message, risk_class};

use crate::ui_error::{ErrorKind, UiError};
use crate::worker::WorkerControl;

impl From<TriggerError> for UiError {
    fn from(e: TriggerError) -> UiError {
        UiError {
            kind: match e.kind {
                TriggerErrorKind::DeviceBusy => ErrorKind::DeviceBusy,
                TriggerErrorKind::Timeout => ErrorKind::Timeout,
                TriggerErrorKind::Protocol => ErrorKind::Protocol,
                TriggerErrorKind::Disconnected => ErrorKind::Disconnected,
                TriggerErrorKind::Usage => ErrorKind::Usage,
                TriggerErrorKind::OutOfSync => ErrorKind::OutOfSync,
                TriggerErrorKind::Internal => ErrorKind::Internal,
            },
            message: e.message,
            hint: e.hint,
            sessions: Vec::new(),
        }
    }
}

/// A PDM session error, in plain language.
pub fn pdm_error(err: &pdm::Error) -> UiError {
    powerz_trigger::pdm_error(err).into()
}
/// A handle to the running trigger session: every call is answered by the
/// session thread. Cheap to clone, so a command never holds the device
/// manager's lock while it waits.
#[derive(Debug, Clone)]
pub struct TriggerControl {
    tx: mpsc::Sender<Msg>,
    /// The meter's measurement stream, held while the automatic list runs.
    stream: Option<WorkerControl>,
}

/// Releases a hold on the stream's polling when dropped, so polling resumes
/// on every way out of the automatic list.
struct PollingHeld(WorkerControl);

impl PollingHeld {
    /// Hold `stream`'s polling; `None` when there is no stream. The guard
    /// exists before the hold is asked for, so its release is sent even when
    /// the hold's answer timed out: the worker takes commands in order, so
    /// a late hold is still undone, and a release with no hold is a no-op.
    fn hold(stream: Option<&WorkerControl>) -> Option<PollingHeld> {
        let held = PollingHeld(stream?.clone());
        let _ = held.0.hold_polling(true);
        Some(held)
    }
}

impl Drop for PollingHeld {
    fn drop(&mut self) {
        let _ = self.0.hold_polling(false);
    }
}

impl TriggerControl {
    /// Run `job` on the session thread and wait for its answer.
    fn call<T: Send + 'static>(
        &self,
        job: impl FnOnce(&mut dyn Engine) -> Result<T, TriggerError> + Send + 'static,
    ) -> Result<T, UiError> {
        let (reply_tx, reply_rx) = mpsc::channel();
        let job: Job = Box::new(move |engine| {
            // The port failed: the meter may be gone, so nothing stays armed.
            let result = powerz_trigger::guarded(engine, |e| job(&mut *e));
            let _ = reply_tx.send(result.map_err(UiError::from));
        });
        self.tx.send(Msg::Job(job)).map_err(|_| closed())?;
        reply_rx.recv().map_err(|_| closed())?
    }

    /// BC 1.2, Apple and the automatic list; nothing above 5 V is asked.
    /// The stream's GET_DATA polling pauses meanwhile on the open bulk
    /// session (no reconnect: a bulk CONNECT silences the PDM session
    /// `[device]`) and resumes however the list ends.
    pub fn detect(&self) -> Result<Detection, UiError> {
        let _held = PollingHeld::hold(self.stream.as_ref());
        let hint = self
            .stream
            .as_ref()
            .and_then(WorkerControl::latest_record)
            .map(|r| detect::hint_from_volts(r.dp_v(), r.dm_v()).label());
        let detection = self.call(|e| e.detect())?;
        Ok(Detection { hint, ..detection })
    }

    /// Enter `protocol` and report its section; a protocol the charger
    /// does not answer is `supported: false`, not an error.
    pub fn list(&self, protocol: TriggerProtocol) -> Result<ProtocolSection, UiError> {
        self.call(move |e| e.list(protocol))
    }

    /// The library risk class of `request` given what the charger offered.
    pub fn risk(&self, request: TriggerRequest) -> Result<RiskClass, UiError> {
        self.call(move |e| e.risk(request))
    }

    /// Send `request`. A request classified above 5 V is refused (a usage
    /// error, nothing written) unless confirmed or covered by an armed "don't
    /// ask again"; [`Confirm::YesRemember`] arms once the request succeeded
    /// and is refused for a request that may not be remembered. After a
    /// collapse every request is refused until a PD listing.
    pub fn request(
        &self,
        request: TriggerRequest,
        confirm: impl Into<Confirm>,
    ) -> Result<TriggerReply, UiError> {
        let confirm = confirm.into();
        self.call(move |e| e.request(request, confirm))
    }

    /// Whether "don't ask again" is armed. An ended session answers an error.
    pub fn armed(&self) -> Result<bool, UiError> {
        self.call(|e| Ok(e.armed()))
    }

    /// End "don't ask again"; nothing is written.
    pub fn disarm(&self) -> Result<(), UiError> {
        self.call(|e| {
            e.disarm(ArmedEnd::Cleared);
            Ok(())
        })
    }

    /// Whether VBUS collapsed under an accepted contract (FR AXHK2N).
    pub fn charger_gone(&self) -> Result<bool, UiError> {
        self.call(|e| Ok(e.charger_gone()))
    }

    /// Whether PD is the protocol the meter entered last in this session,
    /// i.e. whether [`Self::cable_read`] goes out (AC-AXHK2P.13). Nothing is
    /// written.
    pub fn pd_entered(&self) -> Result<bool, UiError> {
        self.call(|e| Ok(e.pd_entered()))
    }

    /// Back to 5 V: PD `pd req=1` (Hard Reset as the fallback), the other
    /// protocols `reset`. With the charger gone no PD way back goes out
    /// (FR JV54SQ). The session stays open. Out of sync it still
    /// works: `pd hrst` / `reset` go out blind, `pdm close` follows, and the
    /// answer is the out-of-sync error saying VBUS is not confirmed.
    pub fn back_to_5v(&self) -> Result<(), UiError> {
        self.call(|e| e.back_to_5v())
    }

    /// The trigger view was left (FR AXHK2N): "don't ask again" ends with
    /// its own reason, then back to 5 V as [`Self::back_to_5v`] does. The
    /// session stays open for the close that follows.
    pub fn leave_view(&self) -> Result<(), UiError> {
        self.call(|e| powerz_trigger::leave_view(e))
    }

    /// The library risk class of a built message (FR DGSQ6T); GoodCRC and
    /// malformed messages are a usage error with the reason.
    pub fn message_risk(&self, message: PdMessageRequest) -> Result<RiskClass, UiError> {
        self.call(move |e| e.message_risk(message))
    }

    /// Send a built message; above 5 V it is refused (nothing written)
    /// unless `confirmed`. A cable's Discover Identity ACK is decoded.
    pub fn send_message(
        &self,
        message: PdMessageRequest,
        confirmed: bool,
    ) -> Result<PdSendReply, UiError> {
        self.call(move |e| e.send_message(message, confirmed))
    }

    /// One press of Read cable: one SOP' Discover Identity inside the open PD
    /// session, no dialog, no retry. Refused with PD not entered; it never
    /// enters anything. An unanswered read is [`CableRead::NoAnswer`].
    pub fn cable_read(&self) -> Result<CableRead, UiError> {
        self.call(|e| e.cable_read())
    }

    /// The library risk class of a raw console line.
    pub fn console_risk(&self, line: String) -> Result<RiskClass, UiError> {
        self.call(move |e| e.console_risk(line))
    }

    /// Send a raw console line; above 5 V (any unparsable line too) it is
    /// refused unless `confirmed`; GoodCRC is always refused.
    pub fn console_send(&self, line: String, confirmed: bool) -> Result<Vec<ConsoleLine>, UiError> {
        // GoodCRC is refused before the line reaches the session thread.
        if let Some(refusal) = powerz_trigger::console_refusal(&line) {
            return Err(refusal.into());
        }
        self.call(move |e| e.console_send(line, confirmed))
    }
}

type Job = Box<dyn FnOnce(&mut dyn Engine) + Send>;

/// What the session thread is asked to do.
enum Msg {
    Job(Job),
    /// Read VBUS from here on.
    WatchVbus(Box<dyn FnMut() -> Option<VbusReading> + Send>),
    /// Back to 5 V, `pdm close`, answer, and end the thread.
    Close(mpsc::Sender<Result<(), UiError>>),
}

/// The handle of a session that has ended.
fn closed() -> UiError {
    UiError::usage("The trigger session is closed.")
}

/// How often VBUS is looked at.
const VBUS_CHECK: Duration = Duration::from_millis(100);
/// One count on a meter's open PDM sessions: taken before `pdm open` and let
/// go once the session has closed, so the stream worker sees the session
/// through the hand-overs on either side of the manager's slot.
#[derive(Debug)]
pub struct PdmHold(Arc<AtomicUsize>);

impl PdmHold {
    /// Take one count on `holds`.
    pub fn new(holds: &Arc<AtomicUsize>) -> PdmHold {
        holds.fetch_add(1, Ordering::SeqCst);
        PdmHold(Arc::clone(holds))
    }
}

impl Drop for PdmHold {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// The trigger session's thread. [`TriggerWorker::close`] or a drop
/// returns the charger to 5 V and sends `pdm close`.
#[derive(Debug)]
pub struct TriggerWorker {
    tx: mpsc::Sender<Msg>,
    thread: Option<JoinHandle<()>>,
    /// The meter's measurement stream, for its controls' detection.
    stream: Option<WorkerControl>,
    /// Let go after the thread has closed the session.
    hold: Option<PdmHold>,
    /// The session's log, shared with its thread.
    log: SharedLog,
}

impl std::fmt::Debug for Msg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Msg::Job(_) => "Job",
            Msg::WatchVbus(_) => "WatchVbus",
            Msg::Close(_) => "Close",
        })
    }
}

impl TriggerWorker {
    /// Open the PDM session on `port` (handshake included) on a new thread.
    pub fn spawn<P, C>(port: P, clock: C) -> Result<TriggerWorker, UiError>
    where
        P: Read + Write + Send + 'static,
        C: Clock + Send + 'static,
    {
        let (tx, rx) = mpsc::channel::<Msg>();
        let log = SharedLog::default();
        let thread_log = Arc::clone(&log);
        let (open_tx, open_rx) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("trigger".to_owned())
            .spawn(move || {
                let mut session = match Session::open_logged(port, clock, thread_log) {
                    Ok(session) => {
                        let _ = open_tx.send(Ok(()));
                        session
                    }
                    Err(e) => {
                        let _ = open_tx.send(Err(UiError::from(e)));
                        return;
                    }
                };
                // The VBUS watch goes by the wall clock here, whatever the
                // session clock (a simulated one stands still between sleeps).
                let started = Instant::now();
                session.time_watch_by(Box::new(move || {
                    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
                }));
                // Every handle gone without a close: still back to 5 V and close.
                let mut reply = None;
                // Not restarted by a job, so a busy queue cannot hold the check.
                let mut next_check = Instant::now() + VBUS_CHECK;
                loop {
                    let wait = next_check.saturating_duration_since(Instant::now());
                    match rx.recv_timeout(wait) {
                        Ok(Msg::Job(job)) => job(&mut session),
                        Ok(Msg::WatchVbus(read)) => session.watch_vbus(read),
                        Ok(Msg::Close(r)) => {
                            reply = Some(r);
                            break;
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                    if Instant::now() >= next_check {
                        session.check_vbus();
                        next_check = Instant::now() + VBUS_CHECK;
                    }
                }
                let result = session.shutdown().map_err(UiError::from);
                if let Some(r) = reply {
                    let _ = r.send(result);
                }
            })
            .map_err(|_| UiError::internal())?;
        match open_rx.recv() {
            Ok(Ok(())) => Ok(TriggerWorker {
                tx,
                thread: Some(thread),
                stream: None,
                hold: None,
                log,
            }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => {
                let _ = thread.join();
                Err(UiError::internal())
            }
        }
    }

    /// A handle for the session's calls.
    pub fn control(&self) -> TriggerControl {
        TriggerControl {
            tx: self.tx.clone(),
            stream: self.stream.clone(),
        }
    }

    /// Read VBUS with `read`; a collapse under a contract ends what stood on
    /// the charger with no press.
    pub fn watch_vbus(&self, read: impl FnMut() -> Option<VbusReading> + Send + 'static) {
        let _ = self.tx.send(Msg::WatchVbus(Box::new(read)));
    }

    /// Send the session's log lines to `log`.
    pub fn log_to(&self, log: impl FnMut(String) + Send + 'static) {
        // Here, not on the thread: the lines so far are in the log on return.
        set_log_sink(&self.log, Box::new(log));
    }

    /// Pause `stream`'s polling while the automatic list runs.
    pub(crate) fn pause_during_list(&mut self, stream: WorkerControl) {
        self.stream = Some(stream);
    }

    /// Back to 5 V where the protocol allows it, then `pdm close`; waits
    /// for the thread.
    pub fn close(mut self) -> Result<(), UiError> {
        self.finish()
    }

    /// [`TriggerWorker::close`], but wait at most `wait` for it; past that
    /// the session thread is left to finish on its own (it still returns to
    /// 5 V and sends `pdm close` if the meter answers late).
    pub fn close_within(mut self, wait: Duration) -> Result<(), UiError> {
        let Some(thread) = self.thread.take() else {
            return Ok(());
        };
        let (reply_tx, reply_rx) = mpsc::channel();
        if self.tx.send(Msg::Close(reply_tx)).is_err() {
            let _ = thread.join();
            return Err(closed());
        }
        match reply_rx.recv_timeout(wait) {
            Ok(result) => {
                let _ = thread.join();
                result
            }
            // Detached: dropping the handle does not stop the thread.
            Err(_) => Err(UiError {
                kind: crate::ui_error::ErrorKind::Timeout,
                message: "The meter did not confirm the return to 5 V in time.".to_owned(),
                hint: Some(
                    "Unplug the charger from the meter if VBUS is still above 5 V.".to_owned(),
                ),
                sessions: Vec::new(),
            }),
        }
    }

    /// Keep `hold` until this session has closed.
    pub(crate) fn hold_until_closed(&mut self, hold: PdmHold) {
        self.hold = Some(hold);
    }

    /// Ask the thread to close and wait for it; once only.
    fn finish(&mut self) -> Result<(), UiError> {
        let Some(thread) = self.thread.take() else {
            return Ok(());
        };
        let (reply_tx, reply_rx) = mpsc::channel();
        let sent = self.tx.send(Msg::Close(reply_tx)).is_ok();
        let result = if sent {
            reply_rx.recv().unwrap_or(Err(closed()))
        } else {
            Err(closed())
        };
        let _ = thread.join();
        result
    }
}

impl Drop for TriggerWorker {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

/// A test VBUS reader: `read`'s volts as a new 50 SPS record on each look,
/// timed by the wall clock, as a stream that keeps running gives them.
#[cfg(test)]
fn fresh(
    mut read: impl FnMut() -> Option<f64> + Send + 'static,
) -> impl FnMut() -> Option<VbusReading> + Send + 'static {
    let start = Instant::now();
    let mut last = 0;
    move || {
        let volts = read()?;
        let now = u32::try_from(start.elapsed().as_millis()).unwrap_or(u32::MAX);
        // Two looks in one millisecond still get two records.
        let ms = now.max(last + 1);
        last = ms;
        Some(VbusReading {
            volts,
            // A charger stays attached: its pull-up on CC1 (FR TFA5N7).
            cc1_v: 1.5,
            cc2_v: 0.0,
            time_ms: ms,
            period: Duration::from_millis(20),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use powerz_protocol::session::SystemClock;
    use powerz_sim::{SimCharger, SimClock, SimPdm};

    use super::*;
    use crate::devices::{AppOpener, DeviceManager};
    use crate::ui_error::ErrorKind;
    use crate::worker::{BackendEvent, Status};

    fn sim(charger: SimCharger) -> (SimPdm, SimClock) {
        let clock = SimClock::new(1_760_000_000_000);
        (SimPdm::new(charger, clock.clone()), clock)
    }

    fn worker(charger: SimCharger) -> (SimPdm, TriggerWorker) {
        let (sim, clock) = sim(charger);
        let w = TriggerWorker::spawn(sim.open_port(), clock).expect("the session opens");
        (sim, w)
    }

    fn pd(position: u8) -> TriggerRequest {
        TriggerRequest::Pd {
            position,
            voltage_mv: None,
            current_ma: None,
        }
    }

    /// The writes after the first `skip`, trimmed.
    fn writes_after(sim: &SimPdm, skip: usize) -> Vec<String> {
        sim.writes()
            .into_iter()
            .skip(skip)
            .map(|w| w.trim().to_owned())
            .collect()
    }

    fn last_index(writes: &[String], prefix: &str) -> Option<usize> {
        writes.iter().rposition(|w| w.starts_with(prefix))
    }

    // ---- AC-DGSQ6S.1: detection result, one section per protocol, unsupported marked ----

    #[test]
    fn there_is_one_section_per_protocol_pd_first() {
        assert_eq!(TriggerProtocol::ALL.len(), 9);
        assert_eq!(TriggerProtocol::ALL[0], TriggerProtocol::Pd);
        let names: Vec<String> = TriggerProtocol::ALL
            .iter()
            .map(|p| {
                serde_json::to_value(p)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect();
        assert_eq!(
            names,
            [
                "pd", "qc", "fcp", "afc", "sfcp", "scp", "vfcp", "tfcp", "ufcs"
            ]
        );
    }

    #[test]
    fn detection_reports_apple_and_the_automatic_list() {
        let (sim, w) = worker(SimCharger::Epr140W);
        let d = w.control().detect().expect("detect");
        assert_eq!(d.apple, "not-supported");
        let entries: Vec<(&str, &str)> = d
            .entries
            .iter()
            .map(|e| (e.name.as_str(), e.support.as_str()))
            .collect();
        assert_eq!(
            entries,
            [
                ("bc1.2", "supported"),
                ("ufcs", "not-supported"),
                ("vfcp", "not-supported")
            ]
        );
        assert_eq!(d.entries[0].vbus_mv, Some(5203));
        // Detection never leaves VBUS raised, and the session stays open.
        assert!(sim.vbus_mv() <= 5200, "VBUS {} mV", sim.vbus_mv());
        assert!(sim.pdm_active());
        w.close().expect("close");
    }

    #[test]
    fn a_protocol_the_charger_lacks_is_marked_not_supported() {
        let (_sim, w) = worker(SimCharger::Epr140W);
        let c = w.control();
        let qc = c
            .list(TriggerProtocol::Qc)
            .expect("an unsupported protocol is not an error");
        assert_eq!(qc.protocol, TriggerProtocol::Qc);
        assert!(!qc.supported);
        assert!(qc.offers.is_empty() && qc.limits.is_empty());
        let ufcs = c.list(TriggerProtocol::Ufcs).expect("list ufcs");
        assert!(!ufcs.supported);
        // The session goes on: PD still lists.
        let pd = c.list(TriggerProtocol::Pd).expect("list pd");
        assert!(pd.supported);
        w.close().expect("close");
    }

    #[test]
    fn every_protocol_of_a_multi_protocol_charger_is_supported() {
        let (_sim, w) = worker(SimCharger::MultiProtocol);
        let c = w.control();
        for p in TriggerProtocol::ALL {
            let s = c.list(p).unwrap_or_else(|e| panic!("list {p:?}: {e:?}"));
            assert_eq!(s.protocol, p);
            assert!(s.supported, "{p:?} should be supported");
        }
        let scp = c.list(TriggerProtocol::Scp).expect("scp");
        assert_eq!(scp.limits.len(), 1);
        assert_eq!(scp.limits[0].min_voltage_mv, 3400);
        assert_eq!(scp.limits[0].max_voltage_mv, 20_000);
        let ufcs = c.list(TriggerProtocol::Ufcs).expect("ufcs");
        assert_eq!(
            ufcs.limits.iter().map(|l| l.index).collect::<Vec<_>>(),
            [Some(1), Some(2)]
        );
        w.close().expect("close");
    }

    #[test]
    fn a_section_serializes_with_the_ui_field_names() {
        let (_sim, w) = worker(SimCharger::Epr140W);
        let s = w.control().list(TriggerProtocol::Pd).expect("list pd");
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["protocol"], "pd");
        assert_eq!(v["supported"], true);
        assert!(v["offers"].is_array());
        assert!(v["limits"].is_array());
        assert_eq!(v["settings"]["pd_type"], "auto");
        w.close().expect("close");
    }

    // ---- AC-DGSQ6S.2: PD offers as buttons, PPS / AVS ranges, PD emulation settings ----

    #[test]
    fn the_pd_section_lists_the_epr_offers_and_the_emulation_settings() {
        let (_sim, w) = worker(SimCharger::Epr140W);
        let s = w.control().list(TriggerProtocol::Pd).expect("list pd");
        let positions: Vec<u8> = s.offers.iter().map(|o| o.position).collect();
        assert_eq!(positions, [1, 2, 3, 4, 8, 9]);
        let nine_volt = &s.offers[1];
        assert_eq!(nine_volt.kind, "fixed");
        assert_eq!(
            (nine_volt.min_voltage_mv, nine_volt.max_voltage_mv),
            (9000, 9000)
        );
        assert_eq!(nine_volt.max_current_ma, Some(3000));
        assert!(!nine_volt.epr);
        let avs = &s.offers[5];
        assert_eq!(avs.kind, "epr-avs");
        assert_eq!((avs.min_voltage_mv, avs.max_voltage_mv), (15_000, 28_000));
        assert_eq!(avs.max_power_mw, Some(140_000));
        assert!(avs.epr);
        assert_eq!(
            s.settings,
            Some(PdEmulation {
                pd_type: "auto".into(),
                emarker: "50v-240w-epr".into(),
                sink: "pps-3a".into(),
            })
        );
        w.close().expect("close");
    }

    #[test]
    fn the_pd_section_gives_the_pps_range() {
        let (_sim, w) = worker(SimCharger::Pps65W);
        let s = w.control().list(TriggerProtocol::Pd).expect("list pd");
        let pps = s
            .offers
            .iter()
            .find(|o| o.kind == "pps")
            .expect("a PPS offer");
        assert_eq!(pps.position, 5);
        assert_eq!((pps.min_voltage_mv, pps.max_voltage_mv), (3300, 21_000));
        w.close().expect("close");
    }

    // ---- AC-DGSQ6S.3: above 5 V needs confirmation; the backend is the final gate ----

    #[test]
    fn the_risk_class_comes_from_the_library_classifier() {
        let (_sim, w) = worker(SimCharger::Epr140W);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        assert_eq!(c.risk(pd(1)).expect("risk"), RiskClass::AtMost5V);
        assert_eq!(c.risk(pd(2)).expect("risk"), RiskClass::Above5VRememberable);
        assert_eq!(c.risk(pd(8)).expect("risk"), RiskClass::Above5V);
        assert_eq!(
            serde_json::to_value(RiskClass::Above5VRememberable).unwrap(),
            "above_5v_rememberable"
        );
        assert_eq!(serde_json::to_value(RiskClass::Armed).unwrap(), "armed");
        assert_eq!(
            serde_json::to_value(RiskClass::Above5V).unwrap(),
            "above_5v"
        );
        assert_eq!(
            serde_json::to_value(RiskClass::AtMost5V).unwrap(),
            "at_most_5v"
        );
        w.close().expect("close");
    }

    #[test]
    fn an_unconfirmed_request_above_5v_is_refused_and_writes_nothing() {
        let (sim, w) = worker(SimCharger::Epr140W);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        let before = sim.writes().len();
        let vbus = sim.vbus_mv();
        let err = c.request(pd(2), false).expect_err("refused");
        assert_eq!(err.kind, ErrorKind::Usage);
        assert!(
            err.message.contains("5 V") || err.message.to_lowercase().contains("confirm"),
            "{}",
            err.message
        );
        assert_eq!(sim.writes().len(), before, "nothing was written");
        assert_eq!(sim.vbus_mv(), vbus);
        w.close().expect("close");
    }

    #[test]
    fn an_unconfirmed_legacy_request_above_5v_is_refused_too() {
        let (sim, w) = worker(SimCharger::MultiProtocol);
        let c = w.control();
        c.list(TriggerProtocol::Qc).expect("list qc");
        let before = sim.writes().len();
        let err = c
            .request(TriggerRequest::Qc { volts: 9 }, false)
            .expect_err("refused");
        assert_eq!(err.kind, ErrorKind::Usage);
        assert_eq!(sim.writes().len(), before, "nothing was written");
        w.close().expect("close");
    }

    #[test]
    fn a_request_at_5v_needs_no_confirmation() {
        let (sim, w) = worker(SimCharger::MultiProtocol);
        let c = w.control();
        c.list(TriggerProtocol::Qc).expect("list qc");
        let before = sim.writes().len();
        c.request(TriggerRequest::Qc { volts: 5 }, false)
            .expect("5 V needs no confirmation");
        assert!(writes_after(&sim, before).iter().any(|w| w == "qc 5V"));
        w.close().expect("close");
    }

    #[test]
    fn a_confirmed_pd_request_is_sent_and_answered() {
        let (sim, w) = worker(SimCharger::Epr140W);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        let before = sim.writes().len();
        let reply = c.request(pd(2), true).expect("confirmed request");
        assert!(
            writes_after(&sim, before)
                .iter()
                .any(|w| w.starts_with("pd req=2"))
        );
        assert!(reply.command.starts_with("pd req=2"), "{}", reply.command);
        assert_eq!(reply.requested_mv, Some(9000));
        assert_eq!(reply.delivered_mv, Some(9000 * 985 / 1000));
        assert!(sim.vbus_mv() > 8000);
        w.close().expect("close");
    }

    #[test]
    fn a_request_names_its_protocol_in_json() {
        let r: TriggerRequest = serde_json::from_value(serde_json::json!({
            "protocol": "pd", "position": 5, "voltage_mv": 7000, "current_ma": 1000
        }))
        .unwrap();
        assert_eq!(
            r,
            TriggerRequest::Pd {
                position: 5,
                voltage_mv: Some(7000),
                current_ma: Some(1000)
            }
        );
        let q: TriggerRequest =
            serde_json::from_value(serde_json::json!({"protocol": "qc", "volts": 9})).unwrap();
        assert_eq!(q, TriggerRequest::Qc { volts: 9 });
    }

    // ---- AC-DGSQ6S.4: the reply of a request ----

    #[test]
    fn a_confirmed_pps_request_reports_the_requested_voltage() {
        let (sim, w) = worker(SimCharger::Pps65W);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        let req = TriggerRequest::Pd {
            position: 5,
            voltage_mv: Some(7000),
            current_ma: Some(1000),
        };
        assert_eq!(c.risk(req).expect("risk"), RiskClass::Above5VRememberable);
        let reply = c.request(req, true).expect("confirmed request");
        assert_eq!(reply.requested_mv, Some(7000));
        assert_eq!(reply.requested_ma, Some(1000));
        assert!(sim.vbus_mv() > 6500);
        w.close().expect("close");
    }

    #[test]
    fn a_confirmed_legacy_request_reports_the_ready_reply() {
        let (sim, w) = worker(SimCharger::MultiProtocol);
        let c = w.control();
        c.list(TriggerProtocol::Qc).expect("list qc");
        let reply = c
            .request(TriggerRequest::Qc { volts: 9 }, true)
            .expect("confirmed request");
        assert_eq!(reply.command, "qc 9V");
        assert_eq!(reply.reply.as_deref(), Some("ready"));
        assert!(sim.vbus_mv() > 8000);
        w.close().expect("close");
    }

    // ---- AC-DGSQ6S.5: Back to 5 V; close / drop / disconnect / quit return to 5 V and close ----

    #[test]
    fn back_to_5v_after_pd_requests_position_1_and_keeps_the_session() {
        let (sim, w) = worker(SimCharger::Epr140W);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        c.request(pd(2), true).expect("9 V");
        let before = sim.writes().len();
        c.back_to_5v().expect("back to 5 V");
        let writes = writes_after(&sim, before);
        assert!(
            writes
                .iter()
                .any(|w| w.starts_with("pd req=1") || w.starts_with("pd hrst")),
            "{writes:?}"
        );
        assert!(sim.vbus_mv() <= 5200, "VBUS {} mV", sim.vbus_mv());
        assert!(sim.pdm_active(), "Back to 5 V keeps the session open");
        w.close().expect("close");
    }

    #[test]
    fn a_pd_contract_is_left_through_pd_even_after_another_protocol_was_listed() {
        // Detect lists every protocol; a PD request held before that must
        // still be ended with `pd req=1` (or a Hard Reset), not `reset` only.
        let (sim, w) = worker(SimCharger::MultiProtocol);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        c.request(pd(2), true).expect("9 V");
        c.list(TriggerProtocol::Qc).expect("list qc");
        let before = sim.writes().len();
        c.back_to_5v().expect("back to 5 V");
        let writes = writes_after(&sim, before);
        assert!(
            writes.iter().any(|w| w == "pd req=1" || w == "pd hrst"),
            "{writes:?}"
        );
        drop(w);
    }

    // ---- AC-DGSQ6S.13: a request for another protocol enters its own first ----

    /// `cmd` is written in `writes`, before `later`.
    fn written_before(writes: &[String], cmd: &str, later: &str) {
        let at = writes
            .iter()
            .position(|w| w == cmd)
            .unwrap_or_else(|| panic!("{cmd:?} not written; wrote {writes:?}"));
        let then = writes
            .iter()
            .position(|w| w.starts_with(later))
            .unwrap_or_else(|| panic!("{later:?} not written; wrote {writes:?}"));
        assert!(at < then, "{cmd:?} must come before {later:?}: {writes:?}");
    }

    #[test]
    fn a_pd_request_after_a_legacy_protocol_was_listed_enters_pd_first() {
        let (sim, w) = worker(SimCharger::MultiProtocol);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        c.list(TriggerProtocol::Qc).expect("list qc");
        let before = sim.writes().len();
        let reply = c.request(pd(2), true).expect("the PD request succeeds");
        let writes = writes_after(&sim, before);
        written_before(&writes, "entry pd", "pd req=2");
        assert_eq!(reply.requested_mv, Some(9000));
        assert!(sim.vbus_mv() > 8000, "VBUS {} mV", sim.vbus_mv());
        w.close().expect("close");
    }

    #[test]
    fn a_pd_request_after_every_protocol_was_listed_enters_pd_first() {
        // What the panel's detection does on the device (2026-10-24): every
        // protocol in order, the last legacy one left entered; the PD
        // request that followed got `req error 252`.
        let (sim, w) = worker(SimCharger::MultiProtocol);
        let c = w.control();
        for p in TriggerProtocol::ALL {
            c.list(p).unwrap_or_else(|e| panic!("list {p:?}: {e:?}"));
        }
        let before = sim.writes().len();
        c.request(pd(2), true).expect("the PD request succeeds");
        let writes = writes_after(&sim, before);
        written_before(&writes, "entry pd", "pd req=2");
        w.close().expect("close");
    }

    #[test]
    fn a_pd_request_with_pd_entered_last_does_not_enter_again() {
        let (sim, w) = worker(SimCharger::MultiProtocol);
        let c = w.control();
        c.list(TriggerProtocol::Qc).expect("list qc");
        c.list(TriggerProtocol::Pd).expect("list pd");
        let before = sim.writes().len();
        c.request(pd(2), true).expect("9 V");
        let writes = writes_after(&sim, before);
        assert!(!writes.iter().any(|w| w == "entry pd"), "{writes:?}");
        assert!(
            writes.iter().any(|w| w.starts_with("pd req=2")),
            "{writes:?}"
        );
        w.close().expect("close");
    }

    #[test]
    fn a_qc_request_after_pd_was_listed_last_enters_qc_first() {
        let (sim, w) = worker(SimCharger::MultiProtocol);
        let c = w.control();
        c.list(TriggerProtocol::Qc).expect("list qc");
        c.list(TriggerProtocol::Pd).expect("list pd");
        let before = sim.writes().len();
        let reply = c
            .request(TriggerRequest::Qc { volts: 9 }, true)
            .expect("the QC request succeeds");
        let writes = writes_after(&sim, before);
        written_before(&writes, "entry qc", "qc 9V");
        assert_eq!(reply.reply.as_deref(), Some("ready"));
        w.close().expect("close");
    }

    #[test]
    fn a_pd_message_after_a_legacy_protocol_was_listed_enters_pd_first() {
        let (sim, w) = worker(SimCharger::MultiProtocol);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        c.list(TriggerProtocol::Ufcs).expect("list ufcs");
        let before = sim.writes().len();
        let soft_reset = PdMessageRequest {
            sop: MessageSop::Sop,
            msg_type: 13,
            data_objects: vec![],
        };
        let r = c.send_message(soft_reset, false).expect("sent");
        assert_eq!(r.command, "pd cmd=13");
        written_before(&writes_after(&sim, before), "entry pd", "pd cmd=13");
        w.close().expect("close");
    }

    #[test]
    fn a_discover_identity_after_a_legacy_protocol_was_listed_enters_pd_first() {
        let clock = SimClock::new(1_760_000_000_000);
        let sim = SimPdm::new(SimCharger::MultiProtocol, clock.clone())
            .with_cable(powerz_sim::SimCable::Epr240W);
        let w = TriggerWorker::spawn(sim.open_port(), clock).expect("the session opens");
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        c.list(TriggerProtocol::Qc).expect("list qc");
        let before = sim.writes().len();
        let discover = PdMessageRequest {
            sop: MessageSop::SopPrime,
            msg_type: 15,
            data_objects: vec![0xFF00_A801],
        };
        let r = c.send_message(discover, true).expect("sent");
        written_before(&writes_after(&sim, before), "entry pd", "pd data=01");
        assert!(r.cable.is_some(), "the cable answered");
        w.close().expect("close");
    }

    #[test]
    fn a_pd_message_with_pd_entered_last_does_not_enter_again() {
        let (sim, w) = worker(SimCharger::MultiProtocol);
        let c = w.control();
        c.list(TriggerProtocol::Qc).expect("list qc");
        c.list(TriggerProtocol::Pd).expect("list pd");
        let before = sim.writes().len();
        let soft_reset = PdMessageRequest {
            sop: MessageSop::Sop,
            msg_type: 13,
            data_objects: vec![],
        };
        c.send_message(soft_reset, false).expect("sent");
        assert_eq!(writes_after(&sim, before), ["pd cmd=13"]);
        w.close().expect("close");
    }

    #[test]
    fn back_to_5v_after_a_legacy_protocol_sends_reset() {
        let (sim, w) = worker(SimCharger::MultiProtocol);
        let c = w.control();
        c.list(TriggerProtocol::Qc).expect("list qc");
        c.request(TriggerRequest::Qc { volts: 12 }, true)
            .expect("12 V");
        let before = sim.writes().len();
        c.back_to_5v().expect("back to 5 V");
        assert!(writes_after(&sim, before).iter().any(|w| w == "reset"));
        assert!(sim.vbus_mv() <= 5200, "VBUS {} mV", sim.vbus_mv());
        w.close().expect("close");
    }

    #[test]
    fn close_returns_to_5v_before_pdm_close() {
        let (sim, w) = worker(SimCharger::Epr140W);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        c.request(pd(2), true).expect("9 V");
        let before = sim.writes().len();
        w.close().expect("close");
        let writes = writes_after(&sim, before);
        let back = last_index(&writes, "pd req=1")
            .or_else(|| last_index(&writes, "pd hrst"))
            .expect("a return to 5 V");
        let close = last_index(&writes, "pdm close").expect("pdm close");
        assert!(back < close, "{writes:?}");
        assert!(!sim.pdm_active());
        // The handle is dead once the session is closed.
        assert!(c.list(TriggerProtocol::Pd).is_err());
    }

    /// The simulated meter until `pdm open` was answered; after that it
    /// hears nothing and every read costs 50 ms of real time.
    struct GoesSilent {
        port: powerz_sim::SimPdmPort,
        opened: bool,
    }

    impl Read for GoesSilent {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.opened {
                std::thread::sleep(Duration::from_millis(50));
                return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "silent"));
            }
            let n = self.port.read(buf)?;
            if buf[..n].windows(14).any(|w| w == b"pdm mode entry") {
                self.opened = true;
            }
            Ok(n)
        }
    }

    impl Write for GoesSilent {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if self.opened {
                return Ok(buf.len());
            }
            self.port.write(buf)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn close_within_gives_up_after_its_bound() {
        // Quitting must not hang on a meter that stopped answering.
        let (sim, clock) = sim(SimCharger::Epr140W);
        let port = GoesSilent {
            port: sim.open_port(),
            opened: false,
        };
        let w = TriggerWorker::spawn(port, clock).expect("the session opens");
        let start = Instant::now();
        assert!(w.close_within(Duration::from_millis(200)).is_err());
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "{:?}",
            start.elapsed()
        );
    }

    #[test]
    fn close_within_returns_to_5v_and_closes_when_the_meter_answers() {
        let (sim, w) = worker(SimCharger::Epr140W);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        c.request(pd(2), true).expect("9 V");
        w.close_within(Duration::from_secs(3)).expect("closed");
        assert!(!sim.pdm_active(), "pdm close was sent");
    }

    #[test]
    fn dropping_the_worker_returns_to_5v_and_closes() {
        let (sim, w) = worker(SimCharger::MultiProtocol);
        let c = w.control();
        c.list(TriggerProtocol::Qc).expect("list qc");
        c.request(TriggerRequest::Qc { volts: 9 }, true)
            .expect("9 V");
        let before = sim.writes().len();
        drop(w);
        let writes = writes_after(&sim, before);
        let reset = last_index(&writes, "reset").expect("reset");
        let close = last_index(&writes, "pdm close").expect("pdm close");
        assert!(reset < close, "{writes:?}");
        assert!(!sim.pdm_active());
    }

    /// Wait (real time, bounded) until `f` holds.
    fn eventually(what: &str, mut f: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !f() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    type Manager = DeviceManager<AppOpener, SystemClock>;

    /// A manager listing only the demo devices, connected to one at `rate_sps`;
    /// its events are collected.
    fn connected(rate_sps: u32) -> (Manager, Arc<Mutex<Vec<BackendEvent>>>) {
        let mut m = DeviceManager::new(|| Ok(Vec::new()), AppOpener, SystemClock);
        let events = Arc::new(Mutex::new(Vec::new()));
        let keep = Arc::clone(&events);
        m.connect_with_stop(
            "demo:load-5v-1a",
            rate_sps,
            move |e: BackendEvent| keep.lock().unwrap().push(e),
            Arc::new(AtomicBool::new(false)),
        )
        .expect("the demo device connects");
        (m, events)
    }

    #[test]
    fn disconnecting_the_meter_returns_to_5v_and_closes_the_trigger_session() {
        let (mut m, _events) = connected(50);
        let (sim, w) = worker(SimCharger::Epr140W);
        m.install_trigger(w).expect("install");
        let c = m.trigger_control().expect("a trigger session is open");
        c.list(TriggerProtocol::Pd).expect("list pd");
        c.request(pd(2), true).expect("9 V");
        m.disconnect().expect("disconnect");
        assert!(!sim.pdm_active(), "pdm close was sent");
        assert!(sim.vbus_mv() <= 5200 || !sim.pdm_active());
        let writes = writes_after(&sim, 0);
        assert!(
            last_index(&writes, "pd req=1").is_some() || last_index(&writes, "pd hrst").is_some()
        );
        assert!(m.trigger_control().is_err(), "the session is gone");
    }

    #[test]
    fn quitting_the_app_returns_to_5v_and_closes_the_trigger_session() {
        let (mut m, _events) = connected(50);
        let (sim, w) = worker(SimCharger::MultiProtocol);
        m.install_trigger(w).expect("install");
        let c = m.trigger_control().expect("open");
        c.list(TriggerProtocol::Qc).expect("list qc");
        c.request(TriggerRequest::Qc { volts: 9 }, true)
            .expect("9 V");
        m.shutdown();
        assert!(!sim.pdm_active(), "pdm close was sent");
        assert!(sim.writes().iter().any(|w| w.trim() == "reset"));
    }

    #[test]
    fn a_trigger_session_needs_a_connected_meter() {
        let mut m: Manager = DeviceManager::new(|| Ok(Vec::new()), AppOpener, SystemClock);
        let (sim, w) = worker(SimCharger::Epr140W);
        let err = m.install_trigger(w).expect_err("no meter is connected");
        assert_eq!(err.kind, ErrorKind::Usage);
        // The refused session was closed, not leaked.
        assert!(!sim.pdm_active());
        assert!(m.trigger_control().is_err());
        assert!(m.open_trigger().is_err());
    }

    #[test]
    fn the_demo_device_answers_a_cable_read() {
        let (mut m, _events) = connected(50);
        m.open_trigger().expect("the demo PDM port");
        let c = m.trigger_control().expect("open");
        c.list(TriggerProtocol::Pd).expect("list pd");
        assert!(matches!(c.cable_read().expect("read"), CableRead::Found(_)));
        m.close_trigger().expect("close");
        m.disconnect().expect("disconnect");
    }

    #[test]
    fn a_demo_session_opens_a_simulated_trigger_session() {
        let (mut m, _events) = connected(50);
        m.open_trigger()
            .expect("the demo device has a simulated PDM port");
        let s = m
            .trigger_control()
            .expect("open")
            .list(TriggerProtocol::Pd)
            .expect("list pd");
        assert!(s.supported);
        assert!(!s.offers.is_empty());
        m.close_trigger().expect("close");
        assert!(m.trigger_control().is_err());
        m.disconnect().expect("disconnect");
    }

    // ---- Notes ([inferred], vendor advice): the queue stays at 50 SPS or less while triggering ----

    #[test]
    fn the_rate_cannot_be_raised_while_a_trigger_session_is_open() {
        // The 50 SPS cap holds for the whole trigger session. [inferred]
        let (mut m, _events) = connected(50);
        let (_sim, w) = worker(SimCharger::Epr140W);
        m.install_trigger(w).expect("install");
        assert!(m.set_rate(1000).is_err(), "raised past the cap");
        assert!(m.rate_control(1000).is_err(), "raised past the cap");
        assert!(m.set_rate(10).is_ok(), "lowering is fine");
    }

    #[test]
    fn opening_a_trigger_session_caps_the_queue_at_50_sps() {
        let (mut m, events) = connected(1000);
        let (_sim, w) = worker(SimCharger::Epr140W);
        m.install_trigger(w).expect("install");
        eventually("a rate of 50 SPS or less", || {
            events.lock().unwrap().iter().any(|e| {
                matches!(e, BackendEvent::Status(Status::Rate { rate_sps, .. }) if *rate_sps <= 50)
            })
        });
        m.disconnect().expect("disconnect");
    }

    #[test]
    fn the_trigger_thread_answers_from_another_thread() {
        // The control is Send + Clone: a Tauri command takes it off the manager.
        let (_sim, w) = worker(SimCharger::Epr140W);
        let c = w.control();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(c.list(TriggerProtocol::Pd).map(|s| s.offers.len()));
        });
        let n = rx
            .recv_timeout(Duration::from_secs(10))
            .expect("answered")
            .expect("list pd");
        assert_eq!(n, 6);
        w.close().expect("close");
    }

    // ---- FR AXHK2N: "don't ask again" ----

    fn armed_session(charger: SimCharger) -> (SimPdm, TriggerWorker) {
        let (sim, w) = worker(charger);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        c.request(pd(2), Confirm::YesRemember)
            .expect("9 V, remembered");
        assert!(c.armed().expect("armed"), "the arming request arms");
        (sim, w)
    }

    fn pps(voltage_mv: u32) -> TriggerRequest {
        TriggerRequest::Pd {
            position: 5,
            voltage_mv: Some(voltage_mv),
            current_ma: Some(1000),
        }
    }

    fn avs(voltage_mv: u32) -> TriggerRequest {
        TriggerRequest::Pd {
            position: 9,
            voltage_mv: Some(voltage_mv),
            current_ma: Some(1000),
        }
    }

    /// Soft_Reset, a built control message at or below 5 V.
    fn soft_reset() -> PdMessageRequest {
        PdMessageRequest {
            sop: MessageSop::Sop,
            msg_type: 13,
            data_objects: Vec::new(),
        }
    }

    /// A built Request for position 2 (9 V): above 5 V.
    fn built_9v() -> PdMessageRequest {
        PdMessageRequest {
            sop: MessageSop::Sop,
            msg_type: 2,
            data_objects: vec![0x2304_B12C],
        }
    }

    /// `c` is disarmed: an unconfirmed 9 V is refused with nothing written.
    fn disarmed(sim: &SimPdm, c: &TriggerControl) {
        assert!(!c.armed().expect("armed"), "still armed");
        assert_eq!(c.risk(pd(2)).expect("risk"), RiskClass::Above5VRememberable);
        let before = sim.writes().len();
        let err = c.request(pd(2), false).expect_err("needs a confirmation");
        assert_eq!(err.kind, ErrorKind::Usage, "{err:?}");
        assert_eq!(writes_after(sim, before), Vec::<String>::new());
    }

    /// A handle to an ended session never reads as armed, and the next session
    /// on the same meter starts disarmed.
    fn ended_disarmed(sim: &SimPdm, old: &TriggerControl) {
        assert!(
            !old.armed().unwrap_or(false),
            "an ended session reads armed"
        );
        let (_, clock) = self::sim(SimCharger::Epr140W);
        let next = TriggerWorker::spawn(sim.open_port(), clock).expect("a new session");
        let c = next.control();
        assert!(!c.armed().expect("armed"), "a new session starts armed");
        c.list(TriggerProtocol::Pd).expect("list pd");
        disarmed(sim, &c);
        next.close().expect("close");
    }

    // ---- AC-AXHK2N.1, .6: what can arm ----

    #[test]
    fn a_session_starts_disarmed() {
        let (sim, w) = worker(SimCharger::Epr140W);
        let c = w.control();
        assert!(!c.armed().expect("armed"));
        c.list(TriggerProtocol::Pd).expect("list pd");
        disarmed(&sim, &c);
        w.close().expect("close");
    }

    #[test]
    fn confirm_maps_from_the_old_flag() {
        assert_eq!(Confirm::from(false), Confirm::No);
        assert_eq!(Confirm::from(true), Confirm::Yes);
    }

    #[test]
    fn an_offer_of_known_voltage_up_to_20v_is_rememberable() {
        let (_sim, w) = worker(SimCharger::Pps65W);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        for r in [pd(2), pd(3), pd(4), pps(9000), pps(20_000)] {
            assert_eq!(
                c.risk(r).expect("risk"),
                RiskClass::Above5VRememberable,
                "{r:?}"
            );
        }
        w.close().expect("close");
    }

    #[test]
    fn a_plain_confirmation_does_not_arm() {
        let (sim, w) = worker(SimCharger::Epr140W);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        c.request(pd(2), true).expect("9 V");
        assert!(!c.armed().expect("armed"));
        disarmed(&sim, &c);
        w.close().expect("close");
    }

    #[test]
    fn remembering_a_request_that_does_not_qualify_is_refused_with_nothing_written() {
        let (sim, w) = worker(SimCharger::Pps65W);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        let before = sim.writes().len();
        let err = c
            .request(pps(21_000), Confirm::YesRemember)
            .expect_err("above 20 V");
        assert_eq!(err.kind, ErrorKind::Usage, "{err:?}");
        assert_eq!(writes_after(&sim, before), Vec::<String>::new());
        assert!(!c.armed().expect("armed"));
        w.close().expect("close");
    }

    #[test]
    fn remembering_an_epr_request_is_refused_with_nothing_written() {
        let (sim, w) = worker(SimCharger::Epr140W);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        let before = sim.writes().len();
        for r in [pd(8), avs(18_000)] {
            let err = c.request(r, Confirm::YesRemember).expect_err("EPR");
            assert_eq!(err.kind, ErrorKind::Usage, "{err:?}");
        }
        assert_eq!(writes_after(&sim, before), Vec::<String>::new());
        assert!(!c.armed().expect("armed"));
        w.close().expect("close");
    }

    #[test]
    fn remembering_a_legacy_request_is_refused_with_nothing_written() {
        let (sim, w) = worker(SimCharger::MultiProtocol);
        let c = w.control();
        c.list(TriggerProtocol::Qc).expect("list qc");
        let before = sim.writes().len();
        let err = c
            .request(TriggerRequest::Qc { volts: 9 }, Confirm::YesRemember)
            .expect_err("not PD");
        assert_eq!(err.kind, ErrorKind::Usage, "{err:?}");
        assert_eq!(writes_after(&sim, before), Vec::<String>::new());
        assert!(!c.armed().expect("armed"));
        w.close().expect("close");
    }

    // ---- AC-AXHK2N.2: armed requests send unconfirmed ----

    #[test]
    fn armed_a_fixed_request_up_to_20v_sends_unconfirmed() {
        let (sim, w) = armed_session(SimCharger::Epr140W);
        let c = w.control();
        for (r, cmd) in [(pd(3), "pd req=3"), (pd(4), "pd req=4")] {
            assert_eq!(c.risk(r).expect("risk"), RiskClass::Armed);
            let before = sim.writes().len();
            let reply = c.request(r, false).expect("no confirmation needed");
            assert_eq!(reply.command, cmd);
            assert!(writes_after(&sim, before).iter().any(|w| w == cmd));
        }
        assert!(c.armed().expect("armed"), "still armed");
        w.close().expect("close");
    }

    #[test]
    fn armed_a_pps_request_up_to_20v_sends_unconfirmed() {
        let (sim, w) = armed_session(SimCharger::Pps65W);
        let c = w.control();
        assert_eq!(c.risk(pps(20_000)).expect("risk"), RiskClass::Armed);
        let reply = c.request(pps(20_000), false).expect("20 V");
        assert_eq!(reply.requested_mv, Some(20_000));
        assert!(sim.vbus_mv() > 19_000, "VBUS {} mV", sim.vbus_mv());
        w.close().expect("close");
    }

    #[test]
    fn a_pps_request_can_arm() {
        let (_sim, w) = worker(SimCharger::Pps65W);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        c.request(pps(12_000), Confirm::YesRemember).expect("12 V");
        assert!(c.armed().expect("armed"));
        w.close().expect("close");
    }

    #[test]
    fn listing_the_same_offers_again_stays_armed() {
        let (_sim, w) = armed_session(SimCharger::Epr140W);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        assert!(c.armed().expect("armed"));
        w.close().expect("close");
    }

    #[test]
    fn a_request_refused_before_anything_is_written_stays_armed() {
        let (sim, w) = armed_session(SimCharger::Pps65W);
        let c = w.control();
        let before = sim.writes().len();
        c.request(pps(25_000), false).expect_err("out of range");
        assert_eq!(writes_after(&sim, before), Vec::<String>::new());
        assert!(c.armed().expect("armed"));
        w.close().expect("close");
    }

    // ---- AC-AXHK2N.3: what always asks, armed or not ----

    /// Armed, `request` still asks: risk `above_5v`, refused unconfirmed with
    /// nothing written.
    fn always_asks(sim: &SimPdm, c: &TriggerControl, request: TriggerRequest) {
        assert_eq!(c.risk(request).expect("risk"), RiskClass::Above5V);
        let before = sim.writes().len();
        let err = c.request(request, false).expect_err("needs a confirmation");
        assert_eq!(err.kind, ErrorKind::Usage, "{err:?}");
        assert_eq!(writes_after(sim, before), Vec::<String>::new());
    }

    #[test]
    fn armed_a_request_above_20v_asks() {
        let (sim, w) = armed_session(SimCharger::Pps65W);
        always_asks(&sim, &w.control(), pps(20_100));
        always_asks(&sim, &w.control(), pps(21_000));
        w.close().expect("close");
    }

    #[test]
    fn armed_an_epr_fixed_request_asks() {
        let (sim, w) = armed_session(SimCharger::Epr140W);
        always_asks(&sim, &w.control(), pd(8));
        w.close().expect("close");
    }

    #[test]
    fn armed_an_epr_avs_request_asks_even_at_18v() {
        let (sim, w) = armed_session(SimCharger::Epr140W);
        always_asks(&sim, &w.control(), avs(18_000));
        w.close().expect("close");
    }

    #[test]
    fn a_confirmed_request_that_does_not_qualify_disarms() {
        let (_sim, w) = armed_session(SimCharger::MultiProtocol);
        let c = w.control();
        c.list(TriggerProtocol::Qc).expect("list qc");
        c.request(TriggerRequest::Qc { volts: 9 }, true)
            .expect("confirmed");
        assert!(!c.armed().expect("armed"));
        w.close().expect("close");
        let (_sim, w) = armed_session(SimCharger::Epr140W);
        let c = w.control();
        c.request(pd(8), true).expect("confirmed");
        assert!(!c.armed().expect("armed"));
        w.close().expect("close");
    }

    #[test]
    fn armed_a_non_pd_request_asks() {
        let (sim, w) = armed_session(SimCharger::MultiProtocol);
        let c = w.control();
        c.list(TriggerProtocol::Qc).expect("list qc");
        always_asks(&sim, &c, TriggerRequest::Qc { volts: 9 });
        c.list(TriggerProtocol::Ufcs).expect("list ufcs");
        always_asks(
            &sim,
            &c,
            TriggerRequest::Ufcs {
                index: 1,
                voltage_mv: 9000,
                current_ma: 1000,
            },
        );
        w.close().expect("close");
    }

    #[test]
    fn armed_a_raw_console_line_asks() {
        let (sim, w) = armed_session(SimCharger::Epr140W);
        let c = w.control();
        for line in ["pd req=3", "unknown text"] {
            assert_eq!(
                c.console_risk(line.to_owned()).expect("risk"),
                RiskClass::Above5V
            );
            let before = sim.writes().len();
            let err = c
                .console_send(line.to_owned(), false)
                .expect_err("needs a confirmation");
            assert_eq!(err.kind, ErrorKind::Usage, "{err:?}");
            assert_eq!(writes_after(&sim, before), Vec::<String>::new());
        }
        w.close().expect("close");
    }

    #[test]
    fn armed_a_built_message_asks() {
        let (sim, w) = armed_session(SimCharger::Epr140W);
        let c = w.control();
        assert_eq!(
            c.message_risk(built_9v()).expect("risk"),
            RiskClass::Above5V
        );
        let before = sim.writes().len();
        let err = c
            .send_message(built_9v(), false)
            .expect_err("needs a confirmation");
        assert_eq!(err.kind, ErrorKind::Usage, "{err:?}");
        assert_eq!(writes_after(&sim, before), Vec::<String>::new());
        w.close().expect("close");
    }

    // ---- AC-AXHK2N.4: the ways it ends ----

    #[test]
    fn back_to_5v_disarms() {
        let (sim, w) = armed_session(SimCharger::Epr140W);
        let c = w.control();
        c.back_to_5v().expect("back to 5 V");
        disarmed(&sim, &c);
        w.close().expect("close");
    }

    #[test]
    fn a_hard_reset_from_the_console_disarms() {
        let (sim, w) = armed_session(SimCharger::Epr140W);
        let c = w.control();
        c.console_send("pd hrst".to_owned(), false)
            .expect("at most 5 V");
        assert!(sim.writes().iter().any(|w| w.trim() == "pd hrst"));
        disarmed(&sim, &c);
        w.close().expect("close");
    }

    #[test]
    fn disconnecting_the_meter_disarms() {
        let (mut m, _events) = connected(50);
        let (sim, w) = armed_session(SimCharger::Epr140W);
        m.install_trigger(w).expect("install");
        let c = m.trigger_control().expect("open");
        m.disconnect().expect("disconnect");
        ended_disarmed(&sim, &c);
    }

    #[test]
    fn losing_the_serial_port_disarms() {
        let (sim, w) = armed_session(SimCharger::Epr140W);
        let c = w.control();
        // Reopening the tty makes the session's own handle fail.
        let _other = sim.open_port();
        c.request(pd(3), false).expect_err("the port is gone");
        assert!(!c.armed().expect("armed"), "still armed");
        drop(w);
    }

    #[test]
    fn closing_the_trigger_session_disarms() {
        let (sim, w) = armed_session(SimCharger::Epr140W);
        let c = w.control();
        w.close().expect("close");
        ended_disarmed(&sim, &c);
    }

    #[test]
    fn dropping_the_trigger_session_disarms() {
        let (sim, w) = armed_session(SimCharger::Epr140W);
        let c = w.control();
        drop(w);
        ended_disarmed(&sim, &c);
    }

    #[test]
    fn quitting_the_app_disarms() {
        let (mut m, _events) = connected(50);
        let (sim, w) = armed_session(SimCharger::Epr140W);
        m.install_trigger(w).expect("install");
        let c = m.trigger_control().expect("open");
        m.shutdown();
        ended_disarmed(&sim, &c);
    }

    // ---- AC-AXHK2N.5: Clear ----

    #[test]
    fn disarm_clears_at_once_and_writes_nothing() {
        let (sim, w) = armed_session(SimCharger::Epr140W);
        let c = w.control();
        let before = sim.writes().len();
        c.disarm().expect("disarm");
        assert_eq!(writes_after(&sim, before), Vec::<String>::new());
        disarmed(&sim, &c);
        w.close().expect("close");
    }

    #[test]
    fn disarming_a_disarmed_session_is_a_no_op() {
        let (_sim, w) = worker(SimCharger::Epr140W);
        let c = w.control();
        c.disarm().expect("disarm");
        assert!(!c.armed().expect("armed"));
        w.close().expect("close");
    }

    // ---- AC-AXHK2N.8: the other ways it ends ----

    #[test]
    fn a_pd_request_at_5v_disarms() {
        let (sim, w) = armed_session(SimCharger::Epr140W);
        let c = w.control();
        c.request(pd(1), false).expect("5 V");
        disarmed(&sim, &c);
        w.close().expect("close");
    }

    #[test]
    fn a_pps_request_at_or_below_5v_disarms() {
        let (_sim, w) = armed_session(SimCharger::Pps65W);
        let c = w.control();
        c.request(pps(5000), false).expect("5 V");
        assert!(!c.armed().expect("armed"));
        w.close().expect("close");
    }

    #[test]
    fn a_legacy_request_at_5v_disarms() {
        let (_sim, w) = armed_session(SimCharger::MultiProtocol);
        let c = w.control();
        c.list(TriggerProtocol::Qc).expect("list qc");
        c.request(TriggerRequest::Qc { volts: 5 }, false)
            .expect("5 V");
        assert!(!c.armed().expect("armed"));
        w.close().expect("close");
    }

    #[test]
    fn detect_disarms() {
        let (sim, w) = armed_session(SimCharger::Epr140W);
        let c = w.control();
        c.detect().expect("detect");
        assert!(!c.armed().expect("armed"), "still armed");
        c.list(TriggerProtocol::Pd).expect("list pd");
        disarmed(&sim, &c);
        w.close().expect("close");
    }

    #[test]
    fn a_built_message_at_or_below_5v_disarms() {
        let (sim, w) = armed_session(SimCharger::Epr140W);
        let c = w.control();
        assert_eq!(
            c.message_risk(soft_reset()).expect("risk"),
            RiskClass::AtMost5V
        );
        c.send_message(soft_reset(), false).expect("sent");
        disarmed(&sim, &c);
        w.close().expect("close");
    }

    #[test]
    fn a_confirmed_built_message_disarms() {
        let (sim, w) = armed_session(SimCharger::Epr140W);
        let c = w.control();
        c.send_message(built_9v(), true).expect("sent");
        disarmed(&sim, &c);
        w.close().expect("close");
    }

    #[test]
    fn a_console_line_at_or_below_5v_disarms() {
        let (sim, w) = armed_session(SimCharger::Epr140W);
        let c = w.control();
        c.console_send("pd cmd=13".to_owned(), false).expect("sent");
        disarmed(&sim, &c);
        w.close().expect("close");
    }

    #[test]
    fn a_confirmed_console_line_disarms() {
        let (sim, w) = armed_session(SimCharger::Epr140W);
        let c = w.control();
        c.console_send("pd req=3".to_owned(), true).expect("sent");
        disarmed(&sim, &c);
        w.close().expect("close");
    }

    #[test]
    fn a_read_only_console_line_stays_armed() {
        let (_sim, w) = armed_session(SimCharger::Epr140W);
        let c = w.control();
        assert_eq!(
            c.console_risk("pdm parm".to_owned()).expect("risk"),
            RiskClass::ReadOnly
        );
        c.console_send("pdm parm".to_owned(), false).expect("sent");
        assert!(c.armed().expect("armed"));
        w.close().expect("close");
    }

    // ---- FR AXHK2P: a read ends "don't ask again", like any built message ----

    #[test]
    fn a_cable_read_disarms() {
        let (sim, w) = armed_session(SimCharger::Epr140W);
        let c = w.control();
        assert!(matches!(c.cable_read(), Ok(CableRead::NoAnswer)));
        disarmed(&sim, &c);
        w.close().expect("close");
    }

    #[test]
    fn the_same_message_from_the_builder_disarms() {
        let (sim, w) = armed_session(SimCharger::Epr140W);
        let c = w.control();
        let read = PdMessageRequest {
            sop: MessageSop::SopPrime,
            msg_type: 15,
            data_objects: vec![0xFF00_A801],
        };
        c.send_message(read, false).expect("no dialog");
        disarmed(&sim, &c);
        w.close().expect("close");
    }

    #[test]
    fn message_and_console_risk_never_report_the_new_classes() {
        let (_sim, w) = worker(SimCharger::Epr140W);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        let check = |c: &TriggerControl| {
            assert_eq!(
                c.message_risk(built_9v()).expect("risk"),
                RiskClass::Above5V
            );
            assert_eq!(
                c.console_risk("pd req=2".to_owned()).expect("risk"),
                RiskClass::Above5V
            );
        };
        check(&c);
        c.request(pd(2), Confirm::YesRemember).expect("9 V");
        check(&c);
        w.close().expect("close");
    }

    // ---- AC-AXHK2N.9 to .13: VBUS collapses, what the log records ----

    type Vbus = Arc<Mutex<Option<f64>>>;
    type Lines = Arc<Mutex<Vec<String>>>;

    /// What the session writes when a collapse ends "don't ask again".
    const GONE_LINE: &str = "trigger armed ended: charger gone";
    /// A reading that must change nothing is given this long: three checks.
    const THREE_CHECKS: Duration = Duration::from_millis(300);
    /// Low readings that must change nothing are given this long: well past
    /// the 300 ms a collapse needs.
    const PAST_THE_SPELL: Duration = Duration::from_millis(500);

    /// A session whose VBUS watcher reads `Vbus` (9 V to begin with) and whose
    /// log lines are collected.
    fn watched(charger: SimCharger) -> (SimPdm, TriggerWorker, Vbus, Lines) {
        let (sim, w) = worker(charger);
        let vbus: Vbus = Arc::new(Mutex::new(Some(9.0)));
        let reading = Arc::clone(&vbus);
        w.watch_vbus(fresh(move || *reading.lock().unwrap()));
        let lines: Lines = Arc::new(Mutex::new(Vec::new()));
        let keep = Arc::clone(&lines);
        w.log_to(move |line: String| keep.lock().unwrap().push(line));
        (sim, w, vbus, lines)
    }

    /// [`watched`], armed by a remembered 9 V.
    fn watched_armed(charger: SimCharger) -> (SimPdm, TriggerWorker, Vbus, Lines) {
        let (sim, w, vbus, lines) = watched(charger);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        c.request(pd(2), Confirm::YesRemember)
            .expect("9 V, remembered");
        assert!(c.armed().expect("armed"), "the arming request arms");
        (sim, w, vbus, lines)
    }

    /// A session with a plain confirmed 9 V contract: accepted, not armed.
    fn watched_contract(charger: SimCharger) -> (SimPdm, TriggerWorker, Vbus, Lines) {
        let (sim, w, vbus, lines) = watched(charger);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        c.request(pd(2), true).expect("9 V");
        (sim, w, vbus, lines)
    }

    fn reads(vbus: &Vbus, volts: f64) {
        *vbus.lock().unwrap() = Some(volts);
    }

    /// How long a test that needs only the outcome waits for it: generous, like
    /// [`wait_for_line`], so a starved runner cannot fail it. The 1 s bound of
    /// AC-AXHK2N.9 is measured only by the dedicated timing tests (FR N502XX).
    const OUTCOME_WAIT: Duration = Duration::from_secs(5);

    /// Poll `armed` the way the page does until "don't ask again" ends, for up to
    /// [`OUTCOME_WAIT`].
    fn disarms_eventually(c: &TriggerControl) {
        let deadline = Instant::now() + OUTCOME_WAIT;
        while c.armed().expect("armed") {
            assert!(
                Instant::now() < deadline,
                "still armed after {OUTCOME_WAIT:?}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Wait until the collapse was noticed, for up to [`OUTCOME_WAIT`].
    fn gone_eventually(c: &TriggerControl) {
        let deadline = Instant::now() + OUTCOME_WAIT;
        while !c.charger_gone().expect("charger gone") {
            assert!(
                Instant::now() < deadline,
                "not noticed after {OUTCOME_WAIT:?}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// One look of the VBUS watcher, as a [`Looks`] reader saw it.
    #[derive(Debug, Clone, Copy)]
    struct LookRecord {
        at: Instant,
        volts: f64,
        time_ms: u32,
        period: Duration,
    }

    /// Every look the watcher made, and how many it had made when it wrote the collapse
    /// line: the line is written on the session thread right after the deciding look, so
    /// the count is exact (FR N502XX).
    #[derive(Clone, Default)]
    struct Looks {
        made: Arc<Mutex<Vec<LookRecord>>>,
        decided: Arc<Mutex<Option<usize>>>,
    }

    impl Looks {
        /// `read`, with each look it answers counted.
        fn count(
            &self,
            mut read: impl FnMut() -> Option<VbusReading> + Send + 'static,
        ) -> impl FnMut() -> Option<VbusReading> + Send + 'static {
            let made = Arc::clone(&self.made);
            move || {
                let reading = read();
                if let Some(r) = reading {
                    made.lock().unwrap().push(LookRecord {
                        at: Instant::now(),
                        volts: r.volts,
                        time_ms: r.time_ms,
                        period: r.period,
                    });
                }
                reading
            }
        }

        /// From now on `w` logs to `lines`, and the look count is noted at the collapse.
        fn note_decision(&self, w: &TriggerWorker, lines: &Lines) {
            let keep = Arc::clone(lines);
            let made = Arc::clone(&self.made);
            let decided = Arc::clone(&self.decided);
            w.log_to(move |line: String| {
                if line.starts_with(COLLAPSE_PREFIX) {
                    let count = made.lock().unwrap().len();
                    decided.lock().unwrap().get_or_insert(count);
                }
                keep.lock().unwrap().push(line);
            });
        }
    }

    /// AC-AXHK2N.9 on the watcher's own terms (FR N502XX): from the first record below
    /// 2 V, the collapse is decided within `ceil(spell / VBUS_CHECK) + 1` looks, the
    /// spell being 300 ms of device time rounded up to whole records, and that many
    /// looks at the real check interval fit in 1 s. The deciding record is 300 ms of
    /// device time after the first low one. The test thread's wall clock is only logged:
    /// on a starved runner it measures the runner, not the watcher. A look later than
    /// the stream's stop rule (its period plus 300 ms) legitimately restarts the spell;
    /// such a run is logged and only its outcome counts.
    fn decided_within_looks(looks: &Looks, what: &str) {
        let decided = looks
            .decided
            .lock()
            .unwrap()
            .expect("a collapse was decided");
        let made = looks.made.lock().unwrap();
        let first = made[..decided]
            .iter()
            .position(|l| l.volts < VBUS_GONE_V)
            .expect("a reading below 2 V was looked at");
        let deciding = made[decided - 1];
        let period = deciding.period;
        let wall = deciding.at.duration_since(made[first].at);
        let late = made[first.saturating_sub(1)..decided]
            .windows(2)
            .map(|p| p[1].at.duration_since(p[0].at))
            .max()
            .unwrap_or_default();
        let looks_taken = decided - first;
        println!(
            "{what}: decided {looks_taken} looks and {wall:?} after the first low reading (longest gap between looks {late:?})"
        );
        if late > period + VBUS_LOW_SPELL {
            println!("{what}: a look came {late:?} late, which restarts a spell; outcome only");
            return;
        }
        let period_ms = u64::try_from(period.as_millis()).unwrap_or(u64::MAX).max(1);
        let spell_ms = u64::try_from(VBUS_LOW_SPELL.as_millis()).unwrap_or(u64::MAX);
        let check_ms = u64::try_from(VBUS_CHECK.as_millis()).unwrap_or(u64::MAX);
        let spell_in_records = spell_ms.div_ceil(period_ms) * period_ms;
        let bound = spell_in_records.div_ceil(check_ms) + 1;
        assert!(
            bound * check_ms <= 1000,
            "{what}: {bound} looks at {check_ms} ms do not fit in 1 s"
        );
        assert!(
            u64::try_from(looks_taken).unwrap_or(u64::MAX) <= bound,
            "{what}: decided {looks_taken} looks after the first low reading, more than {bound}"
        );
        let span = deciding.time_ms - made[first].time_ms;
        assert!(
            u64::from(span) >= spell_ms,
            "{what}: decided {span} ms of device time after the first low reading"
        );
    }

    fn lines_with(lines: &Lines, prefix: &str) -> Vec<String> {
        let lines = lines.lock().unwrap();
        lines
            .iter()
            .filter(|l| l.starts_with(prefix))
            .cloned()
            .collect()
    }

    fn ended_lines(lines: &Lines) -> Vec<String> {
        lines_with(lines, "trigger armed ended: ")
    }

    /// `result` is the refusal after a collapse: a usage error that says to
    /// press Detect.
    fn says_press_detect<T: std::fmt::Debug>(result: Result<T, UiError>) {
        let err = result.expect_err("refused");
        assert_eq!(err.kind, ErrorKind::Usage, "{err:?}");
        assert!(err.message.contains("Detect"), "{err:?}");
    }

    // AC-AXHK2N.9: the collapse ends armed, with no press.

    #[test]
    fn the_collapse_level_is_2_v() {
        assert_eq!(VBUS_GONE_V, 2.0);
    }

    // Amended by the a5442bc hardware run: was "ends armed within 200 ms" of
    // one low reading. Now 300 ms of low readings, and within 1 s.
    #[test]
    fn a_collapse_needs_300_ms_low_and_ends_armed_within_a_second_with_no_call_into_the_session() {
        let (sim, w, _vbus, lines) = watched_armed(SimCharger::Epr140W);
        let c = w.control();
        let before = sim.writes().len();
        // The same reader as `watched`, counted: 9 V until it reads 0 V.
        let looks = Looks::default();
        looks.note_decision(&w, &lines);
        let vbus: Vbus = Arc::new(Mutex::new(Some(9.0)));
        let reading = Arc::clone(&vbus);
        w.watch_vbus(looks.count(fresh(move || *reading.lock().unwrap())));
        reads(&vbus, 0.0);
        let deadline = Instant::now() + OUTCOME_WAIT;
        // Only the log is watched: nothing here gives the session a turn.
        while ended_lines(&lines).is_empty() {
            assert!(Instant::now() < deadline, "armed did not end");
            std::thread::sleep(Duration::from_millis(2));
        }
        decided_within_looks(&looks, "0 V");
        assert!(!c.armed().expect("armed"), "still armed");
        assert_eq!(writes_after(&sim, before), Vec::<String>::new());
        w.close().expect("close");
    }

    #[test]
    fn a_collapse_ends_armed_while_the_page_keeps_asking() {
        // A call every 10 ms keeps the thread's queue busy: the check must
        // not wait for 100 quiet milliseconds.
        let (sim, w, vbus, _lines) = watched_armed(SimCharger::Epr140W);
        let c = w.control();
        let before = sim.writes().len();
        reads(&vbus, 0.0);
        disarms_eventually(&c);
        assert!(c.charger_gone().expect("charger gone"));
        assert_eq!(writes_after(&sim, before), Vec::<String>::new());
        w.close().expect("close");
    }

    #[test]
    fn just_below_2_v_is_a_collapse() {
        let (_sim, w, vbus, _lines) = watched_armed(SimCharger::Epr140W);
        let c = w.control();
        reads(&vbus, 1.99);
        disarms_eventually(&c);
        w.close().expect("close");
    }

    #[test]
    fn exactly_2_v_is_not_a_collapse() {
        let (_sim, w, vbus, _lines) = watched_armed(SimCharger::Epr140W);
        let c = w.control();
        reads(&vbus, 2.0);
        std::thread::sleep(THREE_CHECKS);
        assert!(c.armed().expect("armed"));
        assert!(!c.charger_gone().expect("charger gone"));
        w.close().expect("close");
    }

    #[test]
    fn a_dip_to_4_5v_during_a_voltage_change_does_not_disarm() {
        let (_sim, w, vbus, lines) = watched_armed(SimCharger::Epr140W);
        let c = w.control();
        reads(&vbus, 4.5);
        std::thread::sleep(THREE_CHECKS);
        assert!(c.armed().expect("armed"));
        assert!(!c.charger_gone().expect("charger gone"));
        assert_eq!(ended_lines(&lines), Vec::<String>::new());
        w.close().expect("close");
    }

    #[test]
    fn no_reading_does_nothing() {
        let (_sim, w, vbus, _lines) = watched_armed(SimCharger::Epr140W);
        let c = w.control();
        *vbus.lock().unwrap() = None;
        std::thread::sleep(THREE_CHECKS);
        assert!(c.armed().expect("armed"));
        assert!(!c.charger_gone().expect("charger gone"));
        w.close().expect("close");
    }

    #[test]
    fn zero_volts_before_any_contract_means_nothing() {
        let (sim, w, vbus, _lines) = watched(SimCharger::Epr140W);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        reads(&vbus, 0.0);
        std::thread::sleep(PAST_THE_SPELL);
        assert!(!c.charger_gone().expect("charger gone"));
        assert_eq!(
            c.risk(pd(2)).expect("the offers stay"),
            RiskClass::Above5VRememberable
        );
        disarmed(&sim, &c);
        w.close().expect("close");
    }

    #[test]
    fn detect_ends_the_contract_so_zero_volts_after_it_means_nothing() {
        // The automatic list drops VBUS while the charger is probed.
        let (_sim, w, vbus, lines) = watched_armed(SimCharger::Epr140W);
        let c = w.control();
        c.detect().expect("detect");
        reads(&vbus, 0.0);
        std::thread::sleep(PAST_THE_SPELL);
        assert!(!c.charger_gone().expect("charger gone"));
        assert!(!c.armed().expect("armed"));
        assert!(
            !ended_lines(&lines).iter().any(|l| l == GONE_LINE),
            "Detect is not a collapse"
        );
        c.list(TriggerProtocol::Pd).expect("list pd");
        assert_eq!(
            c.risk(pd(2)).expect("the offers stay"),
            RiskClass::Above5VRememberable
        );
        w.close().expect("close");
    }

    #[test]
    fn a_plain_contract_counts_too() {
        let (_sim, w, vbus, _lines) = watched_contract(SimCharger::Epr140W);
        let c = w.control();
        reads(&vbus, 0.0);
        gone_eventually(&c);
        w.close().expect("close");
    }

    #[test]
    fn the_way_back_to_5v_counts_as_a_contract() {
        let (_sim, w, vbus, _lines) = watched_contract(SimCharger::Epr140W);
        let c = w.control();
        c.back_to_5v().expect("back to 5 V");
        reads(&vbus, 0.0);
        gone_eventually(&c);
        w.close().expect("close");
    }

    #[test]
    fn a_built_message_ends_the_contract() {
        // A Soft Reset may well take VBUS down.
        let (_sim, w, vbus, _lines) = watched_contract(SimCharger::Epr140W);
        let c = w.control();
        c.send_message(soft_reset(), false).expect("sent");
        reads(&vbus, 0.0);
        std::thread::sleep(PAST_THE_SPELL);
        assert!(!c.charger_gone().expect("charger gone"));
        w.close().expect("close");
    }

    #[test]
    fn a_console_line_that_is_not_read_only_ends_the_contract() {
        let (_sim, w, vbus, _lines) = watched_contract(SimCharger::Epr140W);
        let c = w.control();
        c.console_send("pd hrst".to_owned(), false).expect("sent");
        reads(&vbus, 0.0);
        std::thread::sleep(PAST_THE_SPELL);
        assert!(!c.charger_gone().expect("charger gone"));
        w.close().expect("close");
    }

    #[test]
    fn a_read_only_console_line_leaves_the_contract() {
        let (_sim, w, vbus, _lines) = watched_contract(SimCharger::Epr140W);
        let c = w.control();
        c.console_send("pdm parm".to_owned(), false).expect("sent");
        reads(&vbus, 0.0);
        gone_eventually(&c);
        w.close().expect("close");
    }

    #[test]
    fn a_legacy_request_is_no_pd_contract() {
        let (_sim, w, vbus, _lines) = watched(SimCharger::MultiProtocol);
        let c = w.control();
        c.list(TriggerProtocol::Qc).expect("list qc");
        c.request(TriggerRequest::Qc { volts: 9 }, true)
            .expect("QC 9 V");
        reads(&vbus, 0.0);
        std::thread::sleep(PAST_THE_SPELL);
        assert!(!c.charger_gone().expect("charger gone"));
        w.close().expect("close");
    }

    // AC-AXHK2N.10: the collapse forgets the offers.

    #[test]
    fn every_request_after_a_collapse_is_refused_with_nothing_written() {
        let (sim, w, vbus, _lines) = watched_armed(SimCharger::Pps65W);
        let c = w.control();
        reads(&vbus, 0.0);
        gone_eventually(&c);
        let before = sim.writes().len();
        says_press_detect(c.request(pd(2), false));
        says_press_detect(c.request(pd(2), true));
        says_press_detect(c.request(pd(2), Confirm::YesRemember));
        says_press_detect(c.request(pd(1), false));
        says_press_detect(c.request(pps(9000), true));
        assert_eq!(writes_after(&sim, before), Vec::<String>::new());
        assert!(!c.armed().expect("armed"));
        w.close().expect("close");
    }

    #[test]
    fn a_legacy_request_after_a_collapse_is_refused_with_nothing_written() {
        let (sim, w, vbus, _lines) = watched(SimCharger::MultiProtocol);
        let c = w.control();
        c.list(TriggerProtocol::Qc).expect("list qc");
        c.list(TriggerProtocol::Pd).expect("list pd");
        c.request(pd(2), true).expect("9 V");
        reads(&vbus, 0.0);
        gone_eventually(&c);
        let before = sim.writes().len();
        says_press_detect(c.request(TriggerRequest::Qc { volts: 9 }, true));
        assert_eq!(writes_after(&sim, before), Vec::<String>::new());
        w.close().expect("close");
    }

    #[test]
    fn the_risk_of_a_request_after_a_collapse_is_refused_too() {
        // The page asks the risk first: no dialog opens on an offer now gone.
        let (sim, w, vbus, _lines) = watched_armed(SimCharger::Epr140W);
        let c = w.control();
        reads(&vbus, 0.0);
        gone_eventually(&c);
        let before = sim.writes().len();
        says_press_detect(c.risk(pd(2)));
        assert_eq!(writes_after(&sim, before), Vec::<String>::new());
        w.close().expect("close");
    }

    #[test]
    fn reading_the_state_does_not_bring_the_offers_back() {
        // The meter's capabilities still hold the old `pdo:` block.
        let (_sim, w, vbus, _lines) = watched_armed(SimCharger::Epr140W);
        let c = w.control();
        reads(&vbus, 0.0);
        gone_eventually(&c);
        for _ in 0..3 {
            assert!(!c.armed().expect("armed"));
        }
        says_press_detect(c.request(pd(2), true));
        assert!(c.charger_gone().expect("charger gone"));
        w.close().expect("close");
    }

    #[test]
    fn a_same_charger_replug_sends_nothing_and_stays_gone() {
        let (sim, w, vbus, _lines) = watched_armed(SimCharger::Epr140W);
        let c = w.control();
        reads(&vbus, 0.0);
        gone_eventually(&c);
        let before = sim.writes().len();
        reads(&vbus, 5.0);
        std::thread::sleep(THREE_CHECKS);
        assert!(
            c.charger_gone().expect("charger gone"),
            "VBUS is no listing"
        );
        says_press_detect(c.request(pd(2), false));
        assert_eq!(writes_after(&sim, before), Vec::<String>::new());
        w.close().expect("close");
    }

    #[test]
    fn a_pd_listing_of_the_same_offers_clears_it_and_arms_nothing() {
        let (sim, w, vbus, _lines) = watched_armed(SimCharger::Epr140W);
        let c = w.control();
        reads(&vbus, 0.0);
        gone_eventually(&c);
        reads(&vbus, 5.0);
        c.list(TriggerProtocol::Pd).expect("list pd");
        assert!(!c.charger_gone().expect("charger gone"));
        disarmed(&sim, &c);
        c.request(pd(2), true).expect("9 V again");
        w.close().expect("close");
    }

    // Amended (AC-AXHK2N.9): was a request right after one 0 V reading. One
    // reading is no collapse now, so the low spell lasts past 300 ms first.
    #[test]
    fn a_collapse_just_before_an_armed_request_refuses_it() {
        let (sim, w, vbus, _lines) = watched_armed(SimCharger::Epr140W);
        let c = w.control();
        let before = sim.writes().len();
        reads(&vbus, 0.0);
        // Wait for the watcher to flag the collapse instead of racing its
        // 300 ms window with a fixed sleep (a loaded runner sees it late).
        gone_eventually(&c);
        c.request(pd(2), false).expect_err("the charger went away");
        assert_eq!(writes_after(&sim, before), Vec::<String>::new());
        let _ = w.close();
    }

    #[test]
    fn a_contract_that_never_rises_is_a_collapse_after_a_second() {
        let (_sim, w, vbus, _lines) = watched(SimCharger::Epr140W);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        reads(&vbus, 0.0);
        c.request(pd(2), Confirm::YesRemember).expect("9 V");
        std::thread::sleep(THREE_CHECKS);
        assert!(c.armed().expect("armed"), "a stale sample gets its second");
        assert!(!c.charger_gone().expect("charger gone"));
        gone_eventually(&c);
        assert!(!c.armed().expect("armed"));
        let _ = w.close();
    }

    #[test]
    fn neither_detect_nor_another_protocol_clears_it() {
        let (_sim, w, vbus, _lines) = watched_armed(SimCharger::Epr140W);
        let c = w.control();
        reads(&vbus, 0.0);
        gone_eventually(&c);
        c.detect().expect("detect");
        assert!(c.charger_gone().expect("charger gone"), "after Detect");
        c.list(TriggerProtocol::Qc).expect("list qc");
        assert!(c.charger_gone().expect("charger gone"), "after QC");
        let _ = w.close();
    }

    #[test]
    fn a_new_contract_after_a_listing_is_watched_again() {
        let (_sim, w, vbus, _lines) = watched_contract(SimCharger::Epr140W);
        let c = w.control();
        reads(&vbus, 0.0);
        gone_eventually(&c);
        c.list(TriggerProtocol::Pd).expect("list pd");
        // Still 0 V, and no contract yet: the listing is not undone.
        std::thread::sleep(PAST_THE_SPELL);
        assert!(!c.charger_gone().expect("charger gone"));
        reads(&vbus, 9.0);
        c.request(pd(2), true).expect("9 V");
        std::thread::sleep(THREE_CHECKS);
        assert!(!c.charger_gone().expect("charger gone"));
        reads(&vbus, 0.0);
        gone_eventually(&c);
        w.close().expect("close");
    }

    #[test]
    fn a_collapse_with_nothing_armed_says_nothing_of_armed() {
        let (_sim, w, vbus, lines) = watched_contract(SimCharger::Epr140W);
        let c = w.control();
        reads(&vbus, 0.0);
        gone_eventually(&c);
        w.close().expect("close");
        assert_eq!(ended_lines(&lines), Vec::<String>::new());
    }

    // AC-AXHK2N.12: one line for the offers, one for the end of armed.

    #[test]
    fn the_offers_are_logged_each_time_they_are_listed() {
        let (_sim, w, _vbus, lines) = watched(SimCharger::Pps65W);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        let expected = "trigger offers: #1 fixed 5.00 V, #2 fixed 9.00 V, #3 fixed 15.00 V, \
                        #4 fixed 20.00 V, #5 pps 3.30-21.00 V";
        assert_eq!(lines_with(&lines, "trigger offers: "), vec![expected]);
        c.list(TriggerProtocol::Pd).expect("list pd");
        assert_eq!(
            lines_with(&lines, "trigger offers: "),
            vec![expected, expected]
        );
        w.close().expect("close");
    }

    #[test]
    fn listing_another_protocol_logs_no_offers() {
        let (_sim, w, _vbus, lines) = watched(SimCharger::MultiProtocol);
        let c = w.control();
        c.list(TriggerProtocol::Qc).expect("list qc");
        assert_eq!(lines_with(&lines, "trigger offers: "), Vec::<String>::new());
        w.close().expect("close");
    }

    #[test]
    fn a_collapse_logs_why_armed_ended_once() {
        let (_sim, w, vbus, lines) = watched_armed(SimCharger::Epr140W);
        let c = w.control();
        reads(&vbus, 0.0);
        disarms_eventually(&c);
        std::thread::sleep(THREE_CHECKS);
        w.close().expect("close");
        assert_eq!(ended_lines(&lines), vec![GONE_LINE]);
    }

    /// The one line `end` leaves when it ends "don't ask again".
    fn the_reason_of(end: impl FnOnce(&TriggerControl)) -> String {
        let (_sim, w, _vbus, lines) = watched_armed(SimCharger::Epr140W);
        let c = w.control();
        end(&c);
        assert!(!c.armed().expect("armed"), "still armed");
        let ended = ended_lines(&lines);
        assert_eq!(ended.len(), 1, "{ended:?}");
        // The simulator does not answer `pd req=1` after Detect's `reset`.
        let _ = w.close();
        assert_eq!(ended_lines(&lines).len(), 1, "closing logged it again");
        ended[0].clone()
    }

    #[test]
    fn every_way_armed_ends_is_logged_once_with_its_own_reason() {
        let reasons = [
            the_reason_of(|c| c.disarm().expect("clear")),
            the_reason_of(|c| c.back_to_5v().expect("back to 5 V")),
            the_reason_of(|c| {
                c.detect().expect("detect");
            }),
            the_reason_of(|c| {
                c.send_message(soft_reset(), false).expect("sent");
            }),
            the_reason_of(|c| {
                c.console_send("pd cmd=13".to_owned(), false).expect("sent");
            }),
            // AC-AXHK2N.15: leaving the tab has its own reason.
            the_reason_of(|c| c.leave_view().expect("leave the view")),
        ];
        let mut distinct = reasons.to_vec();
        distinct.sort();
        distinct.dedup();
        assert_eq!(distinct.len(), reasons.len(), "{reasons:?}");
        assert!(!reasons.iter().any(|r| r == GONE_LINE), "{reasons:?}");
    }

    #[test]
    fn a_request_that_ends_armed_is_logged_once() {
        the_reason_of(|c| {
            c.request(pd(1), false).expect("5 V");
        });
        the_reason_of(|c| {
            c.request(pd(8), true).expect("28 V, confirmed");
        });
    }

    #[test]
    fn closing_an_armed_session_is_logged_once() {
        let (_sim, w, _vbus, lines) = watched_armed(SimCharger::Epr140W);
        w.close().expect("close");
        assert_eq!(ended_lines(&lines).len(), 1, "{:?}", ended_lines(&lines));
    }

    #[test]
    fn a_request_that_stays_armed_logs_no_end() {
        let (_sim, w, _vbus, lines) = watched_armed(SimCharger::Epr140W);
        let c = w.control();
        c.request(pd(3), false).expect("15 V");
        assert!(c.armed().expect("armed"));
        assert_eq!(ended_lines(&lines), Vec::<String>::new());
        w.close().expect("close");
    }

    #[test]
    fn ending_what_was_never_armed_logs_nothing() {
        let (_sim, w, _vbus, lines) = watched_contract(SimCharger::Epr140W);
        let c = w.control();
        c.disarm().expect("clear");
        c.back_to_5v().expect("back to 5 V");
        c.detect().expect("detect");
        // The simulator does not answer `pd req=1` after Detect's `reset`.
        let _ = w.close();
        assert_eq!(ended_lines(&lines), Vec::<String>::new());
    }

    // ---- AC-AXHK2N.9 (amended), .15, .16, .17: from the a5442bc hardware run ----

    /// The prefix of the line a collapse writes, with its readings.
    const COLLAPSE_PREFIX: &str = "trigger vbus collapse: ";
    /// The prefix of the line one low reading that did not count writes.
    const DIP_PREFIX: &str = "trigger vbus dip ignored: ";
    /// The prefix of the line each message written to the meter gets.
    const SENT_PREFIX: &str = "trigger sent: ";

    /// A real unplug at 100 ms a look: a slow fall, about 1.6 s from the
    /// first fall to 1.73 V, then on down.
    const UNPLUG: [f64; 17] = [
        5.18, 4.6, 4.0, 3.4, 2.9, 2.5, 2.2, 1.95, 1.85, 1.73, 1.6, 1.45, 1.3, 1.1, 0.9, 0.7, 0.5,
    ];

    /// The device time of a script's first record, ms: far from the times
    /// [`fresh`] gives, so no script record passes for an earlier one.
    const SCRIPT_T0: u32 = 1_000_000;
    /// How far apart a script's records are: 10 SPS, one per look.
    const SCRIPT_PERIOD: Duration = Duration::from_millis(100);

    /// What one look at VBUS gets.
    #[derive(Debug, Clone, Copy)]
    enum Look {
        /// The record of this device time, with these volts. The time of the
        /// look before is that record again.
        Record(f64, u32),
        /// No reading.
        Missing,
    }

    #[derive(Default)]
    struct ScriptState {
        next: std::collections::VecDeque<Look>,
        last: Option<Look>,
        /// Once the looks run out: a stream that goes on (a new record of the
        /// last volts each look), not one that stopped (FR XR0RKS).
        goes_on: bool,
        /// When each look was handed out, and its volts.
        handed: Vec<(Instant, f64)>,
    }

    /// The VBUS watcher's looks, one per look, the last one repeated: the
    /// same record again, as a stream that stopped gives it.
    #[derive(Clone, Default)]
    struct Script(Arc<Mutex<ScriptState>>);

    impl Script {
        /// `w` reads `readings` from now on, a new record each look.
        fn watch(w: &TriggerWorker, readings: &[f64]) -> Script {
            let looks: Vec<Look> = (0u32..)
                .zip(readings)
                .map(|(i, &v)| Look::Record(v, SCRIPT_T0 + 100 * i))
                .collect();
            Script::looks(w, &looks)
        }

        /// [`Script::watch`], but the stream goes on at the last reading once
        /// `readings` run out, as a meter keeps streaming after an unplug: a
        /// look the session makes late (a loaded runner) restarts the spell on
        /// later records instead of on a stopped stream (FR XR0RKS).
        fn stream(w: &TriggerWorker, readings: &[f64]) -> Script {
            let script = Script::watch(w, readings);
            script.0.lock().unwrap().goes_on = true;
            script
        }

        /// The longest wait between two looks the session made.
        fn longest_gap(&self) -> Duration {
            let s = self.0.lock().unwrap();
            s.handed
                .windows(2)
                .map(|p| p[1].0.duration_since(p[0].0))
                .max()
                .unwrap_or_default()
        }

        /// The volts of every look handed out, in order.
        fn handed_volts(&self) -> Vec<f64> {
            self.0.lock().unwrap().handed.iter().map(|h| h.1).collect()
        }

        /// `w` gets `looks` from now on.
        fn looks(w: &TriggerWorker, looks: &[Look]) -> Script {
            let script = Script::default();
            script.0.lock().unwrap().next = looks.iter().copied().collect();
            let reader = script.clone();
            w.watch_vbus(move || reader.read());
            script
        }

        fn read(&self) -> Option<VbusReading> {
            let mut s = self.0.lock().unwrap();
            if let Some(look) = s.next.pop_front() {
                s.last = Some(look);
            } else if s.goes_on
                && let Some(Look::Record(volts, time_ms)) = s.last
            {
                s.last = Some(Look::Record(volts, time_ms + 100));
            }
            let Some(Look::Record(volts, time_ms)) = s.last else {
                return None;
            };
            s.handed.push((Instant::now(), volts));
            Some(VbusReading {
                volts,
                cc1_v: 1.5,
                cc2_v: 0.0,
                time_ms,
                period: SCRIPT_PERIOD,
            })
        }

        /// Readings not looked at yet.
        fn left(&self) -> usize {
            self.0.lock().unwrap().next.len()
        }

        /// Wait until every reading was looked at, for up to 5 s.
        fn read_out(&self) {
            let deadline = Instant::now() + Duration::from_secs(5);
            while self.left() > 0 {
                assert!(Instant::now() < deadline, "the readings were not looked at");
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }

    /// Wait for a line starting with `prefix`, for up to 5 s.
    fn wait_for_line(lines: &Lines, prefix: &str) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while lines_with(lines, prefix).is_empty() {
            assert!(
                Instant::now() < deadline,
                "no {prefix:?} line: {:?}",
                lines.lock().unwrap()
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// The text of every "sent" line, in order.
    fn sent_lines(lines: &Lines) -> Vec<String> {
        lines_with(lines, SENT_PREFIX)
            .into_iter()
            .map(|l| l[SENT_PREFIX.len()..].to_owned())
            .collect()
    }

    // AC-AXHK2N.9: a single low reading is not a collapse; an unplug is.

    #[test]
    fn one_low_reading_between_normal_ones_does_not_end_armed() {
        // As 1.77 V and 0.67 V did between 5.18 V readings on the meter.
        for dip in [1.77, 0.67] {
            let (sim, w, _vbus, lines) = watched_armed(SimCharger::Epr140W);
            let c = w.control();
            let before = sim.writes().len();
            let script = Script::watch(&w, &[5.18, dip, 5.18]);
            script.read_out();
            // Well past 300 ms after the dip.
            std::thread::sleep(Duration::from_millis(600));
            assert!(c.armed().expect("armed"), "{dip} V once ended armed");
            assert!(!c.charger_gone().expect("charger gone"), "{dip} V once");
            assert_eq!(ended_lines(&lines), Vec::<String>::new(), "{dip} V");
            assert_eq!(c.risk(pd(3)).expect("the offers stay"), RiskClass::Armed);
            assert_eq!(writes_after(&sim, before), Vec::<String>::new());
            w.close().expect("close");
        }
    }

    #[test]
    fn a_low_spell_shorter_than_300_ms_does_not_end_armed() {
        let (_sim, w, _vbus, lines) = watched_armed(SimCharger::Epr140W);
        let c = w.control();
        let script = Script::watch(&w, &[5.18, 0.5, 0.5, 5.18]);
        script.read_out();
        std::thread::sleep(Duration::from_millis(600));
        assert!(c.armed().expect("armed"), "two low looks ended armed");
        assert!(!c.charger_gone().expect("charger gone"));
        assert_eq!(ended_lines(&lines), Vec::<String>::new());
        w.close().expect("close");
    }

    #[test]
    fn one_low_reading_just_before_an_armed_request_does_not_refuse_it() {
        let (sim, w, _vbus, _lines) = watched_armed(SimCharger::Epr140W);
        let c = w.control();
        let before = sim.writes().len();
        Script::watch(&w, &[0.67, 9.0]);
        c.request(pd(3), false)
            .expect("15 V, armed: one low reading is no collapse");
        assert!(c.armed().expect("armed"));
        assert!(writes_after(&sim, before).iter().any(|w| w == "pd req=3"));
        w.close().expect("close");
    }

    #[test]
    fn an_unplug_shaped_decay_ends_armed_within_a_second_of_the_first_reading_below_2_v() {
        let (sim, w, _vbus, lines) = watched_armed(SimCharger::Epr140W);
        let c = w.control();
        let before = sim.writes().len();
        let looks = Looks::default();
        looks.note_decision(&w, &lines);
        let script = Script::stream(&w, &UNPLUG);
        w.watch_vbus(looks.count(move || script.read()));
        let deadline = Instant::now() + OUTCOME_WAIT;
        // Only the log is watched: nothing here gives the session a turn.
        while !ended_lines(&lines).iter().any(|l| l == GONE_LINE) {
            assert!(Instant::now() < deadline, "the unplug never ended armed");
            std::thread::sleep(Duration::from_millis(2));
        }
        decided_within_looks(&looks, "unplug decay");
        assert!(!c.armed().expect("armed"));
        assert!(c.charger_gone().expect("charger gone"));
        says_press_detect(c.request(pd(2), true));
        assert_eq!(writes_after(&sim, before), Vec::<String>::new());
        w.close().expect("close");
    }

    // AC-AXHK2N.9: a record counts once, and the low spell is timed by the
    // records' device times.

    /// No collapse, no end of armed, for `wait` after the script was set.
    fn stays_armed(w: &TriggerWorker, lines: &Lines, wait: Duration) {
        let c = w.control();
        std::thread::sleep(wait);
        assert!(c.armed().expect("armed"), "{:?}", lines.lock().unwrap());
        assert!(!c.charger_gone().expect("charger gone"));
        assert_eq!(lines_with(lines, COLLAPSE_PREFIX), Vec::<String>::new());
    }

    #[test]
    fn a_repeated_stale_low_record_never_collapses() {
        let (_sim, w, _vbus, lines) = watched_armed(SimCharger::Epr140W);
        // One low record, then the same record again on every look.
        Script::looks(
            &w,
            &[
                Look::Record(5.18, SCRIPT_T0),
                Look::Record(0.0, SCRIPT_T0 + 100),
            ],
        );
        stays_armed(&w, &lines, Duration::from_millis(1500));
        // The stream stopped: the one low record logs as a dip.
        assert_eq!(lines_with(&lines, DIP_PREFIX).len(), 1, "{lines:?}");
        w.close().expect("close");
    }

    #[test]
    fn one_low_a_missing_reading_and_one_low_does_not_collapse() {
        let (_sim, w, _vbus, lines) = watched_armed(SimCharger::Epr140W);
        let script = Script::looks(
            &w,
            &[
                Look::Record(5.18, SCRIPT_T0),
                Look::Record(0.5, SCRIPT_T0 + 100),
                Look::Missing,
                Look::Missing,
                Look::Missing,
                Look::Record(0.5, SCRIPT_T0 + 500),
                Look::Record(5.18, SCRIPT_T0 + 600),
            ],
        );
        script.read_out();
        stays_armed(&w, &lines, PAST_THE_SPELL);
        w.close().expect("close");
    }

    #[test]
    fn one_low_a_stopped_stream_and_one_low_does_not_collapse() {
        let (_sim, w, _vbus, lines) = watched_armed(SimCharger::Epr140W);
        let mut looks = vec![
            Look::Record(5.18, SCRIPT_T0),
            Look::Record(0.5, SCRIPT_T0 + 100),
        ];
        // About 700 ms with no new record, then one 800 ms of device time on.
        looks.extend([Look::Record(0.5, SCRIPT_T0 + 100); 7]);
        looks.extend([
            Look::Record(0.5, SCRIPT_T0 + 900),
            Look::Record(5.18, SCRIPT_T0 + 1000),
        ]);
        let script = Script::looks(&w, &looks);
        script.read_out();
        stays_armed(&w, &lines, PAST_THE_SPELL);
        w.close().expect("close");
    }

    #[test]
    fn an_unplug_at_each_trigger_rate_ends_armed_within_a_second() {
        // The trigger session streams at 50 SPS at most; 2 SPS is the slowest.
        for period_ms in [500u32, 100, 20] {
            let (_sim, w, _vbus, lines) = watched_armed(SimCharger::Epr140W);
            let looks = Looks::default();
            looks.note_decision(&w, &lines);
            let start = Instant::now();
            // A new record each period: 5.18 V, then 0.5 V on.
            w.watch_vbus(looks.count(move || {
                let ms = u32::try_from(start.elapsed().as_millis()).unwrap_or(u32::MAX);
                let n = ms / period_ms;
                let volts = if n == 0 { 5.18 } else { 0.5 };
                Some(VbusReading {
                    volts,
                    cc1_v: 1.5,
                    cc2_v: 0.0,
                    time_ms: SCRIPT_T0 + n * period_ms,
                    period: Duration::from_millis(u64::from(period_ms)),
                })
            }));
            wait_for_line(&lines, COLLAPSE_PREFIX);
            decided_within_looks(&looks, &format!("{period_ms} ms records"));
            assert!(!w.control().armed().expect("armed"));
            w.close().expect("close");
        }
    }

    #[test]
    fn an_accepted_contract_ends_the_low_spell() {
        let (_sim, w, _vbus, lines) = watched_armed(SimCharger::Epr140W);
        let c = w.control();
        let now: Arc<Mutex<VbusReading>> = Arc::new(Mutex::new(VbusReading {
            volts: 0.5,
            cc1_v: 1.5,
            cc2_v: 0.0,
            time_ms: SCRIPT_T0,
            period: SCRIPT_PERIOD,
        }));
        let reading = Arc::clone(&now);
        w.watch_vbus(move || Some(*reading.lock().unwrap()));
        let record = |volts: f64, time_ms: u32| {
            *now.lock().unwrap() = VbusReading {
                volts,
                cc1_v: 1.5,
                cc2_v: 0.0,
                time_ms,
                period: SCRIPT_PERIOD,
            };
        };
        std::thread::sleep(Duration::from_millis(250));
        c.request(pd(3), false).expect("15 V, armed");
        // 350 ms after the first low record, but the first since the contract.
        record(0.5, SCRIPT_T0 + 350);
        std::thread::sleep(Duration::from_millis(250));
        record(5.18, SCRIPT_T0 + 400);
        stays_armed(&w, &lines, THREE_CHECKS);
        w.close().expect("close");
    }

    #[test]
    fn low_records_during_the_rise_wait_do_not_carry_over() {
        let (_sim, w, vbus, _lines) = watched(SimCharger::Epr140W);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        reads(&vbus, 0.0);
        let start = Instant::now();
        c.request(pd(2), Confirm::YesRemember).expect("9 V");
        gone_eventually(&c);
        // The spell starts once the wait is over and needs its own 300 ms.
        assert!(
            start.elapsed() >= VBUS_RISE_WAIT + VBUS_LOW_SPELL,
            "gone {:?} after the contract",
            start.elapsed()
        );
        let _ = w.close();
    }

    // AC-AXHK2N.17: the readings that ended it, and a dip that did not count.

    #[test]
    fn a_collapse_logs_the_readings_that_ended_it() {
        let (_sim, w, _vbus, lines) = watched_contract(SimCharger::Epr140W);
        let c = w.control();
        let script = Script::stream(&w, &UNPLUG);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !c.charger_gone().expect("charger gone") {
            assert!(Instant::now() < deadline, "the unplug was never noticed");
            std::thread::sleep(Duration::from_millis(10));
        }
        wait_for_line(&lines, COLLAPSE_PREFIX);
        let collapse = lines_with(&lines, COLLAPSE_PREFIX);
        assert_eq!(collapse.len(), 1, "{collapse:?}");
        let logged: Vec<&str> = collapse[0][COLLAPSE_PREFIX.len()..].split(", ").collect();
        // The spell's readings: a run of looks below 2 V in the order they were
        // handed out, 300 ms of them at least (FR XR0RKS).
        let lows: Vec<String> = script
            .handed_volts()
            .into_iter()
            .filter(|&v| v < VBUS_GONE_V)
            .map(|v| format!("{v:.2} V"))
            .collect();
        assert!(logged.len() >= 4, "{logged:?}");
        assert!(
            lows.windows(logged.len())
                .any(|run| run.iter().zip(&logged).all(|(a, b)| a == b)),
            "{logged:?} is no run of the looks below 2 V {lows:?}"
        );
        // Unless a late look restarted the spell, it is the unplug's first lows
        // and no dip was logged; a late look logs the readings it cut short as
        // a dip, and those are looks below 2 V too.
        if script.longest_gap() < Duration::from_millis(350) {
            for reading in ["1.95", "1.85", "1.73"] {
                assert!(
                    collapse[0].contains(reading),
                    "{reading} missing: {}",
                    collapse[0]
                );
            }
            assert_eq!(lines_with(&lines, DIP_PREFIX), Vec::<String>::new());
        } else {
            for dip in lines_with(&lines, DIP_PREFIX) {
                for reading in dip[DIP_PREFIX.len()..].split(", ") {
                    assert!(lows.iter().any(|l| l == reading), "{dip}");
                }
            }
        }
        w.close().expect("close");
    }

    #[test]
    fn one_low_reading_that_did_not_count_logs_as_an_ignored_dip_with_its_value() {
        for (dip, text) in [(1.77, "1.77"), (0.67, "0.67")] {
            let (_sim, w, _vbus, lines) = watched_armed(SimCharger::Epr140W);
            Script::watch(&w, &[5.18, dip, 5.18]).read_out();
            wait_for_line(&lines, DIP_PREFIX);
            // Long enough for a second line or a collapse to show up.
            std::thread::sleep(Duration::from_millis(400));
            let dips = lines_with(&lines, DIP_PREFIX);
            assert_eq!(dips.len(), 1, "{dips:?}");
            assert!(dips[0].contains(text), "{text} missing: {}", dips[0]);
            assert_eq!(lines_with(&lines, COLLAPSE_PREFIX), Vec::<String>::new());
            w.close().expect("close");
        }
    }

    // AC-AXHK2N.16: one line per command, as the port took it.

    /// A port whose writes give `results` in turn, then take everything.
    #[derive(Default)]
    struct WritePort {
        results: std::collections::VecDeque<std::io::Result<usize>>,
    }

    impl Write for WritePort {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            match self.results.pop_front() {
                Some(Ok(n)) => Ok(n.min(buf.len())),
                Some(Err(e)) => Err(e),
                None => Ok(buf.len()),
            }
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// A log with a sink that keeps its lines.
    fn kept_log() -> (SharedLog, Lines) {
        let log = SharedLog::default();
        let lines: Lines = Arc::default();
        let keep = Arc::clone(&lines);
        set_log_sink(&log, Box::new(move |l| keep.lock().unwrap().push(l)));
        (log, lines)
    }

    /// Writes `command` the way the library does: until all is taken, again
    /// on `Interrupted`, then a flush.
    fn write_command<W: Write>(port: &mut W, command: &[u8]) -> std::io::Result<()> {
        let mut written = 0;
        while written < command.len() {
            match port.write(&command[written..]) {
                Ok(n) => written += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        port.flush()
    }

    fn logged_port(results: Vec<std::io::Result<usize>>) -> (LoggedPort<WritePort>, Lines) {
        let (log, lines) = kept_log();
        let port = WritePort {
            results: results.into(),
        };
        (LoggedPort::new(port, log), lines)
    }

    #[test]
    fn an_interrupted_then_ok_write_logs_once() {
        let interrupted = std::io::Error::from(std::io::ErrorKind::Interrupted);
        let (mut port, lines) = logged_port(vec![Err(interrupted)]);
        write_command(&mut port, b"pd req=2").expect("written");
        assert_eq!(*lines.lock().unwrap(), vec!["trigger sent: pd req=2"]);
    }

    #[test]
    fn a_partial_write_logs_one_line() {
        let (mut port, lines) = logged_port(vec![Ok(3), Ok(2)]);
        write_command(&mut port, b"pd req=2").expect("written");
        assert_eq!(*lines.lock().unwrap(), vec!["trigger sent: pd req=2"]);
    }

    #[test]
    fn a_failed_write_logs_the_command_marked_failed() {
        let (mut port, lines) = logged_port(vec![Ok(3), Err(std::io::Error::other("gone"))]);
        write_command(&mut port, b"pd req=2").expect_err("failed");
        assert_eq!(
            *lines.lock().unwrap(),
            vec!["trigger send failed: pd req=2"]
        );
    }

    #[test]
    fn a_console_line_holding_a_newline_and_a_fake_log_line_stays_one_escaped_line() {
        let (mut port, lines) = logged_port(Vec::new());
        write_command(
            &mut port,
            b"pd cmd=13\ntrigger armed ended: cleared\r\x1b[2J",
        )
        .expect("written");
        assert_eq!(
            *lines.lock().unwrap(),
            vec!["trigger sent: pd cmd=13\\x0atrigger armed ended: cleared\\x0d\\x1b[2J"]
        );
    }

    #[test]
    fn a_long_command_is_cut_at_256_characters_with_a_marker() {
        let (mut port, lines) = logged_port(Vec::new());
        write_command(&mut port, "x".repeat(1000).as_bytes()).expect("written");
        let lines = lines.lock().unwrap();
        let text = lines[0].strip_prefix("trigger sent: ").expect("sent");
        assert_eq!(text, format!("{} [cut]", "x".repeat(256)));
    }

    // Sink panics: one failure loses no other line, and none escapes.

    #[test]
    fn a_sink_that_panics_on_one_early_line_still_gets_the_rest() {
        let log = SharedLog::default();
        for l in ["one", "two", "three"] {
            log_shared(&log, l.to_owned());
        }
        let lines: Lines = Arc::default();
        let keep = Arc::clone(&lines);
        let set = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            set_log_sink(
                &log,
                Box::new(move |l| {
                    assert_ne!(l, "one", "the sink fails once");
                    keep.lock().unwrap().push(l);
                }),
            );
        }));
        assert!(set.is_ok(), "a sink panic escaped");
        log_shared(&log, "four".to_owned());
        assert_eq!(*lines.lock().unwrap(), vec!["two", "three", "four"]);
    }

    #[test]
    fn a_panicking_sink_lets_no_panic_out_of_a_drop() {
        let log = SharedLog::default();
        set_log_sink(&log, Box::new(|_| panic!("the sink fails")));
        let dropped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut port = LoggedPort::new(WritePort::default(), Arc::clone(&log));
            // Taken but not flushed: the drop logs it.
            port.write_all(b"pd req=1").expect("taken");
            drop(port);
        }));
        assert!(dropped.is_ok(), "a sink panic escaped a drop");
    }

    // AC-AXHK2N.15: leaving the Trigger tab says so.

    #[test]
    fn leaving_the_view_logs_left_the_trigger_view() {
        let line = the_reason_of(|c| c.leave_view().expect("leave the view"));
        assert_eq!(line, "trigger armed ended: left the trigger view");
    }

    #[test]
    fn leaving_the_view_still_returns_to_5v_and_keeps_the_session_for_its_close() {
        let (sim, w) = armed_session(SimCharger::Epr140W);
        let c = w.control();
        let before = sim.writes().len();
        c.leave_view().expect("leave the view");
        assert_eq!(writes_after(&sim, before), vec!["pd req=1".to_owned()]);
        assert!(!c.armed().expect("the session is still open"));
        w.close().expect("close");
    }

    #[test]
    fn back_to_5v_is_not_logged_as_leaving() {
        let line = the_reason_of(|c| c.back_to_5v().expect("back to 5 V"));
        assert_eq!(line, "trigger armed ended: back to 5 V");
    }

    // AC-AXHK2N.16: every message written to the meter, with its text.

    #[test]
    fn every_message_the_session_writes_is_logged_with_its_text_in_order() {
        let (sim, w, _vbus, lines) = watched(SimCharger::Epr140W);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        c.request(pd(2), true).expect("9 V");
        c.back_to_5v().expect("back to 5 V");
        let _ = c.cable_read();
        c.console_send("pdm parm".to_owned(), false)
            .expect("console");
        c.send_message(soft_reset(), false).expect("built message");
        c.detect().expect("detect");
        // The simulator does not answer `pd req=1` after Detect's `reset`.
        let _ = w.close();
        let written = writes_after(&sim, 0);
        let sent = sent_lines(&lines);
        assert_eq!(sent, written, "one line per write, in order");
        for text in [
            "pdm open",
            "entry pd",
            "pd req=2",
            "pd req=1",
            "pdm parm",
            "reset",
            "pdm close",
        ] {
            assert!(
                sent.iter().any(|s| s == text),
                "{text} not logged: {sent:?}"
            );
        }
        assert!(
            sent.iter().any(|s| s.starts_with("pd data=")),
            "the cable read is not logged: {sent:?}"
        );
    }

    #[test]
    fn a_refused_request_writes_and_logs_nothing() {
        let (sim, w, _vbus, lines) = watched(SimCharger::Epr140W);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        let logged = sent_lines(&lines).len();
        let before = sim.writes().len();
        c.request(pd(2), false).expect_err("needs a confirmation");
        assert_eq!(writes_after(&sim, before), Vec::<String>::new());
        assert_eq!(sent_lines(&lines).len(), logged);
        w.close().expect("close");
    }

    // Production wiring: `install_trigger_for` gives the session the meter's
    // log and the stream's latest reading.

    #[test]
    fn the_meters_log_gets_every_sent_line_with_no_serial_and_no_port_name() {
        let dir =
            std::env::temp_dir().join(format!("open-powerz-axhk2n-{}-sent", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create the log folder");
        let path = dir.join("app.log");

        let mut m = DeviceManager::new(|| Ok(Vec::new()), AppOpener, SystemClock);
        m.set_app_log(crate::app_log::AppLog::open(&path).expect("open app.log"));
        m.connect_with_stop(
            "demo:load-5v-1a",
            50,
            |_: BackendEvent| {},
            Arc::new(AtomicBool::new(false)),
        )
        .expect("the demo device connects");
        m.open_trigger().expect("the demo PDM port");
        let c = m.trigger_control().expect("open");
        c.list(TriggerProtocol::Pd).expect("list pd");
        c.request(pd(2), true).expect("9 V");
        c.back_to_5v().expect("back to 5 V");
        c.console_send("pdm parm".to_owned(), false)
            .expect("console");
        m.close_trigger().expect("close");
        m.disconnect().expect("disconnect");

        let log = std::fs::read_to_string(&path).expect("read app.log");
        for text in [
            "pdm open",
            "entry pd",
            "pd req=2",
            "pd req=1",
            "pdm parm",
            "pdm close",
        ] {
            let wanted = format!("{SENT_PREFIX}{text} ");
            assert!(
                log.lines().any(|l| l.contains(&wanted)),
                "no {wanted:?} line: {log}"
            );
        }
        for line in log.lines().filter(|l| l.contains("trigger ")) {
            assert!(!line.contains(powerz_sim::SIM_SERIAL), "{line}");
            for port in ["/dev/", "tty", "cu.", "COM"] {
                assert!(!line.contains(port), "a port name: {line}");
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_meters_log_gets_the_lines_with_no_serial() {
        let dir =
            std::env::temp_dir().join(format!("open-powerz-axhk2n-{}-log", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create the log folder");
        let path = dir.join("app.log");

        let mut m = DeviceManager::new(|| Ok(Vec::new()), AppOpener, SystemClock);
        m.set_app_log(crate::app_log::AppLog::open(&path).expect("open app.log"));
        m.connect_with_stop(
            "demo:load-5v-1a",
            50,
            |_: BackendEvent| {},
            Arc::new(AtomicBool::new(false)),
        )
        .expect("the demo device connects");
        m.open_trigger().expect("the demo PDM port");
        let c = m.trigger_control().expect("open");
        c.list(TriggerProtocol::Pd).expect("list pd");
        c.request(pd(2), Confirm::YesRemember).expect("9 V");
        c.disarm().expect("clear");
        m.close_trigger().expect("close");
        m.disconnect().expect("disconnect");

        let log = std::fs::read_to_string(&path).expect("read app.log");
        let of = |what: &str| log.lines().filter(|l| l.contains(what)).count();
        assert_eq!(of("trigger offers: #1 fixed 5.00 V"), 1, "{log}");
        assert_eq!(of("trigger armed ended: "), 1, "{log}");
        for line in log.lines().filter(|l| l.contains("trigger ")) {
            assert!(!line.contains(powerz_sim::SIM_SERIAL), "{line}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A demo meter's session with a PD contract at 5 V.
    fn demo_contract(id: &str) -> (Manager, TriggerControl) {
        let mut m = DeviceManager::new(|| Ok(Vec::new()), AppOpener, SystemClock);
        m.connect_with_stop(
            id,
            50,
            |_: BackendEvent| {},
            Arc::new(AtomicBool::new(false)),
        )
        .expect("the demo device connects");
        m.open_trigger().expect("the demo PDM port");
        let c = m.trigger_control().expect("open");
        c.list(TriggerProtocol::Pd).expect("list pd");
        c.request(pd(1), false).expect("5 V");
        (m, c)
    }

    #[test]
    fn the_watcher_reads_the_meters_stream() {
        // The idle demo meter reads about 0 V, so the contract never rises.
        let (mut m, c) = demo_contract("demo:idle");
        eventually("the stream's 0 V", || {
            c.charger_gone().expect("charger gone")
        });
        m.close_trigger().expect("close");
        m.disconnect().expect("disconnect");
    }

    #[test]
    fn a_meter_reading_5v_is_no_collapse() {
        let (mut m, c) = demo_contract("demo:load-5v-1a");
        std::thread::sleep(THREE_CHECKS);
        assert!(!c.charger_gone().expect("charger gone"));
        m.close_trigger().expect("close");
        m.disconnect().expect("disconnect");
    }

    // ---- FR TFA5N7: a charger unplugged with no contract is noticed ----

    /// The prefix of the line the CC watch writes when the charger is gone.
    const CC_GONE_PREFIX: &str = "trigger cc gone: ";

    /// One stream record for the CC watch.
    #[derive(Debug, Clone, Copy)]
    struct CcRecord {
        volts: f64,
        cc1: f64,
        cc2: f64,
        time_ms: u32,
    }

    /// The record at `SCRIPT_T0 + dt`, VBUS `volts`.
    fn cc(volts: f64, cc1: f64, cc2: f64, dt: u32) -> CcRecord {
        CcRecord {
            volts,
            cc1,
            cc2,
            time_ms: SCRIPT_T0 + dt,
        }
    }

    /// A charger attached with no contract: VBUS 0 V, CC1 pulled up.
    fn attached(dt: u32) -> CcRecord {
        cc(0.0, 1.6, 0.0, dt)
    }

    /// Nothing attached: VBUS 0 V, both CC lines near zero.
    fn unplugged(dt: u32) -> CcRecord {
        cc(0.0, 0.05, 0.02, dt)
    }

    #[derive(Default)]
    struct CcState {
        next: std::collections::VecDeque<CcRecord>,
        last: Option<CcRecord>,
        /// Once the records run out: a stream that goes on (a new record of
        /// the last values each look), not one that stopped (FR XR0RKS).
        goes_on: bool,
    }

    /// The watcher's records, one per look, the last one repeated (the same
    /// record again, as a stream that stopped gives it).
    #[derive(Clone, Default)]
    struct CcScript(Arc<Mutex<CcState>>);

    impl CcScript {
        fn watch(w: &TriggerWorker, records: &[CcRecord]) -> CcScript {
            let script = CcScript::default();
            script.0.lock().unwrap().next = records.iter().copied().collect();
            let reader = script.clone();
            w.watch_vbus(move || reader.read());
            script
        }

        /// [`CcScript::watch`], but the stream goes on with the last record's
        /// values once `records` run out, as a meter keeps streaming after an
        /// unplug: a look the session makes late (a loaded runner) restarts
        /// the spell on later records instead of on a stopped stream (FR
        /// XR0RKS).
        fn stream(w: &TriggerWorker, records: &[CcRecord]) -> CcScript {
            let script = CcScript::watch(w, records);
            script.0.lock().unwrap().goes_on = true;
            script
        }

        fn read(&self) -> Option<VbusReading> {
            let mut s = self.0.lock().unwrap();
            if let Some(r) = s.next.pop_front() {
                s.last = Some(r);
            } else if s.goes_on
                && let Some(r) = s.last
            {
                s.last = Some(CcRecord {
                    time_ms: r.time_ms + 100,
                    ..r
                });
            }
            s.last.map(|r| VbusReading {
                volts: r.volts,
                cc1_v: r.cc1,
                cc2_v: r.cc2,
                time_ms: r.time_ms,
                period: SCRIPT_PERIOD,
            })
        }

        /// Wait until every record was looked at, for up to 5 s.
        fn read_out(&self) {
            let deadline = Instant::now() + Duration::from_secs(5);
            while !self.0.lock().unwrap().next.is_empty() {
                assert!(Instant::now() < deadline, "records left unread");
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }

    /// Nothing marked the charger gone and no CC line was written.
    fn not_gone(c: &TriggerControl, lines: &Lines, what: &str) {
        assert!(!c.charger_gone().expect("charger gone"), "{what}");
        assert_eq!(
            lines_with(lines, CC_GONE_PREFIX),
            Vec::<String>::new(),
            "{what}"
        );
    }

    // AC-TFA5N7.1: both CC lines low for 300 ms marks the charger gone.

    #[test]
    fn the_cc_gone_level_is_0_3_v() {
        assert_eq!(CC_GONE_V, 0.3);
    }

    #[test]
    fn an_unplug_with_no_contract_marks_the_charger_gone() {
        let (sim, w, _vbus, lines) = watched(SimCharger::Epr140W);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        let before = sim.writes().len();
        let script = CcScript::stream(
            &w,
            &[
                attached(0),
                unplugged(100),
                unplugged(200),
                unplugged(300),
                unplugged(400),
                unplugged(500),
                unplugged(600),
            ],
        );
        wait_for_line(&lines, CC_GONE_PREFIX);
        assert!(c.charger_gone().expect("charger gone"));
        // The offers are forgotten and every request is refused.
        says_press_detect(c.risk(pd(2)));
        says_press_detect(c.request(pd(2), true));
        says_press_detect(c.request(pd(1), false));
        let err = c.request(pd(2), true).expect_err("refused");
        assert_eq!(err.message, "The charger went away. Press Detect.");
        assert_eq!(writes_after(&sim, before), Vec::<String>::new());
        script.read_out();
        std::thread::sleep(THREE_CHECKS);
        assert_eq!(
            lines_with(&lines, CC_GONE_PREFIX).len(),
            1,
            "one line per event: {:?}",
            lines.lock().unwrap()
        );
        w.close().expect("close");
    }

    #[test]
    fn an_unplug_under_a_contract_is_noticed_on_cc_before_vbus_falls() {
        // VBUS decays slowly after an unplug; CC drops at once.
        let (sim, w, _vbus, lines) = watched_armed(SimCharger::Epr140W);
        let c = w.control();
        let before = sim.writes().len();
        let records: Vec<CcRecord> = std::iter::once(cc(9.0, 1.6, 0.0, 0))
            .chain((1..=6).map(|i| cc(4.0, 0.05, 0.02, 100 * i)))
            .collect();
        CcScript::stream(&w, &records);
        wait_for_line(&lines, CC_GONE_PREFIX);
        assert!(c.charger_gone().expect("charger gone"));
        assert!(!c.armed().expect("armed"), "don't ask again ends");
        assert!(
            ended_lines(&lines).iter().any(|l| l == GONE_LINE),
            "{:?}",
            lines.lock().unwrap()
        );
        assert_eq!(lines_with(&lines, COLLAPSE_PREFIX), Vec::<String>::new());
        says_press_detect(c.risk(pd(3)));
        says_press_detect(c.request(pd(3), false));
        assert_eq!(writes_after(&sim, before), Vec::<String>::new());
        w.close().expect("close");
    }

    #[test]
    fn just_below_0_3_v_on_both_lines_is_gone() {
        let (_sim, w, _vbus, lines) = watched(SimCharger::Epr140W);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        let records: Vec<CcRecord> = (0..=6).map(|i| cc(0.0, 0.29, 0.29, 100 * i)).collect();
        CcScript::stream(&w, &records);
        wait_for_line(&lines, CC_GONE_PREFIX);
        assert!(c.charger_gone().expect("charger gone"));
        w.close().expect("close");
    }

    // AC-TFA5N7.2: what ends or never starts a spell.

    #[test]
    fn a_single_low_cc_record_never_counts() {
        let (_sim, w, _vbus, lines) = watched(SimCharger::Epr140W);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        let script = CcScript::watch(
            &w,
            &[
                attached(0),
                unplugged(100),
                attached(200),
                attached(300),
                attached(400),
                attached(500),
                attached(600),
            ],
        );
        script.read_out();
        std::thread::sleep(PAST_THE_SPELL);
        not_gone(&c, &lines, "one low record");
        assert_eq!(
            c.risk(pd(2)).expect("the offers stay"),
            RiskClass::Above5VRememberable
        );
        w.close().expect("close");
    }

    #[test]
    fn one_cc_line_at_0_3_v_ends_the_spell() {
        let (_sim, w, _vbus, lines) = watched(SimCharger::Epr140W);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        let script = CcScript::watch(
            &w,
            &[
                unplugged(0),
                unplugged(100),
                unplugged(200),
                // CC2 exactly at the level: the spell is over.
                cc(0.0, 0.05, 0.3, 300),
                unplugged(400),
                unplugged(500),
                attached(600),
                attached(700),
            ],
        );
        script.read_out();
        std::thread::sleep(PAST_THE_SPELL);
        not_gone(&c, &lines, "the spell was broken at 300 ms");
        w.close().expect("close");
    }

    #[test]
    fn a_charger_with_one_cc_line_pulled_up_is_never_gone() {
        // This charger keeps VBUS at 0 V with no contract; CC2 carries the pull-up.
        let (_sim, w, _vbus, lines) = watched(SimCharger::Epr140W);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        let records: Vec<CcRecord> = (0..=8).map(|i| cc(0.0, 0.0, 1.5, 100 * i)).collect();
        let script = CcScript::watch(&w, &records);
        script.read_out();
        std::thread::sleep(THREE_CHECKS);
        not_gone(&c, &lines, "CC2 pulled up");
        w.close().expect("close");
    }

    #[test]
    fn a_low_cc_spell_spanning_detect_is_ignored() {
        // The automatic list probes the charger; the stream pauses meanwhile,
        // so the first record after Detect may be one from during it.
        let (_sim, w, _vbus, lines) = watched(SimCharger::Epr140W);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        let script = CcScript::watch(&w, &[unplugged(0), unplugged(100), unplugged(200)]);
        script.read_out();
        c.detect().expect("detect");
        let script = CcScript::watch(
            &w,
            &[unplugged(350), attached(450), attached(550), attached(650)],
        );
        script.read_out();
        std::thread::sleep(PAST_THE_SPELL);
        not_gone(&c, &lines, "the low records around Detect");
        // After Detect the simulated session's way back to 5 V is not this
        // AC's concern (see neither_detect_nor_another_protocol_clears_it).
        let _ = w.close();
    }

    #[test]
    fn a_stream_that_stops_ends_the_cc_spell() {
        let (_sim, w, _vbus, lines) = watched(SimCharger::Epr140W);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        let script = CcScript::watch(&w, &[unplugged(0), unplugged(100)]);
        script.read_out();
        // The same record again and again: the stream stopped.
        std::thread::sleep(Duration::from_millis(700));
        not_gone(&c, &lines, "a stopped stream");
        let script = CcScript::watch(&w, &[unplugged(1000), attached(1100), attached(1200)]);
        script.read_out();
        std::thread::sleep(THREE_CHECKS);
        not_gone(&c, &lines, "one low record after the stream came back");
        w.close().expect("close");
    }

    // AC-TFA5N7.3: one log line per event, with the low readings.

    #[test]
    fn the_cc_gone_line_has_both_low_readings() {
        let (_sim, w, _vbus, lines) = watched(SimCharger::Epr140W);
        c_list_pd(&w);
        let records: Vec<CcRecord> = std::iter::once(attached(0))
            .chain((1..=8).map(|i| cc(0.0, 0.12, 0.04, 100 * i)))
            .collect();
        let script = CcScript::stream(&w, &records);
        script.read_out();
        // Wait for the line itself, then give a second one time to show up.
        wait_for_line(&lines, CC_GONE_PREFIX);
        std::thread::sleep(THREE_CHECKS);
        let gone = lines_with(&lines, CC_GONE_PREFIX);
        assert_eq!(gone.len(), 1, "{:?}", lines.lock().unwrap());
        let line = &gone[0];
        for wanted in ["CC1", "CC2", "0.12", "0.04"] {
            assert!(line.contains(wanted), "no {wanted:?} in {line:?}");
        }
        assert!(!line.contains(powerz_sim::SIM_SERIAL), "{line}");
        w.close().expect("close");
    }

    fn c_list_pd(w: &TriggerWorker) {
        w.control().list(TriggerProtocol::Pd).expect("list pd");
    }

    // AC-TFA5N7.4: the simulator, through the production wiring.

    #[test]
    fn a_demo_meter_with_no_charger_shows_the_charger_went_away() {
        let dir =
            std::env::temp_dir().join(format!("open-powerz-tfa5n7-{}-cc", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create the log folder");
        let path = dir.join("app.log");

        // The idle demo meter reads both CC lines at 0 V: nothing attached.
        let mut m = DeviceManager::new(|| Ok(Vec::new()), AppOpener, SystemClock);
        m.set_app_log(crate::app_log::AppLog::open(&path).expect("open app.log"));
        m.connect_with_stop(
            "demo:idle",
            50,
            |_: BackendEvent| {},
            Arc::new(AtomicBool::new(false)),
        )
        .expect("the demo device connects");
        m.open_trigger().expect("the demo PDM port");
        let c = m.trigger_control().expect("open");
        c.list(TriggerProtocol::Pd).expect("list pd");
        // No contract: only the CC lines can tell.
        eventually("the charger gone on CC", || {
            c.charger_gone().expect("charger gone")
        });
        let err = c.request(pd(2), true).expect_err("refused");
        assert_eq!(err.kind, ErrorKind::Usage, "{err:?}");
        assert_eq!(err.message, "The charger went away. Press Detect.");
        m.close_trigger().expect("close");
        m.disconnect().expect("disconnect");

        let log = std::fs::read_to_string(&path).expect("read app.log");
        let gone: Vec<&str> = log
            .lines()
            .filter(|l| l.contains("trigger cc gone:"))
            .collect();
        assert_eq!(gone.len(), 1, "{log}");
        assert!(!gone[0].contains(powerz_sim::SIM_SERIAL), "{}", gone[0]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---- FR JV54SQ: closing after the charger went away ----

    /// What quitting waits for the session to close (`TRIGGER_SHUTDOWN_WAIT`
    /// in devices.rs).
    const QUIT_WAIT: Duration = Duration::from_secs(3);
    /// "Well within" the quit wait: a close that sends only `pdm close`.
    const WELL_WITHIN: Duration = Duration::from_secs(1);

    /// The simulated meter until the charger is pulled: from then on `pd
    /// req=…` and `pd hrst` get no answer, as the real meter gives none with
    /// no charger, and each read waits 50 ms of real and meter time. Every
    /// write is kept, answered or not.
    struct ChargerLeaves {
        port: powerz_sim::SimPdmPort,
        clock: SimClock,
        pulled: Arc<AtomicBool>,
        muted: bool,
        written: Arc<Mutex<Vec<String>>>,
    }

    impl Read for ChargerLeaves {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.muted {
                std::thread::sleep(Duration::from_millis(50));
                self.clock.advance(Duration::from_millis(50));
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "no charger",
                ));
            }
            self.port.read(buf)
        }
    }

    impl Write for ChargerLeaves {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let text = String::from_utf8_lossy(buf).trim().to_owned();
            self.written.lock().unwrap().push(text.clone());
            if self.pulled.load(Ordering::SeqCst)
                && (text.starts_with("pd req") || text.starts_with("pd hrst"))
            {
                self.muted = true;
                return Ok(buf.len());
            }
            self.muted = false;
            self.port.write(buf)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            self.port.flush()
        }
    }

    /// A watched session (VBUS 9 V, log lines kept) on a [`ChargerLeaves`]
    /// port; `pulled` takes the charger away.
    struct Pullable {
        sim: SimPdm,
        w: TriggerWorker,
        vbus: Vbus,
        lines: Lines,
        pulled: Arc<AtomicBool>,
        written: Arc<Mutex<Vec<String>>>,
    }

    fn pullable(charger: SimCharger) -> Pullable {
        let (sim, clock) = sim(charger);
        let pulled = Arc::new(AtomicBool::new(false));
        let written = Arc::new(Mutex::new(Vec::new()));
        let port = ChargerLeaves {
            port: sim.open_port(),
            clock: clock.clone(),
            pulled: Arc::clone(&pulled),
            muted: false,
            written: Arc::clone(&written),
        };
        let w = TriggerWorker::spawn(port, clock).expect("the session opens");
        let vbus: Vbus = Arc::new(Mutex::new(Some(9.0)));
        let reading = Arc::clone(&vbus);
        w.watch_vbus(fresh(move || *reading.lock().unwrap()));
        let lines: Lines = Arc::new(Mutex::new(Vec::new()));
        let keep = Arc::clone(&lines);
        w.log_to(move |line: String| keep.lock().unwrap().push(line));
        Pullable {
            sim,
            w,
            vbus,
            lines,
            pulled,
            written,
        }
    }

    fn written_since(written: &Arc<Mutex<Vec<String>>>, skip: usize) -> Vec<String> {
        written.lock().unwrap().iter().skip(skip).cloned().collect()
    }

    /// No 5 V request and no Hard Reset among `writes`.
    fn no_pd_way_back(writes: &[String]) {
        assert!(
            !writes
                .iter()
                .any(|w| w.starts_with("pd req") || w.starts_with("pd hrst")),
            "a PD way back with the charger gone: {writes:?}"
        );
    }

    /// Leave the view from another thread; its answer must come within
    /// [`WELL_WITHIN`].
    fn leave_within(c: TriggerControl) {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(c.leave_view());
        });
        let answered = rx.recv_timeout(WELL_WITHIN);
        assert!(
            answered.is_ok(),
            "leaving the view did not answer within {WELL_WITHIN:?}"
        );
    }

    // AC-JV54SQ.2: gone after a PD contract: no 5 V request, no Hard Reset,
    // the close goes out well within the quit wait.

    #[test]
    fn with_the_charger_gone_after_a_pd_contract_leaving_and_closing_send_only_the_close() {
        let Pullable {
            sim,
            w,
            vbus,
            lines,
            pulled,
            written,
        } = pullable(SimCharger::Epr140W);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        c.request(pd(2), true).expect("9 V");
        reads(&vbus, 0.0);
        wait_for_line(&lines, COLLAPSE_PREFIX);
        gone_eventually(&c);
        pulled.store(true, Ordering::SeqCst);
        let before = written.lock().unwrap().len();

        leave_within(w.control());
        no_pd_way_back(&written_since(&written, before));

        let start = Instant::now();
        let closed = w.close_within(QUIT_WAIT);
        let took = start.elapsed();
        let writes = written_since(&written, before);
        no_pd_way_back(&writes);
        assert_eq!(
            writes.last().map(String::as_str),
            Some("pdm close"),
            "{writes:?}"
        );
        closed.expect("closed");
        assert!(took < WELL_WITHIN, "closing took {took:?}");
        assert!(!sim.pdm_active(), "pdm close was sent");
    }

    #[test]
    fn with_the_charger_gone_after_a_pd_contract_closing_alone_sends_only_the_close() {
        let Pullable {
            sim,
            w,
            vbus,
            lines,
            pulled,
            written,
        } = pullable(SimCharger::Epr140W);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        c.request(pd(2), true).expect("9 V");
        reads(&vbus, 0.0);
        wait_for_line(&lines, COLLAPSE_PREFIX);
        gone_eventually(&c);
        pulled.store(true, Ordering::SeqCst);
        let before = written.lock().unwrap().len();

        let start = Instant::now();
        let closed = w.close_within(QUIT_WAIT);
        let took = start.elapsed();
        let writes = written_since(&written, before);
        no_pd_way_back(&writes);
        assert_eq!(writes, vec!["pdm close".to_owned()]);
        closed.expect("closed");
        assert!(took < WELL_WITHIN, "closing took {took:?}");
        assert!(!sim.pdm_active(), "pdm close was sent");
    }

    // AC-JV54SQ.3: gone after a legacy protocol: the reset still goes out
    // before the close.

    #[test]
    fn with_the_charger_gone_after_a_legacy_protocol_closing_sends_reset_then_the_close() {
        let Pullable {
            sim,
            w,
            lines,
            pulled,
            written,
            ..
        } = pullable(SimCharger::MultiProtocol);
        let c = w.control();
        c.list(TriggerProtocol::Qc).expect("list qc");
        c.request(TriggerRequest::Qc { volts: 12 }, true)
            .expect("12 V");
        let _script = CcScript::stream(
            &w,
            &[
                attached(0),
                unplugged(100),
                unplugged(200),
                unplugged(300),
                unplugged(400),
                unplugged(500),
                unplugged(600),
            ],
        );
        wait_for_line(&lines, CC_GONE_PREFIX);
        gone_eventually(&c);
        pulled.store(true, Ordering::SeqCst);
        let before = written.lock().unwrap().len();

        let start = Instant::now();
        let closed = w.close_within(QUIT_WAIT);
        let took = start.elapsed();
        let writes = written_since(&written, before);
        no_pd_way_back(&writes);
        let reset = last_index(&writes, "reset").expect("reset");
        let close = last_index(&writes, "pdm close").expect("pdm close");
        assert!(reset < close, "{writes:?}");
        closed.expect("closed");
        assert!(took < WELL_WITHIN, "closing took {took:?}");
        assert!(!sim.pdm_active(), "pdm close was sent");
    }

    // AC-JV54SQ.4: the close is in the app log on quit after the charger went
    // away, and no PD way back is.

    #[test]
    fn quitting_after_the_charger_went_away_logs_the_close_and_no_way_back() {
        let dir =
            std::env::temp_dir().join(format!("open-powerz-jv54sq-{}-quit", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create the log folder");
        let path = dir.join("app.log");

        let mut m = DeviceManager::new(|| Ok(Vec::new()), AppOpener, SystemClock);
        m.set_app_log(crate::app_log::AppLog::open(&path).expect("open app.log"));
        // The idle demo meter reads about 0 V: the contract never rises.
        m.connect_with_stop(
            "demo:idle",
            50,
            |_: BackendEvent| {},
            Arc::new(AtomicBool::new(false)),
        )
        .expect("the demo device connects");
        m.open_trigger().expect("the demo PDM port");
        let c = m.trigger_control().expect("open");
        c.list(TriggerProtocol::Pd).expect("list pd");
        c.request(pd(1), false).expect("5 V");
        eventually("the charger gone", || {
            c.charger_gone().expect("charger gone")
        });
        drop(c);
        let start = Instant::now();
        m.shutdown();
        let took = start.elapsed();

        let log = std::fs::read_to_string(&path).expect("read app.log");
        let lines: Vec<&str> = log.lines().collect();
        let gone = lines
            .iter()
            .position(|l| l.contains(COLLAPSE_PREFIX) || l.contains(CC_GONE_PREFIX))
            .unwrap_or_else(|| panic!("no gone line: {log}"));
        let after = &lines[gone..];
        let close = format!("{SENT_PREFIX}pdm close ");
        assert!(
            after.iter().any(|l| l.contains(&close)),
            "no {close:?} line after the charger went away: {log}"
        );
        for way_back in ["pd req", "pd hrst"] {
            let sent = format!("{SENT_PREFIX}{way_back}");
            assert!(
                !after.iter().any(|l| l.contains(&sent)),
                "{sent:?} after the charger went away: {log}"
            );
        }
        assert!(took < QUIT_WAIT, "quitting took {took:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // AC-JV54SQ.5: with the charger still attached, leaving and closing are as
    // before: the way back to 5 V, then the close.

    #[test]
    fn with_the_charger_attached_leaving_and_closing_return_to_5v_then_close() {
        let (sim, w, _vbus, lines) = watched_contract(SimCharger::Epr140W);
        let c = w.control();
        std::thread::sleep(THREE_CHECKS);
        assert!(!c.charger_gone().expect("charger gone"));
        let before = sim.writes().len();
        c.leave_view().expect("leave the view");
        w.close().expect("close");
        let writes = writes_after(&sim, before);
        assert_eq!(
            writes,
            vec!["pd req=1".to_owned(), "pdm close".to_owned()],
            "{:?}",
            lines.lock().unwrap()
        );
        assert!(!sim.pdm_active());
    }

    #[test]
    fn with_the_charger_attached_closing_alone_returns_to_5v_then_closes() {
        let (sim, w, _vbus, _lines) = watched_contract(SimCharger::Epr140W);
        let c = w.control();
        std::thread::sleep(THREE_CHECKS);
        assert!(!c.charger_gone().expect("charger gone"));
        let before = sim.writes().len();
        w.close().expect("close");
        let writes = writes_after(&sim, before);
        let back = last_index(&writes, "pd req=1").expect("pd req=1");
        let close = last_index(&writes, "pdm close").expect("pdm close");
        assert!(back < close, "{writes:?}");
    }

    // ---- FR ZRJ8W6: the session close with the charger attached ----

    /// The simulated meter, answering the way back to 5 V like the real one
    /// (device run 2026-11-06): once a protocol other than PD was entered
    /// after PD, `pd req=…` gets `req error 252` and `pd hrst` gets nothing
    /// until the next write. With PD entered last, the Hard Reset's offers
    /// come only after `hrst_offers_after` of meter time. While nothing
    /// comes, each read waits 50 ms of real and meter time. Every write is
    /// kept, answered or not.
    struct LikeTheMeter {
        port: powerz_sim::SimPdmPort,
        clock: SimClock,
        pd_last: bool,
        hrst_offers_after: Duration,
        /// `pd req=1` refused with `req error 252` even with PD entered last.
        refuse_5v: Arc<AtomicBool>,
        /// Commands (by prefix) the meter takes and never answers.
        unanswered: Vec<&'static str>,
        /// Nothing is read until this meter time; `u64::MAX`: until the next
        /// write.
        quiet_until: Option<u64>,
        injected: std::collections::VecDeque<u8>,
        written: Arc<Mutex<Vec<String>>>,
    }

    impl Read for LikeTheMeter {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if !self.injected.is_empty() {
                let n = buf.len().min(self.injected.len());
                for (slot, byte) in buf.iter_mut().zip(self.injected.drain(..n)) {
                    *slot = byte;
                }
                return Ok(n);
            }
            if let Some(until) = self.quiet_until {
                if self.clock.now_ms() < until {
                    std::thread::sleep(Duration::from_millis(50));
                    self.clock.advance(Duration::from_millis(50));
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "nothing yet",
                    ));
                }
                self.quiet_until = None;
            }
            self.port.read(buf)
        }
    }

    impl Write for LikeTheMeter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let text = String::from_utf8_lossy(buf).trim().to_owned();
            self.written.lock().unwrap().push(text.clone());
            // Silence after a Hard Reset lasts until the next write.
            if self.quiet_until == Some(u64::MAX) {
                self.quiet_until = None;
            }
            if self.unanswered.iter().any(|p| text.starts_with(p)) {
                self.quiet_until = Some(u64::MAX);
                return Ok(buf.len());
            }
            if text.starts_with("pd req") {
                let refused =
                    !self.pd_last || (text == "pd req=1" && self.refuse_5v.load(Ordering::SeqCst));
                if refused {
                    self.injected.extend(b"req error 252\n");
                    return Ok(buf.len());
                }
            }
            if text.starts_with("pd hrst") {
                if !self.pd_last {
                    self.quiet_until = Some(u64::MAX);
                    return Ok(buf.len());
                }
                let ms = u64::try_from(self.hrst_offers_after.as_millis()).unwrap_or(u64::MAX);
                self.quiet_until = Some(self.clock.now_ms().saturating_add(ms));
            }
            if text.starts_with("entry ") {
                self.pd_last = text.starts_with("entry pd");
            }
            if text.starts_with("reset") || text.starts_with("pdm close") {
                self.pd_last = false;
            }
            self.port.write(buf)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            self.port.flush()
        }
    }

    /// How long the Hard Reset's offers take on the meter (device run
    /// 2026-11-06: `pd hrst` at 20:08:11, nothing until the next write 3 s
    /// later).
    const HRST_OFFERS_AFTER: Duration = Duration::from_secs(3);

    struct MeterLike {
        sim: SimPdm,
        clock: SimClock,
        w: TriggerWorker,
        written: Arc<Mutex<Vec<String>>>,
        refuse_5v: Arc<AtomicBool>,
    }

    /// What [`meter_like_port`] gives: the simulated meter, its clock, the
    /// port, its writes and its `refuse_5v` switch.
    type MeterLikePort = (
        SimPdm,
        SimClock,
        LikeTheMeter,
        Arc<Mutex<Vec<String>>>,
        Arc<AtomicBool>,
    );

    /// A port answering [`LikeTheMeter`], on `charger`.
    fn meter_like_port(
        charger: SimCharger,
        hrst_offers_after: Duration,
        unanswered: Vec<&'static str>,
    ) -> MeterLikePort {
        let (sim, clock) = sim(charger);
        let written = Arc::new(Mutex::new(Vec::new()));
        let refuse_5v = Arc::new(AtomicBool::new(false));
        let port = LikeTheMeter {
            port: sim.open_port(),
            clock: clock.clone(),
            pd_last: false,
            hrst_offers_after,
            refuse_5v: Arc::clone(&refuse_5v),
            unanswered,
            quiet_until: None,
            injected: std::collections::VecDeque::new(),
            written: Arc::clone(&written),
        };
        (sim, clock, port, written, refuse_5v)
    }

    /// A session on a [`LikeTheMeter`] port whose VBUS watcher reads 9 V with
    /// the charger's pull-up on CC1: the charger stays attached.
    fn meter_like(
        charger: SimCharger,
        hrst_offers_after: Duration,
        unanswered: Vec<&'static str>,
    ) -> MeterLike {
        let (sim, clock, port, written, refuse_5v) =
            meter_like_port(charger, hrst_offers_after, unanswered);
        let w = TriggerWorker::spawn(port, clock.clone()).expect("the session opens");
        w.watch_vbus(fresh(|| Some(9.0)));
        MeterLike {
            sim,
            clock,
            w,
            written,
            refuse_5v,
        }
    }

    /// The device run's state: a 9 V PD contract, then QC listed (what
    /// Detect leaves entered last), the charger still attached.
    fn contract_then_legacy(c: &TriggerControl) {
        c.list(TriggerProtocol::Pd).expect("list pd");
        c.request(pd(2), true).expect("9 V");
        c.list(TriggerProtocol::Qc).expect("list qc");
        assert!(!c.charger_gone().expect("charger gone"));
    }

    fn count(writes: &[String], cmd: &str) -> usize {
        writes.iter().filter(|w| w.as_str() == cmd).count()
    }

    // AC-ZRJ8W6.1: leaving sends `pd req=1`, `pd hrst` and `reset` once each,
    // then `pdm close`, all within the quit wait, though the Hard Reset's
    // offers take three seconds.

    #[test]
    fn with_the_charger_attached_after_a_contract_and_a_legacy_entry_leaving_closes_within_the_quit_wait()
     {
        let MeterLike {
            sim, w, written, ..
        } = meter_like(SimCharger::MultiProtocol, HRST_OFFERS_AFTER, Vec::new());
        let c = w.control();
        contract_then_legacy(&c);
        let before = written.lock().unwrap().len();

        let start = Instant::now();
        let _ = c.leave_view();
        let closed = w.close_within(QUIT_WAIT);
        let took = start.elapsed();

        let writes = written_since(&written, before);
        assert!(
            took < QUIT_WAIT,
            "leaving and closing took {took:?}: {writes:?}"
        );
        closed.expect("closed");
        assert_eq!(
            writes,
            ["pd req=1", "pd hrst", "reset", "pdm close"].map(str::to_owned),
            "the way back once, then the close"
        );
        assert!(!sim.pdm_active(), "pdm close was sent");
    }

    // AC-ZRJ8W6.2: quitting from the Trigger tab in that state sends the way
    // back to 5 V and then the close before the app exits; the app log has
    // the close.

    /// A manager connected to the 9 V demo meter (the charger attached), its
    /// app log at `path`, holding a session on a [`LikeTheMeter`] port.
    fn manager_with_meter_like(
        path: &std::path::Path,
        unanswered: Vec<&'static str>,
    ) -> (
        DeviceManager<AppOpener, SystemClock>,
        Arc<Mutex<Vec<String>>>,
    ) {
        let mut m = DeviceManager::new(|| Ok(Vec::new()), AppOpener, SystemClock);
        m.set_app_log(crate::app_log::AppLog::open(path).expect("open app.log"));
        m.connect_with_stop(
            "demo:pd-9v-2a",
            50,
            |_: BackendEvent| {},
            Arc::new(AtomicBool::new(false)),
        )
        .expect("the demo device connects");
        let (_sim, clock, port, written, _refuse) =
            meter_like_port(SimCharger::MultiProtocol, HRST_OFFERS_AFTER, unanswered);
        let worker = TriggerWorker::spawn(port, clock).expect("the session opens");
        m.install_trigger(worker).expect("installed");
        (m, written)
    }

    fn log_dir(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("open-powerz-zrj8w6-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create the log folder");
        dir
    }

    #[test]
    fn quitting_from_the_trigger_tab_with_the_charger_attached_returns_to_5v_and_logs_the_close() {
        let dir = log_dir("quit");
        let path = dir.join("app.log");
        let (mut m, written) = manager_with_meter_like(&path, Vec::new());
        let c = m.trigger_control().expect("open");
        contract_then_legacy(&c);
        drop(c);
        let before = written.lock().unwrap().len();

        let start = Instant::now();
        m.shutdown();
        let took = start.elapsed();

        let writes = written_since(&written, before);
        let back = writes
            .iter()
            .position(|w| w == "pd req=1")
            .unwrap_or_else(|| panic!("no way back to 5 V: {writes:?}"));
        let close = last_index(&writes, "pdm close")
            .unwrap_or_else(|| panic!("no pdm close before the app exits: {writes:?}"));
        assert!(back < close, "{writes:?}");
        let log = std::fs::read_to_string(&path).expect("read app.log");
        let lines: Vec<&str> = log.lines().collect();
        let sent_back = lines
            .iter()
            .position(|l| l.contains(&format!("{SENT_PREFIX}pd req=1")))
            .unwrap_or_else(|| panic!("no way back in the app log: {log}"));
        assert!(
            lines[sent_back..]
                .iter()
                .any(|l| l.contains(&format!("{SENT_PREFIX}pdm close"))),
            "no close after the way back in the app log: {log}"
        );
        // The session's part of quitting stays within the quit wait: a close
        // still running when it ran out would be cut off as the app exits.
        assert!(took < QUIT_WAIT, "quitting took {took:?}: {writes:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // AC-ZRJ8W6.4: the close never repeats a way back that leaving sent,
    // whatever its outcome.

    #[test]
    fn the_close_does_not_repeat_a_way_back_whose_hard_reset_got_no_offers() {
        // PD entered last, the 5 V request refused, the Hard Reset unanswered.
        let MeterLike {
            w,
            written,
            refuse_5v,
            ..
        } = meter_like(SimCharger::Epr140W, HRST_OFFERS_AFTER, vec!["pd hrst"]);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        c.request(pd(2), true).expect("9 V");
        refuse_5v.store(true, Ordering::SeqCst);
        let _ = c.leave_view();
        let after_leave = written.lock().unwrap().len();
        assert!(
            written_since(&written, 0).iter().any(|w| w == "pd hrst"),
            "leaving sent the Hard Reset"
        );

        let _ = w.close_within(QUIT_WAIT + QUIT_WAIT);
        assert_eq!(
            written_since(&written, after_leave),
            vec!["pdm close".to_owned()],
            "the close repeated the way back"
        );
    }

    #[test]
    fn the_close_does_not_repeat_a_legacy_reset_that_got_no_answer() {
        let MeterLike { w, written, .. } =
            meter_like(SimCharger::MultiProtocol, HRST_OFFERS_AFTER, vec!["reset"]);
        let c = w.control();
        c.list(TriggerProtocol::Qc).expect("list qc");
        c.request(TriggerRequest::Qc { volts: 12 }, true)
            .expect("12 V");
        let before = written.lock().unwrap().len();
        let left = c.leave_view();
        assert!(left.is_err(), "the reset was never answered: {left:?}");
        let after_leave = written.lock().unwrap().len();
        assert_eq!(
            count(&written_since(&written, before), "reset"),
            1,
            "leaving sent the reset"
        );

        let _ = w.close_within(QUIT_WAIT + QUIT_WAIT);
        assert_eq!(
            written_since(&written, after_leave),
            vec!["pdm close".to_owned()],
            "the close repeated the way back"
        );
    }

    #[test]
    fn the_close_does_not_repeat_the_way_back_after_a_contract_and_a_legacy_entry() {
        let MeterLike { w, written, .. } =
            meter_like(SimCharger::MultiProtocol, HRST_OFFERS_AFTER, Vec::new());
        let c = w.control();
        contract_then_legacy(&c);
        let before = written.lock().unwrap().len();
        let _ = c.leave_view();
        let after_leave = written.lock().unwrap().len();
        let left = written_since(&written, before);
        assert_eq!(count(&left, "pd req=1"), 1, "{left:?}");
        assert_eq!(count(&left, "pd hrst"), 1, "{left:?}");
        assert_eq!(count(&left, "reset"), 1, "{left:?}");

        let _ = w.close_within(QUIT_WAIT + QUIT_WAIT);
        assert_eq!(
            written_since(&written, after_leave),
            vec!["pdm close".to_owned()],
            "the close repeated the way back"
        );
    }

    // AC-ZRJ8W6.5 (regression): with the charger gone, leaving and quitting
    // send no 5 V request and no Hard Reset; the close is in the app log.
    // Also covered by the FR JV54SQ tests above.

    #[test]
    fn with_the_charger_gone_leaving_then_quitting_logs_the_close_and_no_way_back() {
        let dir = log_dir("gone-leave-quit");
        let path = dir.join("app.log");
        let mut m = DeviceManager::new(|| Ok(Vec::new()), AppOpener, SystemClock);
        m.set_app_log(crate::app_log::AppLog::open(&path).expect("open app.log"));
        // The idle demo meter reads about 0 V: the contract never rises.
        m.connect_with_stop(
            "demo:idle",
            50,
            |_: BackendEvent| {},
            Arc::new(AtomicBool::new(false)),
        )
        .expect("the demo device connects");
        m.open_trigger().expect("the demo PDM port");
        let c = m.trigger_control().expect("open");
        c.list(TriggerProtocol::Pd).expect("list pd");
        c.request(pd(1), false).expect("5 V");
        eventually("the charger gone", || {
            c.charger_gone().expect("charger gone")
        });
        let _ = c.leave_view();
        drop(c);
        let start = Instant::now();
        m.shutdown();
        let took = start.elapsed();

        let log = std::fs::read_to_string(&path).expect("read app.log");
        let lines: Vec<&str> = log.lines().collect();
        let gone = lines
            .iter()
            .position(|l| l.contains(COLLAPSE_PREFIX) || l.contains(CC_GONE_PREFIX))
            .unwrap_or_else(|| panic!("no gone line: {log}"));
        let after = &lines[gone..];
        assert!(
            after
                .iter()
                .any(|l| l.contains(&format!("{SENT_PREFIX}pdm close"))),
            "no close after the charger went away: {log}"
        );
        for way_back in ["pd req", "pd hrst"] {
            let sent = format!("{SENT_PREFIX}{way_back}");
            assert!(
                !after.iter().any(|l| l.contains(&sent)),
                "{sent:?} after the charger went away: {log}"
            );
        }
        assert!(took < QUIT_WAIT, "quitting took {took:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // AC-ZRJ8W6.6 (regression): Back to 5 V still waits for the charger's new
    // offers after a Hard Reset; the session stays open.

    #[test]
    fn back_to_5v_still_waits_for_the_offers_after_a_hard_reset() {
        let MeterLike {
            sim,
            clock,
            w,
            written,
            refuse_5v,
        } = meter_like(SimCharger::Epr140W, Duration::from_secs(1), Vec::new());
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        c.request(pd(2), true).expect("9 V");
        refuse_5v.store(true, Ordering::SeqCst);
        let before = written.lock().unwrap().len();
        let at = clock.now_ms();

        c.back_to_5v()
            .expect("back to 5 V through the Hard Reset's offers");

        let writes = written_since(&written, before);
        assert_eq!(
            writes,
            ["pd req=1", "pd hrst"].map(str::to_owned),
            "{writes:?}"
        );
        assert!(
            clock.now_ms() - at >= 1000,
            "the offers were not waited for: {} ms",
            clock.now_ms() - at
        );
        assert!(sim.pdm_active(), "Back to 5 V keeps the session open");
        w.close().expect("close");
    }
}

/// FR DGSQ6T: the builder and the raw console on the trigger session.
#[cfg(test)]
mod custom_message_tests {
    use powerz_sim::{SimCable, SimCharger, SimClock, SimPdm};

    use super::*;
    use crate::ui_error::ErrorKind;

    fn worker_with(cable: Option<SimCable>) -> (SimPdm, TriggerWorker) {
        let clock = SimClock::new(1_760_000_000_000);
        let mut sim = SimPdm::new(SimCharger::Epr140W, clock.clone());
        if let Some(c) = cable {
            sim = sim.with_cable(c);
        }
        let w = TriggerWorker::spawn(sim.open_port(), clock).expect("the session opens");
        (sim, w)
    }

    fn worker() -> (SimPdm, TriggerWorker) {
        worker_with(None)
    }

    fn writes_after(sim: &SimPdm, skip: usize) -> Vec<String> {
        sim.writes()
            .into_iter()
            .skip(skip)
            .map(|w| w.trim().to_owned())
            .collect()
    }

    fn msg(sop: MessageSop, msg_type: u8, data_objects: Vec<u32>) -> PdMessageRequest {
        PdMessageRequest {
            sop,
            msg_type,
            data_objects,
        }
    }

    /// Discover Identity REQ (structured VDM, SVID 0xFF00) to the cable.
    fn discover_cable() -> PdMessageRequest {
        msg(MessageSop::SopPrime, 15, vec![0xFF00_A801])
    }

    // ---- AC-DGSQ6T.1: the builder sends control and data messages ----

    #[test]
    fn a_built_control_message_goes_out_as_pd_cmd_or_pd_data() {
        let (sim, w) = worker();
        let c = w.control();
        let before = sim.writes().len();
        let r = c
            .send_message(msg(MessageSop::Sop, 13, vec![]), false)
            .expect("soft reset needs no confirmation");
        assert_eq!(r.command, "pd cmd=13");
        let r = c
            .send_message(msg(MessageSop::SopDoublePrime, 13, vec![]), false)
            .expect("sent");
        assert_eq!(r.command, "pd data=028D00");
        let writes = writes_after(&sim, before);
        // Nothing was entered yet: PD first, like `powerz pd send`.
        assert_eq!(writes.first().map(String::as_str), Some("entry pd"));
        let messages: Vec<&String> = writes.iter().filter(|w| w.starts_with("pd ")).collect();
        assert_eq!(messages, ["pd cmd=13", "pd data=028D00"]);
        assert_eq!(
            r.lines.first().map(|l| l.direction),
            Some(ConsoleDirection::Tx)
        );
        w.close().expect("close");
    }

    #[test]
    fn the_ui_names_of_the_sops_deserialize() {
        let m: PdMessageRequest = serde_json::from_value(serde_json::json!({
            "sop": "sop_prime", "msg_type": 15, "data_objects": [4278233089u32]
        }))
        .expect("deserializes");
        assert_eq!(m, discover_cable());
    }

    // ---- AC-DGSQ6T.2: the cable preset's reply is decoded ----

    #[test]
    fn discover_identity_to_the_cable_returns_its_emarker_details() {
        let (sim, w) = worker_with(Some(SimCable::Epr240W));
        let c = w.control();
        // Confirmed either way: what the classifier says about a VDM is its call.
        let r = c.send_message(discover_cable(), true).expect("sent");
        assert!(r.command.starts_with("pd data=01"), "{}", r.command);
        assert!(sim.writes().iter().any(|w| w.trim() == r.command));
        let cable = r.cable.expect("the cable answered");
        assert_eq!(cable.vid, 0x050D);
        assert_eq!(cable.pid, Some(0x8254));
        assert_eq!(cable.max_voltage_v, Some(50));
        assert_eq!(cable.max_current_ma, Some(5000));
        assert!(cable.epr_capable);
        let json = serde_json::to_value(&cable).unwrap();
        assert_eq!(json["vid"], 0x050D);
        assert_eq!(json["epr_capable"], true);
        w.close().expect("close");
    }

    // ---- AC-DGSQ6T.3: GoodCRC is refused with a reason ----

    #[test]
    fn goodcrc_is_refused_in_the_builder_and_the_console() {
        let (sim, w) = worker();
        let c = w.control();
        let before = sim.writes().len();
        for sop in [MessageSop::Sop, MessageSop::SopPrime] {
            let err = c
                .send_message(msg(sop, 1, vec![]), true)
                .expect_err("refused");
            assert_eq!(err.kind, ErrorKind::Usage);
            assert!(err.message.contains("GoodCRC"), "{}", err.message);
            let err = c.message_risk(msg(sop, 1, vec![])).expect_err("refused");
            assert!(err.message.contains("GoodCRC"), "{}", err.message);
        }
        for line in ["pd cmd=1", "pd data=014100"] {
            let err = c.console_send(line.to_owned(), true).expect_err("refused");
            assert_eq!(err.kind, ErrorKind::Usage);
            assert!(err.message.contains("GoodCRC"), "{line}: {}", err.message);
        }
        assert_eq!(sim.writes().len(), before, "nothing was written");
        w.close().expect("close");
    }

    // ---- AC-DGSQ6T.4: the console's transcript ----

    #[test]
    fn the_console_returns_sent_and_received_lines_with_times() {
        let (sim, w) = worker();
        let c = w.control();
        let lines = c
            .console_send("pdm parm".to_owned(), false)
            .expect("read-only needs no confirmation");
        assert!(sim.writes().iter().any(|w| w.trim() == "pdm parm"));
        assert_eq!(lines[0].direction, ConsoleDirection::Tx);
        assert_eq!(lines[0].text, "pdm parm");
        assert!(lines[0].at_ms >= 1_760_000_000_000, "{lines:?}");
        let rx = lines
            .iter()
            .find(|l| l.direction == ConsoleDirection::Rx)
            .expect("a reply");
        assert_eq!(rx.text, "type=0,em=2,sink=0");
        assert!(rx.at_ms >= lines[0].at_ms);
        let json = serde_json::to_value(&lines[0]).unwrap();
        assert_eq!(json["direction"], "tx");
        assert!(json["at_ms"].is_u64());
        w.close().expect("close");
    }

    #[test]
    fn the_console_shows_a_parsed_block() {
        let (_sim, w) = worker();
        let c = w.control();
        let lines = c.console_send("entry pd".to_owned(), false).expect("entry");
        let block = lines
            .iter()
            .find_map(|l| l.block.as_deref())
            .unwrap_or_else(|| panic!("no block in {lines:?}"));
        assert!(block.contains("9.00 V"), "{block}");
        w.close().expect("close");
    }

    // ---- AC-DGSQ6T.5: above 5 V and unparsable lines need the confirmation ----

    #[test]
    fn an_unconfirmed_unparsable_line_is_refused_and_writes_nothing() {
        let (sim, w) = worker();
        let c = w.control();
        assert_eq!(
            c.console_risk("hello meter".to_owned()).expect("risk"),
            RiskClass::Above5V
        );
        assert_eq!(
            c.console_risk("pdm parm".to_owned()).expect("risk"),
            RiskClass::ReadOnly
        );
        let before = sim.writes().len();
        let err = c
            .console_send("hello meter".to_owned(), false)
            .expect_err("refused");
        assert_eq!(err.kind, ErrorKind::Usage);
        assert_eq!(sim.writes().len(), before, "nothing was written");
        c.console_send("hello meter".to_owned(), true)
            .expect("confirmed");
        assert_eq!(writes_after(&sim, before), ["hello meter"]);
        w.close().expect("close");
    }

    #[test]
    fn a_built_request_for_9v_needs_the_confirmation() {
        let (sim, w) = worker();
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        let request_9v = msg(MessageSop::Sop, 2, vec![0x2304_B12C]);
        assert_eq!(
            c.message_risk(request_9v.clone()).expect("risk"),
            RiskClass::Above5V
        );
        let before = sim.writes().len();
        let vbus = sim.vbus_mv();
        let err = c
            .send_message(request_9v.clone(), false)
            .expect_err("refused");
        assert_eq!(err.kind, ErrorKind::Usage);
        assert_eq!(sim.writes().len(), before, "nothing was written");
        assert_eq!(sim.vbus_mv(), vbus);
        let r = c.send_message(request_9v, true).expect("confirmed");
        assert_eq!(r.command, "pd data=0082102CB10423");
        w.close().expect("close");
    }

    /// The simulated port; the write of `fail_write` reaches the meter but
    /// reports an error, and one read fails after `fail_read_after`.
    struct Faulty {
        port: powerz_sim::SimPdmPort,
        fail_write: Option<&'static str>,
        fail_read_after: Option<&'static str>,
        fail_next_read: bool,
    }

    impl Faulty {
        fn spawn(
            charger: SimCharger,
            fail_write: Option<&'static str>,
            fail_read_after: Option<&'static str>,
        ) -> (SimPdm, TriggerWorker) {
            let clock = SimClock::new(1_760_000_000_000);
            let sim = SimPdm::new(charger, clock.clone());
            let port = Faulty {
                port: sim.open_port(),
                fail_write,
                fail_read_after,
                fail_next_read: false,
            };
            let w = TriggerWorker::spawn(port, clock).expect("the session opens");
            (sim, w)
        }
    }

    impl Read for Faulty {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if std::mem::take(&mut self.fail_next_read) {
                return Err(std::io::Error::other("read failed"));
            }
            self.port.read(buf)
        }
    }

    impl Write for Faulty {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let text = String::from_utf8_lossy(buf).into_owned();
            let n = self.port.write(buf)?;
            if self.fail_read_after == Some(text.as_str()) {
                self.fail_next_read = true;
            }
            if self.fail_write == Some(text.as_str()) {
                return Err(std::io::Error::other("write failed"));
            }
            Ok(n)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            self.port.flush()
        }
    }

    fn returned_to_5v_before_close(after: &[String]) {
        let back = after
            .iter()
            .rposition(|w| w == "pd req=1" || w == "pd hrst")
            .unwrap_or_else(|| panic!("no return through PD: {after:?}"));
        let close = after
            .iter()
            .rposition(|w| w == "pdm close")
            .unwrap_or_else(|| panic!("no pdm close: {after:?}"));
        assert!(back < close, "{after:?}");
    }

    #[test]
    fn a_console_read_error_after_the_write_still_records_the_undo() {
        // QC was entered last, so only the recorded PD contract makes the
        // close go back through PD.
        let (sim, w) = Faulty::spawn(SimCharger::MultiProtocol, None, Some("pd req=2"));
        let c = w.control();
        c.list(TriggerProtocol::Qc).expect("list qc");
        c.console_send("pd req=2".to_owned(), true)
            .expect_err("the read failed");
        assert!(sim.writes().iter().any(|w| w == "pd req=2"));
        let before = sim.writes().len();
        let _ = w.close();
        returned_to_5v_before_close(&writes_after(&sim, before));
    }

    #[test]
    fn a_message_whose_write_fails_is_still_undone_through_pd() {
        let (sim, w) = Faulty::spawn(
            SimCharger::MultiProtocol,
            Some("pd data=0082102CB10423"),
            None,
        );
        let c = w.control();
        c.list(TriggerProtocol::Qc).expect("list qc");
        c.send_message(msg(MessageSop::Sop, 2, vec![0x2304_B12C]), true)
            .expect_err("the write failed");
        let before = sim.writes().len();
        let _ = w.close();
        returned_to_5v_before_close(&writes_after(&sim, before));
    }

    // AC-AXHK2N.9: a Hard Reset takes VBUS down on purpose.

    #[test]
    fn a_hard_reset_fallback_ends_the_contract() {
        let (sim, w) = Faulty::spawn(SimCharger::Epr140W, Some("pd req=1"), None);
        let vbus = Arc::new(std::sync::Mutex::new(Some(9.0)));
        let reading = Arc::clone(&vbus);
        w.watch_vbus(fresh(move || *reading.lock().unwrap()));
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        let nine = TriggerRequest::Pd {
            position: 2,
            voltage_mv: None,
            current_ma: None,
        };
        c.request(nine, true).expect("9 V");
        // The failed `pd req=1` leaves the meter out of step here.
        let _ = c.back_to_5v();
        assert!(sim.writes().iter().any(|w| w.trim() == "pd hrst"));
        *vbus.lock().unwrap() = Some(0.0);
        // Well past the 300 ms a collapse needs.
        std::thread::sleep(Duration::from_millis(500));
        assert!(!c.charger_gone().expect("charger gone"));
        let _ = w.close();
    }

    // AC-AXHK2N.16: the Hard Reset fallback is logged like any write.

    #[test]
    fn the_hard_reset_fallback_is_logged_with_its_text() {
        let (sim, w) = Faulty::spawn(SimCharger::Epr140W, Some("pd req=1"), None);
        let lines: Arc<std::sync::Mutex<Vec<String>>> = Arc::default();
        let keep = Arc::clone(&lines);
        w.log_to(move |line: String| keep.lock().unwrap().push(line));
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        let nine = TriggerRequest::Pd {
            position: 2,
            voltage_mv: None,
            current_ma: None,
        };
        c.request(nine, true).expect("9 V");
        // The failed `pd req=1` leaves the meter out of step here.
        let _ = c.back_to_5v();
        let _ = w.close();
        let all = lines.lock().unwrap().clone();
        let text = |prefix: &str| -> Vec<String> {
            all.iter()
                .filter_map(|l| l.strip_prefix(prefix).map(str::to_owned))
                .collect()
        };
        // The failed write is logged too, marked failed.
        assert_eq!(text("trigger send failed: "), vec!["pd req=1"]);
        assert!(text("trigger sent: ").iter().any(|s| s == "pd hrst"));
        let written: Vec<String> = all
            .iter()
            .filter_map(|l| {
                l.strip_prefix("trigger sent: ")
                    .or_else(|| l.strip_prefix("trigger send failed: "))
                    .map(str::to_owned)
            })
            .collect();
        assert_eq!(
            written,
            writes_after(&sim, 0),
            "one line per write, in order"
        );
    }

    #[test]
    fn a_confirmed_raw_line_with_pd_entered_is_undone_through_pd() {
        let clock = SimClock::new(1_760_000_000_000);
        let sim = SimPdm::new(SimCharger::MultiProtocol, clock.clone());
        let w = TriggerWorker::spawn(sim.open_port(), clock).expect("the session opens");
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        c.console_send("hello meter".to_owned(), true)
            .expect("confirmed");
        // Entering QC afterwards must not lose the PD undo.
        c.list(TriggerProtocol::Qc).expect("list qc");
        let before = sim.writes().len();
        w.close().expect("close");
        returned_to_5v_before_close(&writes_after(&sim, before));
    }

    #[test]
    fn pdm_open_and_pdm_close_are_refused_in_the_console() {
        let (sim, w) = worker();
        let c = w.control();
        let before = sim.writes().len();
        for line in ["pdm close", "pdm open"] {
            let err = c.console_send(line.to_owned(), true).expect_err("refused");
            assert_eq!(err.kind, ErrorKind::Usage);
            assert!(err.message.contains("pdm close"), "{}", err.message);
        }
        assert_eq!(sim.writes().len(), before, "nothing was written");
        w.close().expect("close");
    }

    #[test]
    fn goodcrc_in_any_raw_spelling_is_refused_in_the_console() {
        let (sim, w) = worker();
        let c = w.control();
        let before = sim.writes().len();
        for line in ["pd cmd=01", "pd cmd=+1", "pd cmd=0x1", "pd  cmd=1"] {
            let err = c.console_send(line.to_owned(), true).expect_err("refused");
            assert_eq!(err.kind, ErrorKind::Usage);
        }
        assert_eq!(sim.writes().len(), before, "nothing was written");
        w.close().expect("close");
    }

    #[test]
    fn any_structured_discover_identity_to_the_cable_waits_for_its_answer() {
        let (_sim, w) = worker_with(Some(SimCable::Epr240W));
        let c = w.control();
        // VDM version 1.0 and a second object: not the preset's exact VDO.
        let r = c
            .send_message(msg(MessageSop::SopPrime, 15, vec![0xFF00_8001, 0]), true)
            .expect("sent");
        let cable = r.cable.expect("the cable answered");
        assert_eq!(cable.vid, 0x050D);
        w.close().expect("close");
    }

    #[test]
    fn closing_after_a_raw_request_returns_to_5v() {
        let (sim, w) = worker();
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        c.console_send("pd req=2".to_owned(), true)
            .expect("confirmed");
        let before = sim.writes().len();
        w.close().expect("close");
        let after = writes_after(&sim, before);
        assert!(
            after.iter().any(|w| w == "pd req=1" || w == "pd hrst"),
            "{after:?}"
        );
        assert_eq!(after.last().map(String::as_str), Some("pdm close"));
    }
}

/// FR AXHK2P: the one-click cable read and its narrow exemption.
#[cfg(test)]
mod cable_read_tests {
    use powerz_protocol::pdm::{Capabilities, Command, Risk};
    use powerz_sim::{SimCable, SimCharger, SimClock, SimPdm};

    use super::*;
    use crate::ui_error::ErrorKind;

    /// SOP', header 0x108F, object 0xFF00A801, both little endian.
    const READ_BYTES: [u8; 7] = [0x01, 0x8F, 0x10, 0x01, 0xA8, 0x00, 0xFF];
    const READ_LINE: &str = "pd data=018F1001A800FF";

    fn worker_with(cable: Option<SimCable>) -> (SimPdm, TriggerWorker) {
        let clock = SimClock::new(1_760_000_000_000);
        let mut sim = SimPdm::new(SimCharger::Epr140W, clock.clone());
        if let Some(c) = cable {
            sim = sim.with_cable(c);
        }
        let w = TriggerWorker::spawn(sim.open_port(), clock).expect("the session opens");
        (sim, w)
    }

    /// A session with PD entered, and the cable if any.
    fn in_pd(cable: Option<SimCable>) -> (SimPdm, TriggerWorker) {
        let (sim, w) = worker_with(cable);
        w.control().list(TriggerProtocol::Pd).expect("list pd");
        (sim, w)
    }

    fn writes_after(sim: &SimPdm, skip: usize) -> Vec<String> {
        sim.writes()
            .into_iter()
            .skip(skip)
            .map(|w| w.trim().to_owned())
            .collect()
    }

    fn read_message() -> PdMessageRequest {
        PdMessageRequest {
            sop: MessageSop::SopPrime,
            msg_type: 15,
            data_objects: vec![0xFF00_A801],
        }
    }

    fn found(read: Result<CableRead, UiError>) -> CableView {
        match read.expect("the read ran") {
            CableRead::Found(view) => view,
            CableRead::NoAnswer => panic!("no answer"),
        }
    }

    fn refused(sim: &SimPdm, c: &TriggerControl, kind: ErrorKind) {
        let before = sim.writes().len();
        let Err(err) = c.cable_read() else {
            panic!("the read was not refused");
        };
        assert_eq!(err.kind, kind, "{err:?}");
        assert_eq!(writes_after(sim, before), Vec::<String>::new());
    }

    // ---- AC-AXHK2P.1: one read shows the whole e-marker ----

    #[test]
    fn the_exempt_bytes_are_what_the_preset_builds() {
        let built = message::Preset::DiscoverIdentityCable
            .message()
            .to_command()
            .expect("it builds");
        assert_eq!(built, Command::PdData(READ_BYTES.to_vec()));
        assert_eq!(command_text(&built), READ_LINE);
        assert!(is_cable_read(&built));
    }

    #[test]
    fn a_read_returns_every_field_of_the_card() {
        let (_sim, w) = in_pd(Some(SimCable::Epr240W));
        let view = found(w.control().cable_read());
        assert_eq!(view.vid, 0x050D);
        assert_eq!(view.pid, Some(0x8254));
        assert_eq!(view.max_current_ma, Some(5000));
        assert_eq!(view.max_voltage_v, Some(50));
        assert_eq!(view.speed.as_deref(), Some("USB 2.0"));
        assert!(!view.active);
        assert!(view.epr_capable);
        assert_eq!(
            view.vendor.as_deref(),
            pdm::vendors::vendor_name(0x050D),
            "the name comes from our table"
        );
        w.close().expect("close");
    }

    #[test]
    fn a_read_serializes_for_the_ui() {
        let (_sim, w) = in_pd(Some(SimCable::Epr240W));
        let view = found(w.control().cable_read());
        let json = serde_json::to_value(CableRead::Found(view)).expect("json");
        assert_eq!(json["kind"], "found");
        assert_eq!(json["cable"]["vid"], 0x050D);
        assert_eq!(json["cable"]["speed"], "USB 2.0");
        assert!(json["cable"].get("vendor").is_some(), "{json}");
        let json = serde_json::to_value(CableRead::NoAnswer).expect("json");
        assert_eq!(json, serde_json::json!({ "kind": "no_answer" }));
        w.close().expect("close");
    }

    // ---- AC-AXHK2P.5: the vendor comes from our table, else stays out ----

    /// A passive cable's identity from `vid`.
    fn identity_of(vid: u16) -> CableIdentity {
        let objects = [
            0xFF00_A841,
            0x1860_0000 | u32::from(vid),
            0x0000_404D,
            0x8254_0000,
            0x000A_2641,
        ];
        let body: Vec<u8> = objects.iter().flat_map(|o: &u32| o.to_le_bytes()).collect();
        message::cable_identity_from_message(1, 0x518F, &body).expect("a cable ACK")
    }

    #[test]
    fn a_listed_vendor_gets_its_name_and_the_speed_its_text() {
        let view = cable_view(&identity_of(0x05AC));
        assert_eq!(view.vendor.as_deref(), Some("Apple"));
        assert_eq!(view.speed.as_deref(), Some("USB 3.2 Gen 1"));
    }

    #[test]
    fn an_unlisted_vendor_has_no_name_and_keeps_its_vid() {
        let view = cable_view(&identity_of(0x1234));
        assert_eq!(view.vendor, None);
        assert_eq!(view.vid, 0x1234);
    }

    // ---- AC-AXHK2P.2: only inside an open PD session, one message, no retry ----

    #[test]
    fn with_nothing_entered_the_read_is_refused_and_writes_nothing() {
        let (sim, w) = worker_with(Some(SimCable::Epr240W));
        refused(&sim, &w.control(), ErrorKind::Usage);
        w.close().expect("close");
    }

    #[test]
    fn with_a_legacy_protocol_entered_last_the_read_is_refused_and_writes_nothing() {
        let clock = SimClock::new(1_760_000_000_000);
        let sim = SimPdm::new(SimCharger::MultiProtocol, clock.clone());
        let w = TriggerWorker::spawn(sim.open_port(), clock).expect("the session opens");
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        let section = c.list(TriggerProtocol::Qc).expect("list qc");
        assert!(section.supported);
        refused(&sim, &c, ErrorKind::Usage);
        w.close().expect("close");
    }

    #[test]
    fn with_a_protocol_the_charger_lacks_entered_last_the_read_is_refused() {
        let (sim, w) = in_pd(Some(SimCable::Epr240W));
        let c = w.control();
        let section = c.list(TriggerProtocol::Qc).expect("list qc");
        assert!(!section.supported);
        refused(&sim, &c, ErrorKind::Usage);
        w.close().expect("close");
    }

    #[test]
    fn after_detection_the_read_is_refused_and_writes_nothing() {
        let (sim, w) = in_pd(Some(SimCable::Epr240W));
        let c = w.control();
        c.detect().expect("detect");
        refused(&sim, &c, ErrorKind::Usage);
        // PD is gone, so the way back times out: not what this test is about.
        let _ = w.close();
    }

    #[test]
    fn a_legacy_protocol_entered_from_the_console_refuses_the_read() {
        let (sim, w) = in_pd(Some(SimCable::Epr240W));
        let c = w.control();
        c.console_send("entry qc".to_owned(), false).expect("entry");
        refused(&sim, &c, ErrorKind::Usage);
        w.close().expect("close");
    }

    #[test]
    fn a_reset_from_the_console_refuses_the_read() {
        let (sim, w) = in_pd(Some(SimCable::Epr240W));
        let c = w.control();
        c.console_send("reset".to_owned(), false).expect("reset");
        refused(&sim, &c, ErrorKind::Usage);
        // PD is gone, so the way back times out: not what this test is about.
        let _ = w.close();
    }

    #[test]
    fn pd_entered_from_the_console_is_an_open_pd_session() {
        let (sim, w) = worker_with(Some(SimCable::Epr240W));
        let c = w.control();
        c.console_send("entry pd".to_owned(), false).expect("entry");
        let before = sim.writes().len();
        assert_eq!(found(c.cable_read()).vid, 0x050D);
        assert_eq!(writes_after(&sim, before), [READ_LINE]);
        w.close().expect("close");
    }

    #[test]
    fn pd_entered_by_a_pd_request_is_an_open_pd_session() {
        // Detect ends on the last protocol it listed; a PD offer enters PD again.
        let (sim, w) = in_pd(Some(SimCable::Epr240W));
        let c = w.control();
        c.list(TriggerProtocol::Qc).expect("list qc");
        c.request(
            TriggerRequest::Pd {
                position: 1,
                voltage_mv: None,
                current_ma: None,
            },
            false,
        )
        .expect("5 V");
        let before = sim.writes().len();
        assert_eq!(found(c.cable_read()).vid, 0x050D);
        assert_eq!(writes_after(&sim, before), [READ_LINE]);
        w.close().expect("close");
    }

    #[test]
    fn one_press_writes_exactly_one_line() {
        let (sim, w) = in_pd(Some(SimCable::Epr240W));
        let before = sim.writes().len();
        found(w.control().cable_read());
        assert_eq!(writes_after(&sim, before), [READ_LINE]);
        w.close().expect("close");
    }

    // ---- AC-AXHK2P.4: no answer, nothing retried ----

    #[test]
    fn no_answer_after_one_send_and_no_second_one() {
        let (sim, w) = in_pd(None);
        let c = w.control();
        let before = sim.writes().len();
        assert!(matches!(c.cable_read(), Ok(CableRead::NoAnswer)));
        assert_eq!(writes_after(&sim, before), [READ_LINE]);
        // The session goes on: a press is what sends the next one.
        c.armed().expect("the session is alive");
        assert_eq!(writes_after(&sim, before), [READ_LINE]);
        w.close().expect("close");
    }

    // ---- AC-AXHK2P.3: exactly that message needs no dialog ----

    #[test]
    fn the_built_message_is_at_most_5v_and_goes_out_unconfirmed() {
        let (sim, w) = in_pd(Some(SimCable::Epr240W));
        let c = w.control();
        assert_eq!(
            c.message_risk(read_message()).expect("risk"),
            RiskClass::AtMost5V
        );
        let before = sim.writes().len();
        let reply = c.send_message(read_message(), false).expect("no dialog");
        assert_eq!(reply.command, READ_LINE);
        assert_eq!(reply.cable.map(|v| v.vid), Some(0x050D));
        assert_eq!(writes_after(&sim, before), [READ_LINE]);
        w.close().expect("close");
    }

    #[test]
    fn the_raw_console_keeps_asking_for_the_same_bytes() {
        let (sim, w) = in_pd(Some(SimCable::Epr240W));
        let c = w.control();
        for line in [
            READ_LINE,
            "pd data=018f1001a800ff",
            " pd data=018F1001A800FF ",
        ] {
            assert_eq!(
                c.console_risk(line.to_owned()).expect("risk"),
                RiskClass::Above5V,
                "{line:?}"
            );
            let before = sim.writes().len();
            let err = c
                .console_send(line.to_owned(), false)
                .expect_err("needs a confirmation");
            assert_eq!(err.kind, ErrorKind::Usage, "{err:?}");
            assert_eq!(writes_after(&sim, before), Vec::<String>::new());
        }
        w.close().expect("close");
    }

    #[test]
    fn only_the_parsed_pd_data_command_can_match() {
        assert!(is_cable_read(&Command::PdData(READ_BYTES.to_vec())));
        for other in [
            Command::Raw(READ_LINE.to_owned()),
            Command::Raw("pd data=018f1001a800ff".to_owned()),
            Command::PdCmd(15),
            Command::PdPdo,
        ] {
            assert!(!is_cable_read(&other), "{other:?}");
        }
    }

    // ---- AC-AXHK2P.9: every variant still needs the confirmation ----

    /// The read with its object replaced by `object`, as bytes.
    fn with_object(object: u32) -> Vec<u8> {
        let mut bytes = READ_BYTES[..3].to_vec();
        bytes.extend(object.to_le_bytes());
        bytes
    }

    /// The read with its header flipped by `mask`.
    fn with_header_flip(mask: u16) -> Vec<u8> {
        let header = u16::from_le_bytes([READ_BYTES[1], READ_BYTES[2]]) ^ mask;
        let mut bytes = vec![READ_BYTES[0]];
        bytes.extend(header.to_le_bytes());
        bytes.extend(&READ_BYTES[3..]);
        bytes
    }

    /// Every byte in turn, SOP targets, every header bit, every VDM command,
    /// and added or removed bytes.
    fn variants() -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        for i in 0..READ_BYTES.len() {
            for mask in [0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0xFF] {
                let mut v = READ_BYTES.to_vec();
                v[i] ^= mask;
                out.push(v);
            }
            // A byte removed.
            let mut v = READ_BYTES.to_vec();
            v.remove(i);
            out.push(v);
            // A byte added before it.
            for added in [0x00, 0xFF] {
                let mut v = READ_BYTES.to_vec();
                v.insert(i, added);
                out.push(v);
            }
        }
        // A byte added at the end, and a whole object added or removed.
        for added in [0x00, 0xFF] {
            let mut v = READ_BYTES.to_vec();
            v.push(added);
            out.push(v);
        }
        let mut two_objects = with_header_flip(0x1000 | 0x2000);
        two_objects.extend([0; 4]);
        out.push(two_objects);
        out.push(vec![0x01, 0x8F, 0x00]);
        // SOP targets: SOP, SOP'', and the debug and unknown codes.
        for sop in [0x00, 0x02, 0x03, 0x04, 0xFF] {
            let mut v = READ_BYTES.to_vec();
            v[0] = sop;
            out.push(v);
        }
        for bit in 0..16 {
            out.push(with_header_flip(1 << bit));
        }
        for command in (0..32u32).filter(|c| *c != 1) {
            out.push(with_object((0xFF00_A801 & !0x1F) | command));
        }
        out
    }

    /// `bytes` as the builder's request, when the builder renders exactly them.
    fn typed(bytes: &[u8]) -> Option<PdMessageRequest> {
        let [sop, lo, hi, rest @ ..] = bytes else {
            return None;
        };
        let sop = match sop {
            0 => MessageSop::Sop,
            1 => MessageSop::SopPrime,
            2 => MessageSop::SopDoublePrime,
            _ => return None,
        };
        if !rest.len().is_multiple_of(4) {
            return None;
        }
        let request = PdMessageRequest {
            sop,
            msg_type: lo & 0x1F,
            data_objects: rest
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| u32::from_le_bytes(*c))
                .collect(),
        };
        let header = u16::from_le_bytes([*lo, *hi]);
        let rendered = lib_message(&request).to_command().ok()?;
        (lib_message(&request).header() == header && rendered == Command::PdData(bytes.to_vec()))
            .then_some(request)
    }

    /// What a session with no offers asks for `request`: the library's own
    /// class, and above 5 V an unconfirmed send writes nothing. True when it
    /// was above 5 V.
    fn keeps_its_class(sim: &SimPdm, c: &TriggerControl, request: PdMessageRequest) -> bool {
        let Ok(command) = lib_message(&request).to_command() else {
            return false;
        };
        assert!(!is_cable_read(&command), "{command:?}");
        let library = pdm::risk::classify(&command, &Capabilities::default());
        assert_eq!(
            c.message_risk(request.clone()).expect("risk"),
            risk_class(library),
            "{request:?}"
        );
        if library != Risk::Above5V {
            return false;
        }
        let before = sim.writes().len();
        let err = c
            .send_message(request.clone(), false)
            .expect_err("needs a confirmation");
        assert_eq!(err.kind, ErrorKind::Usage, "{request:?}: {err:?}");
        assert_eq!(
            writes_after(sim, before),
            Vec::<String>::new(),
            "{request:?}"
        );
        true
    }

    #[test]
    fn no_variant_of_the_bytes_is_the_read() {
        let variants = variants();
        assert!(variants.len() > 100, "{}", variants.len());
        for v in variants {
            assert_ne!(v, READ_BYTES, "not a variant");
            assert!(!is_cable_read(&Command::PdData(v.clone())), "{v:02X?}");
        }
        assert!(!is_cable_read(&Command::PdData(Vec::new())));
    }

    #[test]
    fn every_variant_of_the_bytes_needs_the_confirmation() {
        // The object removed and the count cleared: a whole control message
        // of its own, which keeps its own class. The only one.
        let control = [0x01, 0x8F, 0x00];
        for v in variants().into_iter().filter(|v| v != &control) {
            let class = pdm::risk::classify(&Command::PdData(v.clone()), &Capabilities::default());
            assert_eq!(class, Risk::Above5V, "{v:02X?}");
        }
    }

    #[test]
    fn every_variant_the_builder_can_make_keeps_the_librarys_class() {
        let (sim, w) = worker_with(Some(SimCable::Epr240W));
        let c = w.control();
        let asked = variants()
            .iter()
            .filter_map(|v| typed(v))
            .filter(|r| keeps_its_class(&sim, &c, r.clone()))
            .count();
        assert!(asked >= 32, "only {asked} variants were above 5 V");
        w.close().expect("close");
    }

    #[test]
    fn every_object_bit_and_message_type_keeps_the_librarys_class() {
        let (sim, w) = worker_with(Some(SimCable::Epr240W));
        let c = w.control();
        let mut asked = 0;
        for bit in 0..32 {
            let mut request = read_message();
            request.data_objects[0] ^= 1 << bit;
            assert!(keeps_its_class(&sim, &c, request), "bit {bit}");
            asked += 1;
        }
        for msg_type in (0..32).filter(|t| *t != 15) {
            let mut request = read_message();
            request.msg_type = msg_type;
            asked += usize::from(keeps_its_class(&sim, &c, request));
        }
        for sop in [MessageSop::Sop, MessageSop::SopDoublePrime] {
            let mut request = read_message();
            request.sop = sop;
            assert!(keeps_its_class(&sim, &c, request), "{sop:?}");
            asked += 1;
        }
        let mut two = read_message();
        two.data_objects.push(0);
        assert!(keeps_its_class(&sim, &c, two), "an added object");
        let mut none = read_message();
        none.data_objects.clear();
        keeps_its_class(&sim, &c, none);
        assert!(asked >= 34, "only {asked} variants were above 5 V");
        w.close().expect("close");
    }

    #[test]
    fn a_session_with_offers_asks_the_same_as_the_console() {
        let (sim, w) = in_pd(Some(SimCable::Epr240W));
        let c = w.control();
        for request in variants().iter().filter_map(|v| typed(v)) {
            let line = command_text(&lib_message(&request).to_command().expect("it builds"));
            let library = c.console_risk(line.clone()).expect("risk");
            assert_eq!(
                c.message_risk(request.clone()).expect("risk"),
                library,
                "{line}"
            );
            if library == RiskClass::Above5V {
                let before = sim.writes().len();
                c.send_message(request, false)
                    .expect_err("needs a confirmation");
                assert_eq!(writes_after(&sim, before), Vec::<String>::new(), "{line}");
            }
        }
        w.close().expect("close");
    }

    #[test]
    fn every_variant_on_the_raw_console_keeps_the_librarys_class() {
        let (sim, w) = worker_with(Some(SimCable::Epr240W));
        let c = w.control();
        for v in variants() {
            let hex: String = v.iter().map(|b| format!("{b:02X}")).collect();
            let line = format!("pd data={hex}");
            let library = pdm::risk::classify(&Command::parse(&line), &Capabilities::default());
            assert_eq!(
                c.console_risk(line.clone()).expect("risk"),
                risk_class(library),
                "{line}"
            );
            if library == Risk::Above5V {
                let before = sim.writes().len();
                c.console_send(line.clone(), false)
                    .expect_err("needs a confirmation");
                assert_eq!(writes_after(&sim, before), Vec::<String>::new(), "{line}");
            }
        }
        w.close().expect("close");
    }
}

/// FR DGSQ6S (AC .9, .10, .11): failed requests, a meter whose replies lag
/// one command behind, and the stream pausing while the automatic list runs.
#[cfg(test)]
mod sync_tests {
    use std::collections::{HashMap, VecDeque};
    use std::io::{Read, Write};
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use powerz_protocol::session::SystemClock;
    use powerz_protocol::transport::{self, Transport};
    use powerz_sim::{Persona, Scenario, SimCharger, SimClock, SimDevice, SimPdm, SimPdmPort};

    use super::*;
    use crate::devices::{DeviceListing, DeviceManager, Opener};
    use crate::ui_error::ErrorKind;
    use crate::worker::{BackendEvent, GapKind, Status};

    const OPEN_REPLY: &[u8] = b"pdm mode entry\nver1.0\n";

    fn hex(s: &str) -> Vec<u8> {
        let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    /// `entry pd` on the 140 W charger, SPR list only. [device]
    fn entry_pd_reply() -> Vec<u8> {
        let mut b = b"ok\ncc1 attach\n".to_vec();
        b.extend(hex(
            "70646f3a 34 0b 2c918109 2cd10200 2cb10400 d6410600
             00000000 00000000 00000000 00000000 00000000 00000000 00000000 0a",
        ));
        b.extend(b"ready:5191mV,0mA\n");
        b
    }

    #[derive(Default)]
    struct Script {
        replies: HashMap<Vec<u8>, Vec<u8>>,
        writes: Vec<String>,
        pending: VecDeque<u8>,
    }

    /// A serial port answering exact command bytes with scripted replies.
    #[derive(Clone, Default)]
    struct Scripted(Arc<Mutex<Script>>);

    impl Scripted {
        fn on(self, cmd: &str, reply: &[u8]) -> Scripted {
            self.0
                .lock()
                .unwrap()
                .replies
                .insert(cmd.as_bytes().to_vec(), reply.to_vec());
            self
        }
        fn writes(&self) -> Vec<String> {
            self.0.lock().unwrap().writes.clone()
        }
        /// Bytes waiting on the port that nothing asked for.
        fn arrive(&self, bytes: &[u8]) {
            self.0.lock().unwrap().pending.extend(bytes);
        }
    }

    impl Read for Scripted {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let mut s = self.0.lock().unwrap();
            if s.pending.is_empty() {
                return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "idle"));
            }
            let n = buf.len().min(s.pending.len());
            for b in buf.iter_mut().take(n) {
                *b = s.pending.pop_front().unwrap();
            }
            Ok(n)
        }
    }

    impl Write for Scripted {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let mut s = self.0.lock().unwrap();
            s.writes
                .push(String::from_utf8_lossy(buf).trim().to_owned());
            if let Some(r) = s.replies.get(buf).cloned() {
                s.pending.extend(r);
            }
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// The 140 W charger (SPR) behind the meter, with `extra` replacing replies.
    fn charger(extra: &[(&str, &[u8])]) -> Scripted {
        let mut port = Scripted::default()
            .on("pdm open", OPEN_REPLY)
            .on("pdm close", b"ok\n")
            .on("reset", b"ok\n")
            .on("entry pd", &entry_pd_reply())
            .on("pdm parm", b"type=0,em=2,sink=0\n")
            .on("pd req=1", b"req:5000mV,3000mA\nready:5180mV,0mA\n")
            .on("pd req=2", b"req:9000mV,3000mA\nready:8554mV,0mA\n");
        for (cmd, reply) in extra {
            port = port.on(cmd, reply);
        }
        port
    }

    fn spawn(port: &Scripted) -> TriggerWorker {
        TriggerWorker::spawn(port.clone(), SimClock::new(1_760_000_000_000))
            .expect("the session opens")
    }

    fn pd(position: u8) -> TriggerRequest {
        TriggerRequest::Pd {
            position,
            voltage_mv: None,
            current_ma: None,
        }
    }

    fn text(e: &UiError) -> String {
        format!("{} {}", e.message, e.hint.clone().unwrap_or_default()).to_lowercase()
    }

    fn says_power_cycle(e: &UiError) -> bool {
        let t = text(e);
        t.contains("power cycle") || t.contains("power-cycle")
    }

    // ---- AC-DGSQ6S.9: a failed request says why ----

    #[test]
    fn an_error_reply_to_a_request_is_a_failure_with_its_reason() {
        // AC-DGSQ6S.9: `req error 252` answers `pd req=2`.
        let port = charger(&[("pd req=2", b"req error 252\n")]);
        let w = spawn(&port);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        let err = c.request(pd(2), true).expect_err("the charger refused");
        assert_ne!(
            err.kind,
            ErrorKind::Timeout,
            "the error reply was waited out: {err:?}"
        );
        assert!(text(&err).contains("252"), "the reason is lost: {err:?}");
        // Its own reply, so the session stays usable.
        c.request(pd(1), false).expect("pd req=1 still works");
    }

    // ---- AC-DGSQ6S.10: a reply that belongs to another command ----

    #[test]
    fn a_late_reply_to_another_command_stops_the_session_with_a_power_cycle_error() {
        // AC-DGSQ6S.10 [device]: `pdm parm` answered by `req error 252`.
        let port = charger(&[("pdm parm", b"req error 252\n")]);
        let w = spawn(&port);
        let c = w.control();
        let err = c
            .list(TriggerProtocol::Pd)
            .expect_err("the settings reply was not the settings");
        assert_eq!(err.kind, ErrorKind::OutOfSync, "{err:?}");
        assert!(says_power_cycle(&err), "{err:?}");
    }

    #[test]
    fn after_out_of_sync_requests_are_refused_and_nothing_is_written() {
        // AC-DGSQ6S.10: requests stay off until the session is reopened.
        let port = charger(&[("pdm parm", b"req error 252\n")]);
        let w = spawn(&port);
        let c = w.control();
        assert!(c.list(TriggerProtocol::Pd).is_err());
        let before = port.writes().len();
        let err = c.request(pd(1), false).expect_err("refused");
        assert_eq!(err.kind, ErrorKind::OutOfSync, "{err:?}");
        assert!(says_power_cycle(&err), "{err:?}");
        let err = c.list(TriggerProtocol::Qc).expect_err("refused");
        assert_eq!(err.kind, ErrorKind::OutOfSync, "{err:?}");
        assert_eq!(
            port.writes().len(),
            before,
            "written after out-of-sync: {:?}",
            &port.writes()[before..]
        );
    }

    // ---- AC-AXHK2P.2, .4: the cable read on a meter that misbehaves ----

    const READ_LINE: &str = "pd data=018F1001A800FF";

    #[test]
    fn out_of_sync_a_cable_read_is_refused_and_writes_nothing() {
        let port = charger(&[("pd req=2", b"type=0,em=2,sink=0\n")]);
        let w = spawn(&port);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        c.request(pd(2), true).expect_err("not a req: reply");
        let before = port.writes().len();
        let Err(err) = c.cable_read() else {
            panic!("the read was not refused");
        };
        assert_eq!(err.kind, ErrorKind::OutOfSync, "{err:?}");
        assert!(says_power_cycle(&err), "{err:?}");
        assert_eq!(port.writes()[before..], [] as [String; 0]);
    }

    #[test]
    fn a_cable_read_nobody_answers_sends_one_line() {
        let port = charger(&[]);
        let w = spawn(&port);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        let before = port.writes().len();
        assert!(matches!(c.cable_read(), Ok(CableRead::NoAnswer)));
        assert_eq!(port.writes()[before..], [READ_LINE]);
        w.close().expect("close");
    }

    #[test]
    fn unrelated_lines_during_the_wait_are_no_answer_and_no_second_send() {
        let port = charger(&[(READ_LINE, b"ok\nready:5180mV,0mA\n")]);
        let w = spawn(&port);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        port.arrive(b"ready:5180mV,0mA\n");
        let before = port.writes().len();
        assert!(matches!(c.cable_read(), Ok(CableRead::NoAnswer)));
        assert_eq!(port.writes()[before..], [READ_LINE]);
        w.close().expect("close");
    }

    #[test]
    fn the_cables_ack_on_the_port_answers_the_read() {
        // Header 0x518F, then the session-2 cable's objects. [device]
        let port = charger(&[(
            READ_LINE,
            b"pd data=018F5141A800FF0D0560184D4000000000548240260A00\n",
        )]);
        let w = spawn(&port);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        let before = port.writes().len();
        let Ok(CableRead::Found(view)) = c.cable_read() else {
            panic!("the cable answered");
        };
        assert_eq!(view.vid, 0x050D);
        assert_eq!(view.speed.as_deref(), Some("USB 2.0"));
        assert_eq!(port.writes()[before..], [READ_LINE]);
        w.close().expect("close");
    }

    #[test]
    fn a_request_answered_by_another_commands_reply_is_out_of_sync() {
        // AC-DGSQ6S.10: `pd req=2` answered by the late `type=...` of `pdm parm`.
        let port = charger(&[("pd req=2", b"type=0,em=2,sink=0\n")]);
        let w = spawn(&port);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        let err = c.request(pd(2), true).expect_err("not a req: reply");
        assert_eq!(err.kind, ErrorKind::OutOfSync, "{err:?}");
        assert!(says_power_cycle(&err), "{err:?}");
    }

    #[test]
    fn the_out_of_sync_kind_serializes_for_the_ui() {
        // AC-DGSQ6S.10: the panel branches on the kind.
        let json = serde_json::to_value(ErrorKind::OutOfSync).unwrap();
        assert_eq!(json, serde_json::json!("out_of_sync"));
    }

    // ---- AC-DGSQ6R.7: detection checks the meter is in step after a cut list ----

    /// Device trace 2026-10-23 (Apple 140 W): `entry bc` / `entry apple` →
    /// `fail`, the list stops after `vfcp:`. [device]
    const CUT_LIST: &[u8] = b"starting\nbc1.2:true 5194mV,0mA\nufcs:null\nvfcp:\n";

    fn detecting(extra: &[(&str, &[u8])]) -> Scripted {
        let mut all: Vec<(&str, &[u8])> = vec![
            ("entry bc", b"fail\n"),
            ("entry apple", b"fail\n"),
            ("entry list", CUT_LIST),
        ];
        all.extend_from_slice(extra);
        charger(&all)
    }

    /// Commands written after `entry list`.
    fn after_entry_list(port: &Scripted) -> Vec<String> {
        let w = port.writes();
        let at = w
            .iter()
            .position(|c| c == "entry list")
            .unwrap_or_else(|| panic!("entry list not sent: {w:?}"));
        w[at + 1..].to_vec()
    }

    #[test]
    fn an_incomplete_detection_checks_with_pdm_parm_right_after_reset() {
        // AC-DGSQ6R.7: in step → the partial detection comes back.
        let port = detecting(&[]);
        let w = spawn(&port);
        let d = w.control().detect().expect("in step");
        assert!(!d.complete);
        assert_eq!(d.entries.len(), 3, "{d:?}");
        assert_eq!(after_entry_list(&port), ["reset", "pdm parm"]);
    }

    #[test]
    fn a_lagging_meter_after_the_list_is_out_of_sync_with_a_power_cycle_error() {
        // AC-DGSQ6R.7: `reset` unanswered, then `pdm parm` gets the late `ok`.
        let port = detecting(&[("reset", b""), ("pdm parm", b"ok\n")]);
        let w = spawn(&port);
        let err = w.control().detect().expect_err("lagging meter");
        assert_eq!(err.kind, ErrorKind::OutOfSync, "{err:?}");
        assert!(says_power_cycle(&err), "{err:?}");
        assert_eq!(after_entry_list(&port), ["reset", "pdm parm"]);
    }

    #[test]
    fn after_a_failed_detection_check_nothing_more_is_sent() {
        // AC-DGSQ6R.7: listing, requests and detection are all refused.
        let port = detecting(&[("pdm parm", b"req error 252\n")]);
        let w = spawn(&port);
        let c = w.control();
        let err = c.detect().expect_err("lagging meter");
        assert_eq!(err.kind, ErrorKind::OutOfSync, "{err:?}");
        let before = port.writes().len();
        for err in [
            c.list(TriggerProtocol::Pd).expect_err("refused"),
            c.request(pd(1), false).expect_err("refused"),
        ] {
            assert_eq!(err.kind, ErrorKind::OutOfSync, "{err:?}");
        }
        let again = c.detect().expect_err("refused");
        assert_eq!(again.kind, ErrorKind::OutOfSync, "{again:?}");
        assert_eq!(
            port.writes().len(),
            before,
            "written after out-of-sync: {:?}",
            &port.writes()[before..]
        );
    }

    // ---- AC-DGSQ6S.11: the stream pauses while the automatic list runs ----

    /// Bulk message types the stream sends, by time.
    type Log = Arc<Mutex<Vec<(Instant, u8)>>>;

    const CONNECT: u8 = 0x02;
    const GET_DATA: u8 = 0x0C;

    /// The demo meter's bulk pipe, logging the type of every request.
    struct Logged {
        inner: SimDevice,
        log: Log,
    }

    impl Transport for Logged {
        fn transact(
            &mut self,
            request: &[u8],
            timeout: Duration,
        ) -> Result<Vec<u8>, transport::Error> {
            if let Some(&b) = request.first() {
                self.log.lock().unwrap().push((Instant::now(), b & 0x7F));
            }
            self.inner.transact(request, timeout)
        }
        fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, transport::Error> {
            self.inner.read(timeout)
        }
        fn recover(&mut self) -> Result<(), transport::Error> {
            self.inner.recover()
        }
    }

    struct LoggingOpener(Log);

    impl Opener for LoggingOpener {
        type Transport = Logged;
        fn open(&self, device: &DeviceListing) -> Result<Logged, transport::Error> {
            let scenario = device
                .demo_scenario
                .as_deref()
                .and_then(Scenario::from_name)
                .ok_or(transport::Error::NotFound)?;
            Ok(Logged {
                inner: SimDevice::new(Persona::Bulk, scenario),
                log: Arc::clone(&self.0),
            })
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum ListEnd {
        Complete,
        Incomplete,
        Error,
    }

    /// The simulated meter's serial port, except that the automatic list
    /// takes `HOLD` of real time and then ends the way `end` says.
    struct ListGate {
        port: SimPdmPort,
        end: ListEnd,
        armed: bool,
        own: VecDeque<u8>,
        /// When `entry list` was written and when the hold ended.
        window: Arc<Mutex<Option<(Instant, Instant)>>>,
    }

    const HOLD: Duration = Duration::from_millis(600);

    impl Read for ListGate {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.armed {
                self.armed = false;
                std::thread::sleep(HOLD);
                let mut window = self.window.lock().unwrap();
                if let Some((start, _)) = *window {
                    *window = Some((start, Instant::now()));
                }
                drop(window);
                if self.end == ListEnd::Error {
                    return Err(std::io::Error::other("serial fault"));
                }
            }
            if !self.own.is_empty() {
                let n = buf.len().min(self.own.len());
                for b in buf.iter_mut().take(n) {
                    *b = self.own.pop_front().unwrap();
                }
                return Ok(n);
            }
            self.port.read(buf)
        }
    }

    impl Write for ListGate {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if buf.starts_with(b"entry list") {
                let now = Instant::now();
                *self.window.lock().unwrap() = Some((now, now));
                self.armed = true;
                if self.end == ListEnd::Complete {
                    self.own
                        .extend(b"starting\nqc2.0:5V,9V,12V\nufcs:null\n\ndone\n".iter());
                    return Ok(buf.len());
                }
            }
            self.port.write(buf)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            self.port.flush()
        }
    }

    fn eventually(what: &str, mut f: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !f() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn count(log: &Log, ty: u8, after: Option<Instant>) -> usize {
        log.lock()
            .unwrap()
            .iter()
            .filter(|(t, k)| *k == ty && after.is_none_or(|a| *t > a))
            .count()
    }

    /// Connect the demo meter at 50 SPS, open a trigger session whose list
    /// ends as `end`, detect, and check the stream around the list.
    fn stream_pauses_during_the_list(end: ListEnd) {
        let log: Log = Arc::default();
        let mut m = DeviceManager::new(
            || Ok(Vec::new()),
            LoggingOpener(Arc::clone(&log)),
            SystemClock,
        );
        m.connect_with_stop(
            "demo:load-5v-1a",
            50,
            |_e: BackendEvent| {},
            Arc::new(AtomicBool::new(false)),
        )
        .expect("the demo device connects");
        eventually("the stream polls", || count(&log, GET_DATA, None) > 0);

        let clock = SimClock::new(1_760_000_000_000);
        let sim = SimPdm::new(SimCharger::Epr140W, clock.clone());
        let window = Arc::new(Mutex::new(None));
        let port = ListGate {
            port: sim.open_port(),
            end,
            armed: false,
            own: VecDeque::new(),
            window: Arc::clone(&window),
        };
        let w = TriggerWorker::spawn(port, clock).expect("the session opens");
        m.install_trigger(w).expect("install");
        let connects = count(&log, CONNECT, None);

        let result = m.trigger_control().expect("open").detect();
        let returned = Instant::now();
        match end {
            ListEnd::Complete => assert!(result.expect("detect").complete),
            ListEnd::Incomplete => assert!(!result.expect("detect").complete),
            ListEnd::Error => assert!(result.is_err(), "{result:?}"),
        }
        let (start, stop) = window.lock().unwrap().expect("entry list was sent");
        let during: Vec<u8> = log
            .lock()
            .unwrap()
            .iter()
            .filter(|(t, k)| *t >= start && *t <= stop && (*k == GET_DATA || *k == CONNECT))
            .map(|(_, k)| *k)
            .collect();
        assert!(
            during.is_empty(),
            "{end:?}: the stream sent {} requests while the list ran: {during:02x?}",
            during.len()
        );
        eventually("polling to resume", || {
            count(&log, GET_DATA, Some(returned)) > 0
        });
        assert_eq!(
            count(&log, CONNECT, None),
            connects,
            "{end:?}: the meter was reconnected"
        );
        m.disconnect().expect("disconnect");
    }

    #[test]
    fn the_stream_pauses_during_a_complete_list_and_resumes() {
        // AC-DGSQ6S.11
        stream_pauses_during_the_list(ListEnd::Complete);
    }

    #[test]
    fn the_stream_pauses_during_an_incomplete_list_and_resumes() {
        // AC-DGSQ6S.11: the list stalls and is ended early with `reset`.
        stream_pauses_during_the_list(ListEnd::Incomplete);
    }

    #[test]
    fn the_stream_resumes_when_the_list_fails() {
        // AC-DGSQ6S.11: the list ends with an error.
        stream_pauses_during_the_list(ListEnd::Error);
    }

    #[test]
    fn polling_is_released_even_when_the_hold_answer_timed_out() {
        // AC-DGSQ6S.11: a slow worker applies the hold after the wait gave
        // up; the release must still follow it, or polling stays held.
        let (stream, seen) = crate::worker::late_hold_control();
        let held = PollingHeld::hold(Some(&stream));
        assert!(held.is_some(), "the guard exists whatever the answer");
        drop(held);
        eventually("the release to be applied", || {
            seen.lock().unwrap().len() == 2
        });
        assert_eq!(*seen.lock().unwrap(), [true, false]);
    }

    // ---- AC-DGSQ6S.12: the stream survives the meter's backlog ----

    /// When the backlog reply was cut, and the requests after it.
    #[derive(Default)]
    struct BacklogState {
        last_request: Option<Instant>,
        cut_at: Option<Instant>,
        polls_after_cut: usize,
    }

    /// The demo meter's bulk pipe, except that the first queue poll after a
    /// pause of at least `PAUSE` arrives cut short, as FW 2.0.6 sends its
    /// backlog [device]: the section still declares every record, but the
    /// tail of the last one is missing.
    struct Backlog {
        inner: SimDevice,
        state: Arc<Mutex<BacklogState>>,
    }

    const PAUSE: Duration = Duration::from_millis(400);

    impl Transport for Backlog {
        fn transact(
            &mut self,
            request: &[u8],
            timeout: Duration,
        ) -> Result<Vec<u8>, transport::Error> {
            let mut reply = self.inner.transact(request, timeout)?;
            let now = Instant::now();
            let queue_poll = request.first().map(|b| b & 0x7F) == Some(GET_DATA)
                && request.get(2..4) == Some(&[0x04, 0x00][..]);
            let mut s = self.state.lock().unwrap();
            let paused = s
                .last_request
                .is_some_and(|t| now.duration_since(t) >= PAUSE);
            s.last_request = Some(now);
            if !queue_poll {
                return Ok(reply);
            }
            if s.cut_at.is_some() {
                s.polls_after_cut += 1;
                return Ok(reply);
            }
            // Section header at offset 4: att 0x002, chunk in bits 16..21.
            if paused && reply.len() >= 8 && reply[4] == 0x02 {
                let chunk = reply[6] & 0x3F;
                if chunk >= 1 {
                    reply.truncate(reply.len() - 12);
                } else {
                    reply[6] = (reply[6] & !0x3F) | 3;
                }
                s.cut_at = Some(now);
            }
            Ok(reply)
        }
        fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, transport::Error> {
            self.inner.read(timeout)
        }
        fn recover(&mut self) -> Result<(), transport::Error> {
            self.inner.recover()
        }
    }

    struct BacklogOpener(Arc<Mutex<BacklogState>>);

    impl Opener for BacklogOpener {
        type Transport = Backlog;
        fn open(&self, device: &DeviceListing) -> Result<Backlog, transport::Error> {
            let scenario = device
                .demo_scenario
                .as_deref()
                .and_then(Scenario::from_name)
                .ok_or(transport::Error::NotFound)?;
            Ok(Backlog {
                inner: SimDevice::new(Persona::Bulk, scenario),
                state: Arc::clone(&self.0),
            })
        }
    }

    #[test]
    fn a_short_backlog_reply_after_the_list_keeps_the_stream_and_the_trigger_session() {
        // AC-DGSQ6S.12 [device]: the first queue poll after the automatic
        // list arrives shorter than it declares.
        let state = Arc::new(Mutex::new(BacklogState::default()));
        let events: Arc<Mutex<Vec<(Instant, BackendEvent)>>> = Arc::default();
        let sink_events = Arc::clone(&events);
        let mut m = DeviceManager::new(
            || Ok(Vec::new()),
            BacklogOpener(Arc::clone(&state)),
            SystemClock,
        );
        m.connect_with_stop(
            "demo:load-5v-1a",
            50,
            move |e: BackendEvent| sink_events.lock().unwrap().push((Instant::now(), e)),
            Arc::new(AtomicBool::new(false)),
        )
        .expect("the demo device connects");
        eventually("the stream polls", || {
            state.lock().unwrap().last_request.is_some()
        });

        let clock = SimClock::new(1_760_000_000_000);
        let sim = SimPdm::new(SimCharger::Epr140W, clock.clone());
        let port = ListGate {
            port: sim.open_port(),
            end: ListEnd::Complete,
            armed: false,
            own: VecDeque::new(),
            window: Arc::new(Mutex::new(None)),
        };
        let w = TriggerWorker::spawn(port, clock).expect("the session opens");
        m.install_trigger(w).expect("install");
        assert!(m.trigger_control().expect("open").detect().is_ok());

        eventually("the backlog reply to be cut", || {
            state.lock().unwrap().cut_at.is_some()
        });
        let cut_at = state.lock().unwrap().cut_at.expect("cut");
        eventually("polls to carry on after the cut", || {
            state.lock().unwrap().polls_after_cut >= 3
        });
        eventually("samples after the cut", || {
            events.lock().unwrap().iter().any(|(t, e)| {
                *t > cut_at && matches!(e, BackendEvent::Batch(b) if !b.samples.is_empty())
            })
        });

        let seen = events.lock().unwrap().clone();
        let lost: Vec<&BackendEvent> = seen
            .iter()
            .map(|(_, e)| e)
            .filter(|e| matches!(e, BackendEvent::Status(Status::Disconnected { .. })))
            .collect();
        assert!(
            lost.is_empty(),
            "AC-DGSQ6S.12: the short reply ended the session: {lost:?}"
        );
        assert!(
            seen.iter().any(|(t, e)| *t > cut_at
                && matches!(e, BackendEvent::Gap(g) if g.kind == GapKind::Stream)),
            "AC-DGSQ6S.12: the lost samples were not reported as a gap"
        );
        assert!(
            m.trigger_open(),
            "AC-DGSQ6S.12: the trigger session was closed"
        );
        assert!(m.trigger_control().is_ok());
        m.disconnect().expect("disconnect");
    }

    // ---- AC-DGSQ6S.10: the way back to 5 V stays open out of sync ----

    #[test]
    fn out_of_sync_after_a_9v_contract_back_to_5v_sends_a_hard_reset_then_closes() {
        let port = charger(&[]);
        let w = spawn(&port);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        c.request(pd(2), true).expect("9 V");
        // From now on the meter lags: `pdm parm` gets `pd req=`'s late error.
        let port = port.on("pdm parm", b"req error 252\n");
        let err = c.list(TriggerProtocol::Pd).expect_err("out of sync");
        assert_eq!(err.kind, ErrorKind::OutOfSync, "{err:?}");
        let before = port.writes().len();
        let err = c.back_to_5v().expect_err("VBUS is not confirmed");
        assert_eq!(err.kind, ErrorKind::OutOfSync, "{err:?}");
        assert_eq!(&port.writes()[before..], ["pd hrst", "pdm close"]);
        let t = text(&err);
        assert!(t.contains("above 5 v"), "{err:?}");
        assert!(t.contains("hard reset"), "{err:?}");
        assert!(t.contains("unplug the charger"), "{err:?}");
        assert!(says_power_cycle(&err), "{err:?}");
        // Sent once: Back to 5 V again and closing send nothing more.
        let again = c.back_to_5v().expect_err("still out of sync");
        assert!(text(&again).contains("above 5 v"), "{again:?}");
        drop(c);
        let _ = w.close();
        assert_eq!(&port.writes()[before..], ["pd hrst", "pdm close"]);
    }

    #[test]
    fn a_9v_request_answered_out_of_sync_is_ended_with_a_hard_reset() {
        // The request was written; its reply was another command's.
        let port = charger(&[("pd req=2", b"type=0,em=2,sink=0\n")]);
        let w = spawn(&port);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        let err = c.request(pd(2), true).expect_err("out of sync");
        assert_eq!(err.kind, ErrorKind::OutOfSync, "{err:?}");
        let before = port.writes().len();
        let err = c.back_to_5v().expect_err("VBUS is not confirmed");
        assert!(text(&err).contains("above 5 v"), "{err:?}");
        assert_eq!(&port.writes()[before..], ["pd hrst", "pdm close"]);
    }

    // ---- AC-AXHK2N.2, .4, .6, .8: failures and changed offers while armed ----

    const NOT_ARMED: &str = "still armed";

    /// `entry pd` on a charger whose four SPR offers are `pdos`.
    fn entry_pd_offering(pdos: [u32; 4]) -> Vec<u8> {
        let mut b = b"ok\ncc1 attach\npdo:4\x0b".to_vec();
        for p in pdos {
            b.extend(p.to_le_bytes());
        }
        b.extend([0u8; 7 * 4]);
        b.extend(b"\nready:5191mV,0mA\n");
        b
    }

    /// The offers `entry_pd_reply` gives, with position 2 at 12 V instead of 9 V.
    const OFFERS_12V_AT_2: [u32; 4] = [0x0981_912C, 0x0003_C12C, 0x0004_B12C, 0x0006_41D6];

    /// The 140 W charger behind the meter, armed at 9 V.
    fn armed(extra: &[(&str, &[u8])]) -> (Scripted, TriggerWorker, TriggerControl) {
        let port = charger(&[("entry qc", b"ok\nready\n")]);
        let port = extra
            .iter()
            .fold(port, |port, (cmd, reply)| port.on(cmd, reply));
        let w = spawn(&port);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        c.request(pd(2), Confirm::YesRemember).expect("9 V");
        assert!(c.armed().expect("armed"));
        (port, w, c)
    }

    #[test]
    fn a_failed_arming_request_does_not_arm() {
        let port = charger(&[("pd req=2", b"req error 252\n")]);
        let w = spawn(&port);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        c.request(pd(2), Confirm::YesRemember)
            .expect_err("the charger refused");
        assert!(port.writes().iter().any(|w| w == "pd req=2"));
        assert!(!c.armed().expect("armed"), "armed by a failed request");
        w.close().expect("close");
    }

    #[test]
    fn a_request_the_charger_refuses_disarms() {
        let (port, w, c) = armed(&[("pd req=3", b"req error 252\n")]);
        let err = c.request(pd(3), false).expect_err("the charger refused");
        assert_eq!(err.kind, ErrorKind::Protocol, "{err:?}");
        assert!(port.writes().iter().any(|w| w == "pd req=3"));
        assert!(!c.armed().expect("armed"), "{NOT_ARMED}");
        drop(c);
        let _ = w.close();
    }

    #[test]
    fn a_request_nobody_answers_disarms() {
        // No reply is scripted for `pd req=3`: written, then a timeout.
        let (port, w, c) = armed(&[]);
        c.request(pd(3), false).expect_err("no reply");
        assert!(port.writes().iter().any(|w| w == "pd req=3"));
        assert!(!c.armed().expect("armed"), "{NOT_ARMED}");
        drop(c);
        let _ = w.close();
    }

    #[test]
    fn a_request_answered_out_of_sync_disarms() {
        let (port, w, c) = armed(&[("pd req=3", b"type=0,em=2,sink=0\n")]);
        let err = c.request(pd(3), false).expect_err("out of sync");
        assert_eq!(err.kind, ErrorKind::OutOfSync, "{err:?}");
        assert!(port.writes().iter().any(|w| w == "pd req=3"));
        assert!(!c.armed().expect("armed"), "{NOT_ARMED}");
        let _ = c.back_to_5v();
        drop(c);
        let _ = w.close();
    }

    #[test]
    fn back_to_5v_through_a_hard_reset_disarms() {
        // `pd req=1` fails, so the way back falls to the Hard Reset.
        let (port, w, c) = armed(&[
            ("pd req=1", b"req error 252\n"),
            ("pd hrst", &entry_pd_reply()[b"ok\ncc1 attach\n".len()..]),
        ]);
        c.back_to_5v().expect("back through the Hard Reset");
        assert!(port.writes().iter().any(|w| w == "pd hrst"));
        assert!(!c.armed().expect("armed"), "{NOT_ARMED}");
        drop(c);
        let _ = w.close();
    }

    #[test]
    fn back_to_5v_that_fails_out_of_sync_still_disarms() {
        let (port, w, c) = armed(&[]);
        let port = port.on("pdm parm", b"req error 252\n");
        let err = c.list(TriggerProtocol::Pd).expect_err("out of sync");
        assert_eq!(err.kind, ErrorKind::OutOfSync, "{err:?}");
        c.back_to_5v().expect_err("VBUS is not confirmed");
        assert!(port.writes().iter().any(|w| w == "pd hrst"));
        assert!(!c.armed().expect("armed"), "{NOT_ARMED}");
        drop(c);
        let _ = w.close();
    }

    #[test]
    fn listing_changed_offers_disarms() {
        let (port, w, c) = armed(&[]);
        let _port = port.on("entry pd", &entry_pd_offering(OFFERS_12V_AT_2));
        c.list(TriggerProtocol::Pd).expect("list pd");
        assert!(!c.armed().expect("armed"), "{NOT_ARMED}");
        w.close().expect("close");
    }

    #[test]
    fn a_console_entry_with_changed_offers_disarms() {
        // `entry pd` is itself a console line that is not read-only, so
        // this also holds without the offers check.
        let (port, w, c) = armed(&[]);
        let _port = port.on("entry pd", &entry_pd_offering(OFFERS_12V_AT_2));
        c.console_send("entry pd".to_owned(), false).expect("sent");
        assert!(!c.armed().expect("armed"), "{NOT_ARMED}");
        w.close().expect("close");
    }

    #[test]
    fn an_armed_request_whose_offer_changed_is_refused_not_sent() {
        let (port, w, c) = armed(&[]);
        // QC entered last, so the request enters PD again and sees the change.
        c.list(TriggerProtocol::Qc).expect("list qc");
        let port = port.on("entry pd", &entry_pd_offering(OFFERS_12V_AT_2));
        let before = port.writes().len();
        c.request(pd(2), false).expect_err("the offer changed");
        let writes = port.writes()[before..].to_vec();
        assert!(writes.iter().any(|w| w == "entry pd"), "{writes:?}");
        assert!(!writes.iter().any(|w| w == "pd req=2"), "{writes:?}");
        assert!(!c.armed().expect("armed"), "{NOT_ARMED}");
        drop(c);
        let _ = w.close();
    }

    /// `armed`, then another charger's `pdo:` block arrives unasked, with the
    /// reply to a read-only line that keeps it armed.
    fn armed_then_an_unasked_block() -> (Scripted, TriggerWorker, TriggerControl) {
        let (port, w, c) = armed(&[]);
        let reply = [
            entry_pd_offering(OFFERS_12V_AT_2),
            b"type=0,em=2,sink=0\n".to_vec(),
        ]
        .concat();
        let port = port.on("pdm parm", &reply);
        c.console_send("pdm parm".to_owned(), false).expect("sent");
        (port, w, c)
    }

    #[test]
    fn an_unasked_block_makes_the_next_risk_ask_again() {
        let (_port, w, c) = armed_then_an_unasked_block();
        assert_eq!(c.risk(pd(2)).expect("risk"), RiskClass::Above5VRememberable);
        assert!(!c.armed().expect("armed"), "{NOT_ARMED}");
        drop(c);
        let _ = w.close();
    }

    #[test]
    fn an_unasked_block_refuses_the_held_request_with_nothing_sent() {
        let (port, w, c) = armed_then_an_unasked_block();
        let before = port.writes().len();
        c.request(pd(2), false).expect_err("the offers changed");
        assert_eq!(port.writes()[before..], [] as [String; 0]);
        assert!(!c.armed().expect("armed"), "{NOT_ARMED}");
        drop(c);
        let _ = w.close();
    }

    #[test]
    fn a_charger_with_no_pd_after_a_collapse_is_no_dead_end() {
        let (port, w, c) = armed(&[]);
        let vbus = Arc::new(Mutex::new(Some(0.0)));
        let reading = Arc::clone(&vbus);
        // The session has not seen VBUS rise yet, so show it first.
        *vbus.lock().unwrap() = Some(9.0);
        w.watch_vbus(fresh(move || *reading.lock().unwrap()));
        c.request(pd(2), false).expect("armed, 9 V");
        *vbus.lock().unwrap() = Some(0.0);
        // Only the outcome matters here; the 1 s bound has its own timing tests.
        let deadline = Instant::now() + Duration::from_secs(5);
        while !c.charger_gone().expect("charger gone") {
            assert!(Instant::now() < deadline, "still not gone");
            std::thread::sleep(Duration::from_millis(20));
        }
        let _port = port.on("entry pd", b"");
        let section = c.list(TriggerProtocol::Pd).expect("no PD is an answer");
        assert!(!section.supported);
        assert!(!c.charger_gone().expect("charger gone"));
        drop(c);
        let _ = w.close();
    }

    #[test]
    fn a_block_waiting_unread_on_the_port_refuses_the_held_request() {
        let (port, w, c) = armed(&[]);
        port.arrive(&entry_pd_offering(OFFERS_12V_AT_2));
        let before = port.writes().len();
        c.request(pd(2), false).expect_err("the offers changed");
        assert_eq!(port.writes()[before..], [] as [String; 0]);
        assert!(!c.armed().expect("armed"), "{NOT_ARMED}");
        drop(c);
        let _ = w.close();
    }

    #[test]
    fn a_block_waiting_unread_on_the_port_shows_in_risk_and_armed() {
        let (port, w, c) = armed(&[]);
        port.arrive(&entry_pd_offering(OFFERS_12V_AT_2));
        assert!(!c.armed().expect("armed"), "{NOT_ARMED}");
        drop(c);
        let _ = w.close();
        let (port, w, c) = armed(&[]);
        port.arrive(&entry_pd_offering(OFFERS_12V_AT_2));
        assert_eq!(c.risk(pd(2)).expect("risk"), RiskClass::Above5VRememberable);
        drop(c);
        let _ = w.close();
    }

    /// A session whose page was shown 9 V at position 2, with 12 V there now.
    fn shown_9v_now_12v() -> (Scripted, TriggerWorker, TriggerControl) {
        let port = charger(&[]);
        let w = spawn(&port);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        let reply = [
            entry_pd_offering(OFFERS_12V_AT_2),
            b"type=0,em=2,sink=0\n".to_vec(),
        ]
        .concat();
        let port = port.on("pdm parm", &reply);
        c.console_send("pdm parm".to_owned(), false).expect("sent");
        (port, w, c)
    }

    #[test]
    fn a_confirmation_of_a_voltage_the_page_no_longer_shows_is_refused() {
        let (port, w, c) = shown_9v_now_12v();
        // The risk call takes the new block in, and the dialog still says 9 V.
        c.risk(pd(2)).expect("risk");
        let before = port.writes().len();
        c.request(pd(2), true).expect_err("the page showed 9 V");
        c.request(pd(2), Confirm::YesRemember)
            .expect_err("the page showed 9 V");
        assert_eq!(port.writes()[before..], [] as [String; 0]);
        assert!(!c.armed().expect("armed"), "{NOT_ARMED}");
        drop(c);
        let _ = w.close();
    }

    #[test]
    fn listing_again_shows_the_new_offers_so_the_request_goes_out() {
        let (port, w, c) = shown_9v_now_12v();
        let port = port
            .on("entry pd", &entry_pd_offering(OFFERS_12V_AT_2))
            .on("pdm parm", b"type=0,em=2,sink=0\n")
            .on("pd req=2", b"req:12000mV,3000mA\nready:12000mV,0mA\n");
        c.list(TriggerProtocol::Pd).expect("list pd");
        c.request(pd(2), true).expect("the page shows 12 V now");
        assert!(port.writes().iter().any(|w| w == "pd req=2"));
        drop(c);
        let _ = w.close();
    }

    #[test]
    fn armed_a_request_of_unknown_voltage_is_refused_with_nothing_sent() {
        let (port, w, c) = armed(&[]);
        let before = port.writes().len();
        // No offer at position 6: nothing says what it would raise VBUS to.
        c.request(pd(6), false).expect_err("no such offer");
        assert_eq!(port.writes()[before..], [] as [String; 0]);
        drop(c);
        let _ = w.close();
    }

    #[test]
    fn a_confirmed_request_whose_offer_changed_is_refused_not_sent() {
        // The dialog named 9 V; position 2 is 12 V by the time it is answered.
        let port = charger(&[("entry qc", b"ok\nready\n")]);
        let w = spawn(&port);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        c.list(TriggerProtocol::Qc).expect("list qc");
        let port = port.on("entry pd", &entry_pd_offering(OFFERS_12V_AT_2));
        let before = port.writes().len();
        c.request(pd(2), true).expect_err("the offer changed");
        let writes = port.writes()[before..].to_vec();
        assert!(!writes.iter().any(|w| w == "pd req=2"), "{writes:?}");
        drop(c);
        let _ = w.close();
    }

    #[test]
    fn a_remembered_request_whose_offer_changed_is_refused_and_does_not_arm() {
        let port = charger(&[("entry qc", b"ok\nready\n")]);
        let w = spawn(&port);
        let c = w.control();
        c.list(TriggerProtocol::Pd).expect("list pd");
        c.list(TriggerProtocol::Qc).expect("list qc");
        let port = port.on("entry pd", &entry_pd_offering(OFFERS_12V_AT_2));
        let before = port.writes().len();
        c.request(pd(2), Confirm::YesRemember)
            .expect_err("the offer changed");
        let writes = port.writes()[before..].to_vec();
        assert!(!writes.iter().any(|w| w == "pd req=2"), "{writes:?}");
        assert!(!c.armed().expect("armed"), "armed on a changed offer");
        drop(c);
        let _ = w.close();
    }

    #[test]
    fn entering_pd_again_with_the_same_offers_stays_armed() {
        let (port, w, c) = armed(&[("pd req=3", b"req:15000mV,3000mA\nready:14800mV,0mA\n")]);
        c.list(TriggerProtocol::Qc).expect("list qc");
        c.request(pd(3), false).expect("the same offers");
        assert!(port.writes().iter().any(|w| w == "pd req=3"));
        assert!(c.armed().expect("armed"));
        w.close().expect("close");
    }
}
