//! The trigger panel's backend (FR DGSQ6S): a PDM text session on the
//! meter's virtual serial port, run on a thread of its own next to the
//! measurement stream. The UI detects protocols, lists offers / limits /
//! PD emulation settings, and requests voltages through it; every command
//! above 5 V needs the user's explicit confirmation, and the backend is the
//! final gate (the library risk classifier decides). Closing, dropping,
//! disconnecting or quitting returns the charger to 5 V where the protocol
//! allows it and sends `pdm close`.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use powerz_protocol::pdm::console;
use powerz_protocol::pdm::detect::{self, ListOptions};
use powerz_protocol::pdm::legacy::{self, LegacyProtocol};
use powerz_protocol::pdm::message::{self, CableIdentity, PdMessage, SopTarget};
use powerz_protocol::pdm::{self, HighVoltageConfirmation, PdmSession, pd};
use powerz_protocol::session::Clock;
use serde::{Deserialize, Serialize};

use crate::ui_error::{ErrorKind, UiError};
use crate::worker::WorkerControl;

/// Every protocol the panel has a section for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerProtocol {
    Pd,
    Qc,
    Fcp,
    Afc,
    Sfcp,
    Scp,
    Vfcp,
    Tfcp,
    Ufcs,
}

impl TriggerProtocol {
    /// Every protocol, PD first: one section each in the view.
    pub const ALL: [TriggerProtocol; 9] = [
        TriggerProtocol::Pd,
        TriggerProtocol::Qc,
        TriggerProtocol::Fcp,
        TriggerProtocol::Afc,
        TriggerProtocol::Sfcp,
        TriggerProtocol::Scp,
        TriggerProtocol::Vfcp,
        TriggerProtocol::Tfcp,
        TriggerProtocol::Ufcs,
    ];
}

/// One line of the automatic list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DetectEntry {
    /// Protocol name as the meter prints it (`bc1.2`, `ufcs`, ...).
    pub name: String,
    /// `supported`, `not-supported` or `unknown`.
    pub support: String,
    pub vbus_mv: Option<u32>,
    pub ibus_ma: Option<u32>,
}

/// What a detection found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Detection {
    /// `supported`, `not-supported` or `unknown`.
    pub bc12: String,
    /// `supported`, `not-supported` or `unknown`.
    pub apple: String,
    /// The automatic list, in arrival order.
    pub entries: Vec<DetectEntry>,
    /// Whether the list ended normally.
    pub complete: bool,
    /// The D+ / D- charging-mode hint line from the latest measurement,
    /// taken before the probes; `None` without one.
    pub hint: Option<String>,
}

/// One PD offer, as a button (fixed) or a slider range (PPS / AVS).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OfferView {
    pub position: u8,
    /// `fixed`, `battery`, `variable`, `pps`, `spr-avs` or `epr-avs`.
    pub kind: String,
    pub min_voltage_mv: u32,
    pub max_voltage_mv: u32,
    pub max_current_ma: Option<u32>,
    pub max_power_mw: Option<u32>,
    /// An EPR position (8 and above).
    pub epr: bool,
}

/// The limits of one non-PD capability (one UFCS entry, or the block).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LimitsView {
    pub index: Option<u8>,
    pub min_voltage_mv: u32,
    pub max_voltage_mv: u32,
    pub min_current_ma: u32,
    pub max_current_ma: u32,
    pub max_power_mw: Option<u32>,
}

/// The meter's PD emulation settings (`pdm parm`), by name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PdEmulation {
    pub pd_type: String,
    pub emarker: String,
    pub sink: String,
}

/// One protocol's section of the view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProtocolSection {
    pub protocol: TriggerProtocol,
    /// False when the charger did not answer the protocol.
    pub supported: bool,
    /// PD offers (PD only).
    pub offers: Vec<OfferView>,
    /// Capability limits (SCP / VFCP / TFCP / UFCS).
    pub limits: Vec<LimitsView>,
    /// PD emulation settings (PD only).
    pub settings: Option<PdEmulation>,
}

/// One request from the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(tag = "protocol", rename_all = "snake_case")]
pub enum TriggerRequest {
    /// A PD offer by position; PPS / AVS take a voltage and a current.
    Pd {
        position: u8,
        voltage_mv: Option<u32>,
        current_ma: Option<u32>,
    },
    Qc {
        volts: u8,
    },
    Qc3 {
        voltage_mv: u32,
    },
    Fcp {
        volts: u8,
    },
    Afc {
        volts: u8,
    },
    Sfcp {
        volts: u8,
    },
    Scp {
        voltage_mv: u32,
        current_ma: u32,
    },
    Vfcp {
        voltage_mv: u32,
        current_ma: u32,
    },
    Tfcp {
        voltage_mv: u32,
        current_ma: u32,
    },
    Ufcs {
        index: u8,
        voltage_mv: u32,
        current_ma: u32,
    },
}

/// The library risk class of a request, for the UI's confirmation dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum RiskClass {
    #[serde(rename = "read_only")]
    ReadOnly,
    #[serde(rename = "at_most_5v")]
    AtMost5V,
    #[serde(rename = "above_5v")]
    Above5V,
    /// Above 5 V, and the dialog may offer "don't ask again" (FR AXHK2N).
    #[serde(rename = "above_5v_rememberable")]
    Above5VRememberable,
    /// Above 5 V, and covered by a "don't ask again": no dialog (FR AXHK2N).
    #[serde(rename = "armed")]
    Armed,
}

/// What the charger answered to a request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TriggerReply {
    /// The command sent, as text (`pd req=2`, `qc 9V`, ...).
    pub command: String,
    /// What the meter asked for (`req:`), mV / mA.
    pub requested_mv: Option<u32>,
    pub requested_ma: Option<u32>,
    /// What the meter measured at the reply (`ready:`), mV / mA.
    pub delivered_mv: Option<u32>,
    pub delivered_ma: Option<u32>,
    /// The raw reply line of a non-PD request (`ready`), if one came.
    pub reply: Option<String>,
}

/// Where a built PD message goes, as the UI names it (FR DGSQ6T).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageSop {
    Sop,
    SopPrime,
    SopDoublePrime,
}

/// A PD message from the builder: no data objects is a control message.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PdMessageRequest {
    pub sop: MessageSop,
    pub msg_type: u8,
    pub data_objects: Vec<u32>,
}

/// Which way a console line went.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsoleDirection {
    Tx,
    Rx,
}

/// One console transcript line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConsoleLine {
    /// Clock time, ms since the Unix epoch.
    pub at_ms: u64,
    pub direction: ConsoleDirection,
    /// The line sent or received (a block's tag for a block).
    pub text: String,
    /// A parsed binary block, in words (`pdo: #1 Fixed 5.00 V 3.00 A, ...`).
    pub block: Option<String>,
}

/// A cable's e-marker details.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CableView {
    pub vid: u16,
    pub pid: Option<u16>,
    pub active: bool,
    pub max_voltage_v: Option<u32>,
    pub max_current_ma: Option<u32>,
    pub epr_capable: bool,
    /// USB speed in words; `None` when the cable sent no cable VDO.
    pub speed: Option<String>,
    /// The name from our own vendor table; `None` for a VID it does not list.
    pub vendor: Option<String>,
}

/// What sending a built message did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PdSendReply {
    /// The command sent (`pd cmd=13`, `pd data=01...`).
    pub command: String,
    /// The transcript of the send.
    pub lines: Vec<ConsoleLine>,
    /// The cable's identity, when a cable answered a Discover Identity.
    pub cable: Option<CableView>,
}

/// What one Read cable press found (FR AXHK2P).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "cable", rename_all = "snake_case")]
pub enum CableRead {
    /// The cable's Discover Identity ACK came.
    Found(CableView),
    /// The wait ended with no ACK.
    NoAnswer,
}

/// How the user answered the above-5 V question (FR AXHK2N).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confirm {
    No,
    Yes,
    /// Confirmed with "don't ask again" checked. The session judges from the
    /// charger's offers whether the request may arm it.
    YesRemember,
}

impl From<bool> for Confirm {
    fn from(confirmed: bool) -> Confirm {
        if confirmed { Confirm::Yes } else { Confirm::No }
    }
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
        job: impl FnOnce(&mut dyn Engine) -> Result<T, UiError> + Send + 'static,
    ) -> Result<T, UiError> {
        let (reply_tx, reply_rx) = mpsc::channel();
        let job: Job = Box::new(move |engine| {
            let result = job(&mut *engine);
            // The port failed: the meter may be gone, so nothing stays armed.
            if let Err(e) = &result
                && matches!(e.kind, ErrorKind::Disconnected | ErrorKind::Internal)
            {
                engine.disarm(ArmedEnd::PortFailed);
            }
            let _ = reply_tx.send(result);
        });
        self.tx.send(Msg::Job(job)).map_err(|_| closed())?;
        reply_rx.recv().map_err(|_| closed())?
    }

    /// BC 1.2, Apple and the automatic list; nothing above 5 V is asked.
    /// The stream's GET_DATA polling pauses meanwhile on the open bulk
    /// session (no reconnect: a bulk CONNECT silences the PDM session
    /// [device]) and resumes however the list ends.
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

    /// Back to 5 V: PD `pd req=1` (Hard Reset as the fallback), the other
    /// protocols `reset`. The session stays open. Out of sync it still
    /// works: `pd hrst` / `reset` go out blind, `pdm close` follows, and the
    /// answer is the out-of-sync error saying VBUS is not confirmed.
    pub fn back_to_5v(&self) -> Result<(), UiError> {
        self.call(|e| e.back_to_5v())
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
        if let Some(reason) = message::refusal(&pdm::Command::parse(line.trim())) {
            return Err(UiError::usage(reason));
        }
        self.call(move |e| e.console_send(line, confirmed))
    }
}

/// The session's calls, on the session thread (erases the port / clock types).
trait Engine {
    fn detect(&mut self) -> Result<Detection, UiError>;
    fn list(&mut self, protocol: TriggerProtocol) -> Result<ProtocolSection, UiError>;
    fn risk(&mut self, request: TriggerRequest) -> Result<RiskClass, UiError>;
    fn request(
        &mut self,
        request: TriggerRequest,
        confirm: Confirm,
    ) -> Result<TriggerReply, UiError>;
    fn armed(&mut self) -> bool;
    fn disarm(&mut self, why: ArmedEnd);
    fn charger_gone(&mut self) -> bool;
    fn back_to_5v(&mut self) -> Result<(), UiError>;
    fn message_risk(&mut self, message: PdMessageRequest) -> Result<RiskClass, UiError>;
    fn send_message(
        &mut self,
        message: PdMessageRequest,
        confirmed: bool,
    ) -> Result<PdSendReply, UiError>;
    fn cable_read(&mut self) -> Result<CableRead, UiError>;
    fn console_risk(&mut self, line: String) -> Result<RiskClass, UiError>;
    fn console_send(&mut self, line: String, confirmed: bool) -> Result<Vec<ConsoleLine>, UiError>;
}

type Job = Box<dyn FnOnce(&mut dyn Engine) + Send>;

/// What the session thread is asked to do.
enum Msg {
    Job(Job),
    /// Read VBUS from here on.
    WatchVbus(VbusReader),
    /// Send the session's log lines here from now on.
    LogTo(LogSink),
    /// Back to 5 V, `pdm close`, answer, and end the thread.
    Close(mpsc::Sender<Result<(), UiError>>),
}

/// The latest VBUS reading, V; `None` when there is none.
type VbusReader = Box<dyn FnMut() -> Option<f64> + Send>;
/// Where the session's log lines go.
type LogSink = Box<dyn FnMut(String) + Send>;

/// Why "don't ask again" ended, for the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArmedEnd {
    Cleared,
    BackTo5V,
    Detect,
    BuiltMessage,
    ConsoleLine,
    CableRead,
    OffersChanged,
    RequestFailed,
    RequestNotCovered,
    PortFailed,
    PortNotQuiet,
    ChargerGone,
    SessionClosed,
}

impl ArmedEnd {
    fn text(self) -> &'static str {
        match self {
            ArmedEnd::Cleared => "cleared",
            ArmedEnd::BackTo5V => "back to 5 V",
            ArmedEnd::Detect => "detect",
            ArmedEnd::BuiltMessage => "built message",
            ArmedEnd::ConsoleLine => "console line",
            ArmedEnd::CableRead => "cable read",
            ArmedEnd::OffersChanged => "offers changed",
            ArmedEnd::RequestFailed => "request failed",
            ArmedEnd::RequestNotCovered => "request not covered",
            ArmedEnd::PortFailed => "port failed",
            ArmedEnd::PortNotQuiet => "port not quiet",
            ArmedEnd::ChargerGone => "charger gone",
            ArmedEnd::SessionClosed => "session closed",
        }
    }
}

/// What the VBUS watch knows of the charger (FR AXHK2N).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Powered {
    /// No contract stands: this charger reads 0 V until a PD session starts,
    /// and Detect and resets take VBUS down on purpose, so low means nothing.
    No,
    /// A contract was accepted at this time and no reading of [`VBUS_UP_V`]
    /// came yet. A sample from just before the request must not count as a
    /// collapse, so low only counts once [`VBUS_RISE_WAIT`] has passed.
    Rising(Instant),
    /// VBUS rose under the contract: below [`VBUS_GONE_V`] the charger is gone.
    Up,
}

/// What was last entered, i.e. how to get back to 5 V.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Entered {
    None,
    Pd,
    Legacy,
}

/// A request past the refusals, with what judging it found.
struct Judged {
    lib: LibRequest,
    command: pdm::Command,
    above: bool,
    rememberable: bool,
    covered: bool,
}

/// The PDM session and what the charger offered, owned by the thread.
struct Session<P: Read + Write, C: Clock> {
    pdm: PdmSession<P, C>,
    offers: Vec<pd::Offer>,
    /// The offers last listed: what the page's buttons and dialog go by.
    shown: Vec<pd::Offer>,
    entered: Entered,
    /// A PD request was accepted and not yet undone: it is always ended
    /// through PD (`pd req=1`, Hard Reset as the fallback), whatever was
    /// entered after it.
    pd_contract: bool,
    /// The way back to 5 V went out blind while the meter was out of sync
    /// with a contract above 5 V possibly active: VBUS is not confirmed.
    vbus_unconfirmed: bool,
    /// "Don't ask again" (FR AXHK2N): set only by a request that succeeded
    /// and qualified. It covers only what `pd::rememberable` allows.
    armed: bool,
    /// Whether a low VBUS reading would mean anything.
    powered: Powered,
    /// VBUS collapsed under an accepted contract: the offers are forgotten and
    /// every request is refused until a PD listing.
    gone: bool,
    vbus: Option<VbusReader>,
    log: Option<LogSink>,
}

impl<P: Read + Write, C: Clock> Session<P, C> {
    /// End "don't ask again", saying why once if it was on.
    fn end_armed(&mut self, why: ArmedEnd) {
        if std::mem::take(&mut self.armed) {
            self.log_armed_end(why);
        }
    }

    fn log_armed_end(&mut self, why: ArmedEnd) {
        self.log_line(format!("trigger armed ended: {}", why.text()));
    }

    fn log_line(&mut self, line: String) {
        if let Some(log) = &mut self.log {
            log(line);
        }
    }

    /// A PD request or the way back was accepted: VBUS is watched again. The
    /// reading right now tells whether it has already risen.
    fn contract_accepted(&mut self) {
        // A new contract on a charger that was up stays watched as up.
        if self.powered != Powered::Up {
            self.powered = Powered::Rising(Instant::now());
        }
        self.check_vbus();
    }

    /// One look at VBUS. Nothing is written; a collapse only ends what stood
    /// on the charger.
    fn check_vbus(&mut self) {
        if self.powered == Powered::No {
            return;
        }
        let Some(volts) = self.vbus.as_mut().and_then(|read| read()) else {
            return;
        };
        if volts >= VBUS_UP_V {
            self.powered = Powered::Up;
            return;
        }
        let collapsed = match self.powered {
            Powered::Rising(since) => since.elapsed() >= VBUS_RISE_WAIT,
            _ => true,
        };
        if volts < VBUS_GONE_V && collapsed {
            self.powered = Powered::No;
            self.gone = true;
            self.offers.clear();
            self.shown.clear();
            self.end_armed(ArmedEnd::ChargerGone);
        }
    }

    /// Back to 5 V where the protocol allows it: PD `pd req=1` (Hard Reset
    /// as the fallback), the others `reset`. Out of sync, the library's
    /// safety path writes `pd hrst` / `reset` blind and `pdm close` follows.
    /// Disarms before anything is written, so a failed way back ends "don't
    /// ask again" too.
    fn return_to_5v(&mut self) -> Result<(), UiError> {
        self.end_armed(ArmedEnd::BackTo5V);
        // Until it is accepted: a Hard Reset takes VBUS down on purpose.
        self.powered = Powered::No;
        if let Err(e) = self.pdm.check_in_step() {
            return Err(self.blind_return_to_5v(&e));
        }
        let pd = if self.pd_contract || self.entered == Entered::Pd {
            pd::return_to_5v(&mut self.pdm, &self.offers)
                .map(|back| matches!(back, pd::BackTo5V::Requested))
                .map_err(|e| pd_error(&e.hard_reset))
        } else {
            Ok(false)
        };
        if pd.is_ok() {
            self.pd_contract = false;
        }
        if pd == Ok(true) && self.entered != Entered::Legacy {
            self.contract_accepted();
        }
        let legacy = if self.entered == Entered::Legacy {
            legacy::return_to_5v(&mut self.pdm).map_err(|e| legacy_error(&e))
        } else {
            Ok(())
        };
        pd.map(|_| ()).and(legacy)
    }

    /// The meter is out of step, so no reply can confirm 5 V: send the way
    /// back blind (`pd hrst` for PD, `reset` for the others), close the PDM
    /// session, and say what was done. Sent once; later calls only report.
    fn blind_return_to_5v(&mut self, err: &pdm::Error) -> UiError {
        // A PD contract or a legacy protocol may have raised VBUS.
        if self.pd_contract || self.entered == Entered::Legacy {
            self.vbus_unconfirmed = true;
        }
        // Both report OutOfSync after their blind write; nothing to add.
        if self.pd_contract || self.entered == Entered::Pd {
            let _ = pd::return_to_5v(&mut self.pdm, &self.offers);
        }
        if self.entered == Entered::Legacy {
            let _ = legacy::return_to_5v(&mut self.pdm);
        }
        self.pd_contract = false;
        self.entered = Entered::None;
        let _ = self.pdm.close_in_place();
        let mut ui = pdm_error(err);
        if self.vbus_unconfirmed {
            ui.hint = Some(
                "VBUS may still be above 5 V: the app sent a Hard Reset (or reset) and closed \
                 the session, but the meter could not confirm it. Unplug the charger if in \
                 doubt, then power-cycle the meter (unplug it and plug it back in) and open \
                 the session again."
                    .to_owned(),
            );
        }
        ui
    }

    /// Back to 5 V, then `pdm close`; the first error wins.
    fn shutdown(mut self) -> Result<(), UiError> {
        self.end_armed(ArmedEnd::SessionClosed);
        let back = self.return_to_5v();
        let closed = self.pdm.close().map_err(|e| pdm_error(&e));
        back.and(closed)
    }

    fn list_pd(&mut self) -> Result<ProtocolSection, UiError> {
        let negotiation = match pd::enter(&mut self.pdm) {
            Ok(n) => n,
            Err(pd::Error::Pdm(pdm::Error::Timeout { .. })) => {
                // An answer too: this charger has no PD. It is not a dead end,
                // and with no offers no PD request can go out.
                self.gone = false;
                self.offers.clear();
                self.shown.clear();
                return Ok(unsupported(TriggerProtocol::Pd));
            }
            Err(e) => return Err(pd_error(&e)),
        };
        self.entered = Entered::Pd;
        self.set_offers(negotiation.offers);
        self.shown = self.offers.clone();
        self.gone = false;
        self.log_line(offers_line(&self.offers));
        let settings = pd::read_settings(&mut self.pdm).map_err(|e| pd_error(&e))?;
        Ok(ProtocolSection {
            protocol: TriggerProtocol::Pd,
            supported: true,
            offers: self.offers.iter().map(offer_view).collect(),
            limits: Vec::new(),
            settings: Some(PdEmulation {
                pd_type: settings.pd_type.name().to_owned(),
                emarker: settings.emarker.name().to_owned(),
                sink: settings.sink.name().to_owned(),
            }),
        })
    }

    fn list_legacy(
        &mut self,
        protocol: TriggerProtocol,
        lp: LegacyProtocol,
    ) -> Result<ProtocolSection, UiError> {
        // Any entry attempt may leave the meter in the protocol: `reset` on close.
        self.entered = Entered::Legacy;
        self.powered = Powered::No;
        match legacy::enter(&mut self.pdm, lp) {
            Ok(entered) => Ok(ProtocolSection {
                protocol,
                supported: true,
                offers: Vec::new(),
                limits: entered.limits.iter().map(limits_view).collect(),
                settings: None,
            }),
            Err(legacy::Error::NotSupported { .. }) => Ok(unsupported(protocol)),
            Err(e) => Err(legacy_error(&e)),
        }
    }

    /// Remember what a written console line entered or raised, so closing
    /// the session still returns to 5 V the right way. Raw text of unknown
    /// effect is ended through PD once PD is entered, else by `pdm close`.
    fn track_console(&mut self, effect: Option<console::Effect>) {
        match effect {
            Some(console::Effect::EnteredPd) => {
                self.entered = Entered::Pd;
                self.sync_offers();
            }
            Some(console::Effect::PdContract) => self.pd_contract = true,
            Some(console::Effect::Legacy) => self.entered = Entered::Legacy,
            Some(console::Effect::RawLine) if self.entered == Entered::Pd => {
                self.pd_contract = true;
            }
            Some(console::Effect::RawLine) | None => {}
        }
    }

    /// `entry pd` unless PD is the protocol the meter entered last; the
    /// refreshed offers replace the old ones. (AC-DGSQ6S.13)
    fn enter_pd(&mut self) -> Result<(), UiError> {
        if let Some(negotiation) = pd::ensure_entered(&mut self.pdm).map_err(|e| pd_error(&e))? {
            self.entered = Entered::Pd;
            self.set_offers(negotiation.offers);
        }
        Ok(())
    }

    /// Replace the offers. Any change ends "don't ask again": being armed
    /// stands on the offers it was judged from.
    fn set_offers(&mut self, offers: Vec<pd::Offer>) {
        if offers != self.offers {
            self.end_armed(ArmedEnd::OffersChanged);
        }
        self.offers = offers;
    }

    /// Take in the meter's latest `pdo:` block, which may have arrived
    /// unasked (another charger plugged in) between two commands.
    fn sync_offers(&mut self) {
        // The capabilities still hold the block of the charger that went away.
        if self.gone {
            return;
        }
        if self.armed {
            self.read_unasked();
        }
        if self.pdm.capabilities().pd.is_some() {
            self.set_offers(pd::offers(self.pdm.capabilities()));
        }
    }

    /// Whether a "don't ask again" may cover `request`, judged from the
    /// charger's offers, never from a UI value.
    fn rememberable(&self, request: &LibRequest) -> bool {
        match request {
            LibRequest::Pd(r) => pd::rememberable(&self.offers, r),
            LibRequest::Legacy(_) => false,
        }
    }

    /// The offer `request` names differs from the one the page was shown. A
    /// position the page never listed named no voltage, so nothing moved.
    fn offer_moved(&self, request: &pd::Request) -> bool {
        pd::offer_of(&self.shown, request)
            .is_some_and(|shown| Some(shown) != pd::offer_of(&self.offers, request))
    }

    /// Read what is waiting on the port. Nothing else reads it between
    /// commands, so a new charger's `pdo:` block would stay unseen until the
    /// next reply, after an armed request had gone out.
    fn read_unasked(&mut self) {
        for _ in 0..UNASKED_ITEMS {
            match self.pdm.next_item(UNASKED_WAIT) {
                Ok(_) => {}
                Err(pdm::Error::Timeout { .. }) => return,
                Err(_) => break,
            }
        }
        // A port that fails or never falls quiet proves nothing: ask again.
        self.end_armed(ArmedEnd::PortNotQuiet);
    }

    /// The class of a built message: the library's, except the one cable read.
    fn message_class(&self, command: &pdm::Command) -> pdm::Risk {
        if is_cable_read(command) {
            pdm::Risk::AtMost5V
        } else {
            self.pdm.risk(command)
        }
    }

    /// Write a built message. The library keeps the cable read above 5 V, so
    /// `powerz` keeps asking; this confirmation is made here, for those exact
    /// bytes only, and no answer from the UI can reach it.
    fn write_message(&mut self, command: &pdm::Command, above: bool) -> Result<(), pdm::Error> {
        if above || is_cable_read(command) {
            let confirmation = HighVoltageConfirmation::for_command(command);
            self.pdm.send_confirmed(command, confirmation)
        } else {
            self.pdm.send(command)
        }
    }

    /// Read a cable's Discover Identity ACK, if one comes within the wait.
    fn await_cable(&mut self) -> Result<Option<CableView>, UiError> {
        let cable = message::await_cable_identity(&mut self.pdm, message::CABLE_WAIT)
            .map_err(|e| pdm_error(&e))?;
        Ok(cable.map(|c| cable_view(&c)))
    }

    /// Validate `request` against what the charger offered and render its
    /// command; pure, nothing is written.
    fn command_of(&self, request: &LibRequest) -> Result<pdm::Command, UiError> {
        match request {
            LibRequest::Pd(r) => pd::validate(&self.offers, r).map_err(|e| pd_error(&e)),
            LibRequest::Legacy(r) => {
                legacy::validate(self.pdm.capabilities(), r).map_err(|e| legacy_error(&e))
            }
        }
    }

    /// The refusals of `request`; nothing is written and `armed` stands.
    fn judge(&mut self, request: TriggerRequest, confirm: Confirm) -> Result<Judged, UiError> {
        let lib = lib_request(request)?;
        // A queued request must not outrun the 100 ms check.
        self.check_vbus();
        // A meter out of step refuses everything, before any validation.
        self.pdm.check_in_step().map_err(|e| pdm_error(&e))?;
        // Whatever it asks for, and whichever protocol: nothing is written.
        if self.gone {
            return Err(charger_went_away());
        }
        // An unasked `pdo:` block disarms here, before anything is judged.
        self.sync_offers();
        // Validation first: an invalid request writes nothing.
        let command = self.command_of(&lib)?;
        let above = self.pdm.risk(&command) == pdm::Risk::Above5V;
        let rememberable = above && self.rememberable(&lib);
        if confirm == Confirm::YesRemember && !rememberable {
            return Err(UiError::usage("This request cannot be remembered."));
        }
        let covered = confirm == Confirm::No && self.armed && rememberable;
        // The final gate: above 5 V without the user's confirmation or an
        // armed "don't ask again" is refused before anything is written.
        if above && confirm == Confirm::No && !covered {
            return Err(not_confirmed());
        }
        Ok(Judged {
            lib,
            command,
            above,
            rememberable,
            covered,
        })
    }

    /// Send a judged request. `armed` is already down; `was_armed` says it
    /// stood before. Only a request that qualifies puts it back.
    fn send_judged(
        &mut self,
        Judged {
            lib,
            command,
            above,
            rememberable,
            covered,
        }: Judged,
        confirm: Confirm,
        was_armed: bool,
    ) -> Result<TriggerReply, UiError> {
        let confirmation = above.then(|| HighVoltageConfirmation::for_command(&command));
        match lib {
            LibRequest::Pd(r) => {
                // The dialog named what the page was shown, so a "yes" stands
                // on that offer: refused, not sent, once it changed.
                if above && self.offer_moved(&r) {
                    return Err(offers_changed());
                }
                let judged = self.offers.clone();
                // Another protocol (or none) entered since PD: enter PD
                // again so the request goes to a PD session, and check it
                // against what the charger offers now. (AC-DGSQ6S.13)
                self.enter_pd()?;
                // Being armed stands on all the offers, not only this one.
                let same_offers = judged == self.offers;
                if (above && self.offer_moved(&r)) || (covered && !same_offers) {
                    return Err(offers_changed());
                }
                let outcome =
                    pd::request(&mut self.pdm, &self.offers, &r, confirmation).map_err(|e| {
                        // Written, then out of step: the charger may have
                        // taken it, so Back to 5 V must end it through PD.
                        if above && matches!(e, pd::Error::Pdm(pdm::Error::OutOfSync { .. })) {
                            self.pd_contract = true;
                        }
                        pd_error(&e)
                    })?;
                self.entered = Entered::Pd;
                self.pd_contract = true;
                self.armed =
                    confirm == Confirm::YesRemember || (was_armed && rememberable && same_offers);
                self.contract_accepted();
                Ok(TriggerReply {
                    command: command_text(&outcome.command),
                    requested_mv: Some(outcome.requested.voltage_mv),
                    requested_ma: Some(outcome.requested.current_ma),
                    delivered_mv: Some(outcome.delivered.voltage_mv),
                    delivered_ma: Some(outcome.delivered.current_ma),
                    reply: None,
                })
            }
            LibRequest::Legacy(r) => {
                // The library enters the protocol again first when another
                // one was entered since; any entry attempt may leave the
                // meter in it: `reset` on close.
                self.entered = Entered::Legacy;
                self.powered = Powered::No;
                let outcome = legacy::request(&mut self.pdm, &r, confirmation)
                    .map_err(|e| legacy_error(&e))?;
                Ok(TriggerReply {
                    command: command_text(&outcome.command),
                    requested_mv: None,
                    requested_ma: None,
                    delivered_mv: None,
                    delivered_ma: None,
                    reply: outcome.reply.map(|l| l.trim().to_owned()),
                })
            }
        }
    }
}

impl<P: Read + Write, C: Clock> Engine for Session<P, C> {
    fn detect(&mut self) -> Result<Detection, UiError> {
        // Before the first write, so a failed detection ends it too. The
        // automatic list drops VBUS while it probes the charger.
        self.end_armed(ArmedEnd::Detect);
        self.powered = Powered::No;
        let bc12 = detect::bc12(&mut self.pdm).map_err(|e| pdm_error(&e))?;
        let apple = detect::apple(&mut self.pdm).map_err(|e| pdm_error(&e))?;
        let list = detect::list(&mut self.pdm, &ListOptions::default(), &mut |_| {})
            .map_err(|e| pdm_error(&e))?;
        Ok(Detection {
            bc12: bc12.name().to_owned(),
            apple: apple.name().to_owned(),
            entries: list
                .entries
                .iter()
                .map(|e| DetectEntry {
                    name: e.name.clone(),
                    support: e.support.name().to_owned(),
                    vbus_mv: e.vbus_mv,
                    ibus_ma: e.ibus_ma,
                })
                .collect(),
            complete: list.complete,
            hint: None,
        })
    }

    fn list(&mut self, protocol: TriggerProtocol) -> Result<ProtocolSection, UiError> {
        match legacy_of(protocol) {
            None => self.list_pd(),
            Some(lp) => self.list_legacy(protocol, lp),
        }
    }

    fn risk(&mut self, request: TriggerRequest) -> Result<RiskClass, UiError> {
        let lib = lib_request(request)?;
        self.check_vbus();
        if self.gone {
            return Err(charger_went_away());
        }
        // So an unasked `pdo:` block shows as a dialog, not as a refusal.
        self.sync_offers();
        let command = self.command_of(&lib)?;
        Ok(match risk_class(self.pdm.risk(&command)) {
            RiskClass::Above5V if self.rememberable(&lib) => {
                if self.armed {
                    RiskClass::Armed
                } else {
                    RiskClass::Above5VRememberable
                }
            }
            class => class,
        })
    }

    fn request(
        &mut self,
        request: TriggerRequest,
        confirm: Confirm,
    ) -> Result<TriggerReply, UiError> {
        let judged = self.judge(request, confirm)?;
        // Past the refusals: disarmed before the first write, so a failure after
        // it leaves "don't ask again" ended.
        let was_armed = std::mem::take(&mut self.armed);
        let offers = self.offers.clone();
        let reply = self.send_judged(judged, confirm, was_armed);
        // Only now is it known whether a request that held "don't ask again"
        // kept it, and why not.
        if was_armed && !self.armed {
            self.log_armed_end(if self.offers != offers {
                ArmedEnd::OffersChanged
            } else if reply.is_err() {
                ArmedEnd::RequestFailed
            } else {
                ArmedEnd::RequestNotCovered
            });
        }
        reply
    }

    fn armed(&mut self) -> bool {
        self.sync_offers();
        self.armed
    }

    fn disarm(&mut self, why: ArmedEnd) {
        self.end_armed(why);
    }

    fn charger_gone(&mut self) -> bool {
        self.gone
    }

    fn back_to_5v(&mut self) -> Result<(), UiError> {
        self.return_to_5v()
    }

    fn message_risk(&mut self, message: PdMessageRequest) -> Result<RiskClass, UiError> {
        let command = lib_message(&message)
            .to_command()
            .map_err(|e| UiError::usage(e.to_string()))?;
        Ok(risk_class(self.message_class(&command)))
    }

    fn send_message(
        &mut self,
        message: PdMessageRequest,
        confirmed: bool,
    ) -> Result<PdSendReply, UiError> {
        // Building first: GoodCRC and malformed messages write nothing.
        let lib = lib_message(&message);
        let command = lib
            .to_command()
            .map_err(|e| UiError::usage(e.to_string()))?;
        let above = self.message_class(&command) == pdm::Risk::Above5V;
        // The final gate: above 5 V without the user's confirmation is
        // refused before anything is written.
        if above && !confirmed {
            return Err(UiError::usage(
                "A message that can raise VBUS above 5 V needs your confirmation.",
            ));
        }
        // Every sent message ends "don't ask again", before the first write.
        // A Soft Reset may well take VBUS down.
        self.end_armed(ArmedEnd::BuiltMessage);
        self.powered = Powered::No;
        // Like `powerz pd send`: PD first (again, when another protocol was
        // entered since), so the message has a PD session and the way back
        // to 5 V (`pd req=1`, Hard Reset as the fallback) works.
        self.enter_pd()?;
        if above {
            // It may change the contract: always end it through PD. Set
            // before the write, so a failure after it still undoes it.
            self.pd_contract = true;
        }
        let at_ms = self.pdm.now_ms();
        self.write_message(&command, above)
            .map_err(|e| pdm_error(&e))?;
        // A Discover Identity to a cable plug: read its ACK, if any.
        let cable = if lib.is_cable_discover_identity() {
            self.await_cable()?
        } else {
            None
        };
        let text = command_text(&command);
        Ok(PdSendReply {
            command: text.clone(),
            lines: vec![ConsoleLine {
                at_ms,
                direction: ConsoleDirection::Tx,
                text,
                block: None,
            }],
            cable,
        })
    }

    fn cable_read(&mut self) -> Result<CableRead, UiError> {
        // What the meter entered last, not what this session last listed: the
        // read goes out only on a PD session already open.
        if self.pdm.entered() != Some(pdm::Protocol::Pd) {
            return Err(UiError::usage(
                "Reading the cable needs an open PD session. Press a PD offer first.",
            ));
        }
        self.pdm.check_in_step().map_err(|e| pdm_error(&e))?;
        // Like every sent message, before the first write.
        self.end_armed(ArmedEnd::CableRead);
        let command = pdm::Command::PdData(CABLE_READ.to_vec());
        self.write_message(&command, false)
            .map_err(|e| pdm_error(&e))?;
        Ok(self
            .await_cable()?
            .map_or(CableRead::NoAnswer, CableRead::Found))
    }

    fn console_risk(&mut self, line: String) -> Result<RiskClass, UiError> {
        Ok(risk_class(self.pdm.risk(&console::command_of(&line))))
    }

    fn console_send(&mut self, line: String, confirmed: bool) -> Result<Vec<ConsoleLine>, UiError> {
        let command = console::command_of(&line);
        let risk = self.pdm.risk(&command);
        let above = risk == pdm::Risk::Above5V;
        // The final gate: above 5 V (any unparsable line too) without the
        // user's confirmation is refused before anything is written.
        if above && !confirmed {
            return Err(UiError::usage(
                "A line that can raise VBUS above 5 V needs your confirmation.",
            ));
        }
        // A line that is not read-only ends "don't ask again"; before the
        // write, so a failed one ends it too.
        if risk != pdm::Risk::ReadOnly {
            self.end_armed(ArmedEnd::ConsoleLine);
            self.powered = Powered::No;
        }
        let confirmation = above.then(|| HighVoltageConfirmation::for_command(&command));
        let listen = console::listen_for(&command);
        let sent = console::send_line(&mut self.pdm, &line, confirmation, listen)
            .map_err(|e| pdm_error(&e))?;
        // The line was written: its undo is recorded even if reading the
        // replies failed.
        self.track_console(sent.effect);
        if let Some(e) = &sent.error {
            return Err(pdm_error(e));
        }
        Ok(sent.entries.iter().map(console_line).collect())
    }
}

/// The refusal of a request above 5 V that nothing confirmed.
fn not_confirmed() -> UiError {
    UiError::usage("A request above 5 V needs your confirmation.")
}

/// The refusal of a request held on offers that changed.
fn offers_changed() -> UiError {
    UiError::usage("The charger's offers changed, so the request was not sent. Press Detect.")
}

/// The refusal of a request after a collapse, until a PD listing.
fn charger_went_away() -> UiError {
    UiError::usage("The charger went away. Press Detect.")
}

/// A library console entry as the UI's line; a block is described in words.
fn console_line(e: &console::ConsoleEntry) -> ConsoleLine {
    ConsoleLine {
        at_ms: e.at_ms,
        direction: match e.direction {
            console::Direction::Tx => ConsoleDirection::Tx,
            console::Direction::Rx => ConsoleDirection::Rx,
        },
        text: e.text.clone(),
        block: e.block.as_ref().map(console::describe_block),
    }
}

/// The one message the app may send a cable with no dialog: SOP', header
/// 0x108F, object 0xFF00A801 (both little endian). It cannot raise VBUS.
const CABLE_READ: [u8; 7] = [0x01, 0x8F, 0x10, 0x01, 0xA8, 0x00, 0xFF];

/// Exactly that message, parsed. Raw console text never matches, so the
/// console keeps asking for the same bytes.
fn is_cable_read(command: &pdm::Command) -> bool {
    matches!(command, pdm::Command::PdData(bytes) if bytes.as_slice() == CABLE_READ)
}

/// The library's cable identity as the UI's.
pub(crate) fn cable_view(c: &CableIdentity) -> CableView {
    CableView {
        vid: c.vid,
        pid: c.pid,
        active: c.active,
        max_voltage_v: c.max_voltage_v,
        max_current_ma: c.max_current_ma,
        epr_capable: c.epr_capable,
        speed: c.speed.map(|s| s.name()),
        vendor: pdm::vendors::vendor_name(c.vid).map(str::to_owned),
    }
}

/// A UI message as the library's.
fn lib_message(m: &PdMessageRequest) -> PdMessage {
    let sop = match m.sop {
        MessageSop::Sop => SopTarget::Sop,
        MessageSop::SopPrime => SopTarget::SopPrime,
        MessageSop::SopDoublePrime => SopTarget::SopDoublePrime,
    };
    PdMessage {
        sop,
        msg_type: m.msg_type,
        data_objects: m.data_objects.clone(),
    }
}

/// A UI request as a library request.
enum LibRequest {
    Pd(pd::Request),
    Legacy(legacy::Request),
}

/// Map a UI request to the library's; a PPS / AVS request needs both a
/// voltage and a current.
fn lib_request(request: TriggerRequest) -> Result<LibRequest, UiError> {
    use legacy::Request as L;
    use pdm::FixedProtocol as F;
    let fixed = |protocol, volts| LibRequest::Legacy(L::Fixed { protocol, volts });
    Ok(match request {
        TriggerRequest::Pd {
            position,
            voltage_mv: None,
            current_ma,
        } => LibRequest::Pd(pd::Request::Fixed {
            position,
            current_ma,
        }),
        TriggerRequest::Pd {
            position,
            voltage_mv: Some(voltage_mv),
            current_ma: Some(current_ma),
        } => LibRequest::Pd(pd::Request::Programmable {
            position,
            voltage_mv,
            current_ma,
        }),
        TriggerRequest::Pd {
            voltage_mv: Some(_),
            current_ma: None,
            ..
        } => {
            return Err(UiError::usage(
                "A PPS or AVS request needs a voltage and a current.",
            ));
        }
        TriggerRequest::Qc { volts } => fixed(F::Qc, volts),
        TriggerRequest::Fcp { volts } => fixed(F::Fcp, volts),
        TriggerRequest::Afc { volts } => fixed(F::Afc, volts),
        TriggerRequest::Sfcp { volts } => fixed(F::Sfcp, volts),
        TriggerRequest::Qc3 { voltage_mv } => LibRequest::Legacy(L::Qc3Volt { voltage_mv }),
        TriggerRequest::Scp {
            voltage_mv,
            current_ma,
        } => LibRequest::Legacy(L::Scp {
            voltage_mv,
            current_ma,
        }),
        TriggerRequest::Vfcp {
            voltage_mv,
            current_ma,
        } => LibRequest::Legacy(L::Vfcp {
            voltage_mv,
            current_ma,
        }),
        TriggerRequest::Tfcp {
            voltage_mv,
            current_ma,
        } => LibRequest::Legacy(L::Tfcp {
            voltage_mv,
            current_ma,
        }),
        TriggerRequest::Ufcs {
            index,
            voltage_mv,
            current_ma,
        } => LibRequest::Legacy(L::Ufcs {
            index,
            voltage_mv,
            current_ma,
        }),
    })
}

fn risk_class(risk: pdm::Risk) -> RiskClass {
    match risk {
        pdm::Risk::ReadOnly => RiskClass::ReadOnly,
        pdm::Risk::AtMost5V => RiskClass::AtMost5V,
        pdm::Risk::Above5V => RiskClass::Above5V,
    }
}

/// A command as the meter receives it, without the line end.
fn command_text(command: &pdm::Command) -> String {
    String::from_utf8_lossy(&command.render()).trim().to_owned()
}

/// The library protocol of a non-PD section.
fn legacy_of(protocol: TriggerProtocol) -> Option<LegacyProtocol> {
    match protocol {
        TriggerProtocol::Pd => None,
        TriggerProtocol::Qc => Some(LegacyProtocol::Qc),
        TriggerProtocol::Fcp => Some(LegacyProtocol::Fcp),
        TriggerProtocol::Afc => Some(LegacyProtocol::Afc),
        TriggerProtocol::Sfcp => Some(LegacyProtocol::Sfcp),
        TriggerProtocol::Scp => Some(LegacyProtocol::Scp),
        TriggerProtocol::Vfcp => Some(LegacyProtocol::Vfcp),
        TriggerProtocol::Tfcp => Some(LegacyProtocol::Tfcp),
        TriggerProtocol::Ufcs => Some(LegacyProtocol::Ufcs),
    }
}

fn unsupported(protocol: TriggerProtocol) -> ProtocolSection {
    ProtocolSection {
        protocol,
        supported: false,
        offers: Vec::new(),
        limits: Vec::new(),
        settings: None,
    }
}

fn offer_view(o: &pd::Offer) -> OfferView {
    OfferView {
        position: o.position,
        kind: o.kind.name().to_owned(),
        min_voltage_mv: o.min_voltage_mv,
        max_voltage_mv: o.max_voltage_mv,
        max_current_ma: o.max_current_ma,
        max_power_mw: o.max_power_mw,
        epr: o.is_epr(),
    }
}

/// The log line for a PD listing: position, kind and voltage of each offer.
fn offers_line(offers: &[pd::Offer]) -> String {
    let volts = |mv: u32| f64::from(mv) / 1000.0;
    let list: Vec<String> = offers
        .iter()
        .map(|o| {
            let (min, max) = (volts(o.min_voltage_mv), volts(o.max_voltage_mv));
            let range = if o.min_voltage_mv == o.max_voltage_mv {
                format!("{max:.2}")
            } else {
                format!("{min:.2}-{max:.2}")
            };
            format!("#{} {} {range} V", o.position, o.kind.name())
        })
        .collect();
    format!("trigger offers: {}", list.join(", "))
}

fn limits_view(l: &legacy::Limits) -> LimitsView {
    LimitsView {
        index: l.index,
        min_voltage_mv: l.min_voltage_mv,
        max_voltage_mv: l.max_voltage_mv,
        min_current_ma: l.min_current_ma,
        max_current_ma: l.max_current_ma,
        max_power_mw: l.max_power_mw,
    }
}

/// The handle of a session that has ended.
fn closed() -> UiError {
    UiError::usage("The trigger session is closed.")
}

/// A PDM session error, in plain language.
pub fn pdm_error(err: &pdm::Error) -> UiError {
    use pdm::Error as E;
    match err {
        E::Timeout { .. } => UiError {
            kind: ErrorKind::Timeout,
            message: "The meter did not answer in time.".to_owned(),
            hint: Some("Check the cable, then try again.".to_owned()),
            sessions: Vec::new(),
        },
        E::Disconnected | E::NotFound => UiError {
            kind: ErrorKind::Disconnected,
            message: "The meter's serial port went away.".to_owned(),
            hint: Some("Plug the meter back in.".to_owned()),
            sessions: Vec::new(),
        },
        E::PortHeld { .. } => crate::ui_error::port_held(),
        E::HighVoltageNotConfirmed { .. } => {
            UiError::usage("A request above 5 V needs your confirmation.")
        }
        E::Refused { reason, .. } => UiError::usage(reason.clone()),
        E::Forbidden { .. } | E::UnsupportedOnCdc { .. } => {
            UiError::usage("The app refused a command that is not safe.")
        }
        E::Ambiguous { .. } => UiError::usage("Several meters are attached; connect only one."),
        E::OutOfSync { .. } => UiError {
            kind: ErrorKind::OutOfSync,
            message: "The meter's replies are out of step with the commands, so no more \
                      requests are sent."
                .to_owned(),
            hint: Some(
                "Power-cycle the meter (unplug it and plug it back in), then open the \
                 session again."
                    .to_owned(),
            ),
            sessions: Vec::new(),
        },
        E::Malformed { .. } | E::LineTooLong { .. } | E::UnexpectedReply { .. } => UiError {
            kind: ErrorKind::Protocol,
            message: "The meter sent a reply the app did not understand.".to_owned(),
            hint: None,
            sessions: Vec::new(),
        },
        E::Io(_) => UiError {
            kind: ErrorKind::Internal,
            message: "Talking to the meter's serial port failed.".to_owned(),
            hint: Some("Replug the meter and try again.".to_owned()),
            sessions: Vec::new(),
        },
    }
}

fn pd_error(err: &pd::Error) -> UiError {
    match err {
        pd::Error::Pdm(e) => pdm_error(e),
        pd::Error::RequestFailed { reply } => UiError {
            kind: ErrorKind::Protocol,
            message: format!("The charger refused the request ({reply})."),
            hint: None,
            sessions: Vec::new(),
        },
        other => UiError::usage(other.to_string()),
    }
}

fn legacy_error(err: &legacy::Error) -> UiError {
    match err {
        legacy::Error::Pdm(e) => pdm_error(e),
        other => UiError::usage(other.to_string()),
    }
}

/// How long a quiet port is read for an unasked block before an armed request.
const UNASKED_WAIT: Duration = Duration::from_millis(20);
/// The most items read that way, so a chatty port cannot hold a request.
const UNASKED_ITEMS: usize = 64;

/// How often VBUS is looked at.
const VBUS_CHECK: Duration = Duration::from_millis(100);
/// Below this, V, the charger is gone: PD holds 4.75 V or more across a
/// voltage change, and 0.8 V is its off level.
pub const VBUS_GONE_V: f64 = 2.0;
/// At this, V, VBUS has risen under an accepted contract.
const VBUS_UP_V: f64 = 3.0;
/// How long a low reading after a new contract may still be an old sample.
const VBUS_RISE_WAIT: Duration = Duration::from_secs(1);

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
}

impl std::fmt::Debug for Msg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Msg::Job(_) => "Job",
            Msg::WatchVbus(_) => "WatchVbus",
            Msg::LogTo(_) => "LogTo",
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
        let (open_tx, open_rx) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("trigger".to_owned())
            .spawn(move || {
                let pdm = match PdmSession::open_with_clock(port, clock) {
                    Ok(pdm) => {
                        let _ = open_tx.send(Ok(()));
                        pdm
                    }
                    Err(e) => {
                        let _ = open_tx.send(Err(pdm_error(&e)));
                        return;
                    }
                };
                let mut session = Session {
                    pdm,
                    offers: Vec::new(),
                    shown: Vec::new(),
                    entered: Entered::None,
                    pd_contract: false,
                    vbus_unconfirmed: false,
                    armed: false,
                    powered: Powered::No,
                    gone: false,
                    vbus: None,
                    log: None,
                };
                // Every handle gone without a close: still back to 5 V and close.
                let mut reply = None;
                // Not restarted by a job, so a busy queue cannot hold the check.
                let mut next_check = Instant::now() + VBUS_CHECK;
                loop {
                    let wait = next_check.saturating_duration_since(Instant::now());
                    match rx.recv_timeout(wait) {
                        Ok(Msg::Job(job)) => job(&mut session),
                        Ok(Msg::WatchVbus(read)) => session.vbus = Some(read),
                        Ok(Msg::LogTo(log)) => session.log = Some(log),
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
                let result = session.shutdown();
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
    pub fn watch_vbus(&self, read: impl FnMut() -> Option<f64> + Send + 'static) {
        let _ = self.tx.send(Msg::WatchVbus(Box::new(read)));
    }

    /// Send the session's log lines to `log`.
    pub fn log_to(&self, log: impl FnMut(String) + Send + 'static) {
        let _ = self.tx.send(Msg::LogTo(Box::new(log)));
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

    /// A session whose VBUS watcher reads `Vbus` (9 V to begin with) and whose
    /// log lines are collected.
    fn watched(charger: SimCharger) -> (SimPdm, TriggerWorker, Vbus, Lines) {
        let (sim, w) = worker(charger);
        let vbus: Vbus = Arc::new(Mutex::new(Some(9.0)));
        let reading = Arc::clone(&vbus);
        w.watch_vbus(move || *reading.lock().unwrap());
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

    /// Poll `armed` the way the page does, for up to 1 s of real time.
    fn disarms_within_a_second(c: &TriggerControl) {
        let start = Instant::now();
        while c.armed().expect("armed") {
            assert!(
                start.elapsed() < Duration::from_secs(1),
                "still armed after 1 s"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Wait until the collapse was noticed, for up to 1 s of real time.
    fn gone_within_a_second(c: &TriggerControl) {
        let start = Instant::now();
        while !c.charger_gone().expect("charger gone") {
            assert!(
                start.elapsed() < Duration::from_secs(1),
                "not noticed after 1 s"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
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

    #[test]
    fn a_collapse_ends_armed_within_200_ms_with_no_call_into_the_session() {
        let (sim, w, vbus, lines) = watched_armed(SimCharger::Epr140W);
        let c = w.control();
        let before = sim.writes().len();
        reads(&vbus, 0.0);
        let start = Instant::now();
        // Only the log is watched: nothing here gives the session a turn.
        while ended_lines(&lines).is_empty() {
            assert!(
                start.elapsed() < Duration::from_millis(200),
                "armed did not end within 200 ms"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
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
        disarms_within_a_second(&c);
        assert!(c.charger_gone().expect("charger gone"));
        assert_eq!(writes_after(&sim, before), Vec::<String>::new());
        w.close().expect("close");
    }

    #[test]
    fn just_below_2_v_is_a_collapse() {
        let (_sim, w, vbus, _lines) = watched_armed(SimCharger::Epr140W);
        let c = w.control();
        reads(&vbus, 1.99);
        disarms_within_a_second(&c);
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
        std::thread::sleep(THREE_CHECKS);
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
        std::thread::sleep(THREE_CHECKS);
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
        gone_within_a_second(&c);
        w.close().expect("close");
    }

    #[test]
    fn the_way_back_to_5v_counts_as_a_contract() {
        let (_sim, w, vbus, _lines) = watched_contract(SimCharger::Epr140W);
        let c = w.control();
        c.back_to_5v().expect("back to 5 V");
        reads(&vbus, 0.0);
        gone_within_a_second(&c);
        w.close().expect("close");
    }

    #[test]
    fn a_built_message_ends_the_contract() {
        // A Soft Reset may well take VBUS down.
        let (_sim, w, vbus, _lines) = watched_contract(SimCharger::Epr140W);
        let c = w.control();
        c.send_message(soft_reset(), false).expect("sent");
        reads(&vbus, 0.0);
        std::thread::sleep(THREE_CHECKS);
        assert!(!c.charger_gone().expect("charger gone"));
        w.close().expect("close");
    }

    #[test]
    fn a_console_line_that_is_not_read_only_ends_the_contract() {
        let (_sim, w, vbus, _lines) = watched_contract(SimCharger::Epr140W);
        let c = w.control();
        c.console_send("pd hrst".to_owned(), false).expect("sent");
        reads(&vbus, 0.0);
        std::thread::sleep(THREE_CHECKS);
        assert!(!c.charger_gone().expect("charger gone"));
        w.close().expect("close");
    }

    #[test]
    fn a_read_only_console_line_leaves_the_contract() {
        let (_sim, w, vbus, _lines) = watched_contract(SimCharger::Epr140W);
        let c = w.control();
        c.console_send("pdm parm".to_owned(), false).expect("sent");
        reads(&vbus, 0.0);
        gone_within_a_second(&c);
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
        std::thread::sleep(THREE_CHECKS);
        assert!(!c.charger_gone().expect("charger gone"));
        w.close().expect("close");
    }

    // AC-AXHK2N.10: the collapse forgets the offers.

    #[test]
    fn every_request_after_a_collapse_is_refused_with_nothing_written() {
        let (sim, w, vbus, _lines) = watched_armed(SimCharger::Pps65W);
        let c = w.control();
        reads(&vbus, 0.0);
        gone_within_a_second(&c);
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
        gone_within_a_second(&c);
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
        gone_within_a_second(&c);
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
        gone_within_a_second(&c);
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
        gone_within_a_second(&c);
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
        gone_within_a_second(&c);
        reads(&vbus, 5.0);
        c.list(TriggerProtocol::Pd).expect("list pd");
        assert!(!c.charger_gone().expect("charger gone"));
        disarmed(&sim, &c);
        c.request(pd(2), true).expect("9 V again");
        w.close().expect("close");
    }

    #[test]
    fn a_collapse_just_before_an_armed_request_refuses_it() {
        let (sim, w, vbus, _lines) = watched_armed(SimCharger::Epr140W);
        let c = w.control();
        let before = sim.writes().len();
        reads(&vbus, 0.0);
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
        gone_within_two_seconds(&c);
        assert!(!c.armed().expect("armed"));
        let _ = w.close();
    }

    fn gone_within_two_seconds(c: &TriggerControl) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !c.charger_gone().expect("charger gone") {
            assert!(Instant::now() < deadline, "still not gone");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn neither_detect_nor_another_protocol_clears_it() {
        let (_sim, w, vbus, _lines) = watched_armed(SimCharger::Epr140W);
        let c = w.control();
        reads(&vbus, 0.0);
        gone_within_a_second(&c);
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
        gone_within_a_second(&c);
        c.list(TriggerProtocol::Pd).expect("list pd");
        // Still 0 V, and no contract yet: the listing is not undone.
        std::thread::sleep(THREE_CHECKS);
        assert!(!c.charger_gone().expect("charger gone"));
        reads(&vbus, 9.0);
        c.request(pd(2), true).expect("9 V");
        std::thread::sleep(THREE_CHECKS);
        assert!(!c.charger_gone().expect("charger gone"));
        reads(&vbus, 0.0);
        gone_within_a_second(&c);
        w.close().expect("close");
    }

    #[test]
    fn a_collapse_with_nothing_armed_says_nothing_of_armed() {
        let (_sim, w, vbus, lines) = watched_contract(SimCharger::Epr140W);
        let c = w.control();
        reads(&vbus, 0.0);
        gone_within_a_second(&c);
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
        disarms_within_a_second(&c);
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

    // Production wiring: `install_trigger_for` gives the session the meter's
    // log and the stream's latest reading.

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
        w.watch_vbus(move || *reading.lock().unwrap());
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
        std::thread::sleep(Duration::from_millis(300));
        assert!(!c.charger_gone().expect("charger gone"));
        let _ = w.close();
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
        w.watch_vbus(move || *reading.lock().unwrap());
        c.request(pd(2), false).expect("armed, 9 V");
        *vbus.lock().unwrap() = Some(0.0);
        let deadline = Instant::now() + Duration::from_secs(2);
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
