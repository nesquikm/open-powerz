//! The trigger panel's backend (FR DGSQ6S): a PDM text session on the
//! meter's virtual serial port, run on a thread of its own next to the
//! measurement stream. The UI detects protocols, lists offers / limits /
//! PD emulation settings, and requests voltages through it; every command
//! above 5 V needs the user's explicit confirmation, and the backend is the
//! final gate (the library risk classifier decides). Closing, dropping,
//! disconnecting or quitting returns the charger to 5 V where the protocol
//! allows it and sends `pdm close`.

use std::io::{Read, Write};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

use powerz_protocol::pdm::console;
use powerz_protocol::pdm::detect::{self, ListOptions};
use powerz_protocol::pdm::legacy::{self, LegacyProtocol};
use powerz_protocol::pdm::message::{self, CableIdentity, PdMessage, SopTarget};
use powerz_protocol::pdm::{self, HighVoltageConfirmation, PdmSession, pd};
use powerz_protocol::session::Clock;
use serde::{Deserialize, Serialize};

use crate::ui_error::{ErrorKind, UiError};

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

/// A handle to the running trigger session: every call is answered by the
/// session thread. Cheap to clone, so a command never holds the device
/// manager's lock while it waits.
#[derive(Debug, Clone)]
pub struct TriggerControl {
    tx: mpsc::Sender<Msg>,
}

impl TriggerControl {
    /// Run `job` on the session thread and wait for its answer.
    fn call<T: Send + 'static>(
        &self,
        job: impl FnOnce(&mut dyn Engine) -> Result<T, UiError> + Send + 'static,
    ) -> Result<T, UiError> {
        let (reply_tx, reply_rx) = mpsc::channel();
        let job: Job = Box::new(move |engine| {
            let _ = reply_tx.send(job(engine));
        });
        self.tx.send(Msg::Job(job)).map_err(|_| closed())?;
        reply_rx.recv().map_err(|_| closed())?
    }

    /// BC 1.2, Apple and the automatic list; nothing above 5 V is asked.
    pub fn detect(&self) -> Result<Detection, UiError> {
        self.call(|e| e.detect())
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
    /// error, nothing written) unless `confirmed`.
    pub fn request(
        &self,
        request: TriggerRequest,
        confirmed: bool,
    ) -> Result<TriggerReply, UiError> {
        self.call(move |e| e.request(request, confirmed))
    }

    /// Back to 5 V: PD `pd req=1` (Hard Reset as the fallback), the other
    /// protocols `reset`. The session stays open.
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
        confirmed: bool,
    ) -> Result<TriggerReply, UiError>;
    fn back_to_5v(&mut self) -> Result<(), UiError>;
    fn message_risk(&mut self, message: PdMessageRequest) -> Result<RiskClass, UiError>;
    fn send_message(
        &mut self,
        message: PdMessageRequest,
        confirmed: bool,
    ) -> Result<PdSendReply, UiError>;
    fn console_risk(&mut self, line: String) -> Result<RiskClass, UiError>;
    fn console_send(&mut self, line: String, confirmed: bool) -> Result<Vec<ConsoleLine>, UiError>;
}

type Job = Box<dyn FnOnce(&mut dyn Engine) + Send>;

/// What the session thread is asked to do.
enum Msg {
    Job(Job),
    /// Back to 5 V, `pdm close`, answer, and end the thread.
    Close(mpsc::Sender<Result<(), UiError>>),
}

/// What was last entered, i.e. how to get back to 5 V.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Entered {
    None,
    Pd,
    Legacy,
}

/// The PDM session and what the charger offered, owned by the thread.
struct Session<P: Read + Write, C: Clock> {
    pdm: PdmSession<P, C>,
    offers: Vec<pd::Offer>,
    entered: Entered,
    /// A PD request was accepted and not yet undone: it is always ended
    /// through PD (`pd req=1`, Hard Reset as the fallback), whatever was
    /// entered after it.
    pd_contract: bool,
}

impl<P: Read + Write, C: Clock> Session<P, C> {
    /// Back to 5 V where the protocol allows it: PD `pd req=1` (Hard Reset
    /// as the fallback), the others `reset`.
    fn return_to_5v(&mut self) -> Result<(), UiError> {
        let pd = if self.pd_contract || self.entered == Entered::Pd {
            pd::return_to_5v(&mut self.pdm, &self.offers)
                .map(|_| ())
                .map_err(|e| pd_error(&e.hard_reset))
        } else {
            Ok(())
        };
        if pd.is_ok() {
            self.pd_contract = false;
        }
        let legacy = if self.entered == Entered::Legacy {
            legacy::return_to_5v(&mut self.pdm).map_err(|e| legacy_error(&e))
        } else {
            Ok(())
        };
        pd.and(legacy)
    }

    /// Back to 5 V, then `pdm close`; the first error wins.
    fn shutdown(mut self) -> Result<(), UiError> {
        let back = self.return_to_5v();
        let closed = self.pdm.close().map_err(|e| pdm_error(&e));
        back.and(closed)
    }

    fn list_pd(&mut self) -> Result<ProtocolSection, UiError> {
        let negotiation = match pd::enter(&mut self.pdm) {
            Ok(n) => n,
            Err(pd::Error::Pdm(pdm::Error::Timeout { .. })) => {
                return Ok(unsupported(TriggerProtocol::Pd));
            }
            Err(e) => return Err(pd_error(&e)),
        };
        self.entered = Entered::Pd;
        self.offers = negotiation.offers;
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
                if self.pdm.capabilities().pd.is_some() {
                    self.offers = pd::offers(self.pdm.capabilities());
                }
            }
            Some(console::Effect::PdContract) => self.pd_contract = true,
            Some(console::Effect::Legacy) => self.entered = Entered::Legacy,
            Some(console::Effect::RawLine) if self.entered == Entered::Pd => {
                self.pd_contract = true;
            }
            Some(console::Effect::RawLine) | None => {}
        }
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
}

impl<P: Read + Write, C: Clock> Engine for Session<P, C> {
    fn detect(&mut self) -> Result<Detection, UiError> {
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
        })
    }

    fn list(&mut self, protocol: TriggerProtocol) -> Result<ProtocolSection, UiError> {
        match legacy_of(protocol) {
            None => self.list_pd(),
            Some(lp) => self.list_legacy(protocol, lp),
        }
    }

    fn risk(&mut self, request: TriggerRequest) -> Result<RiskClass, UiError> {
        let command = self.command_of(&lib_request(request)?)?;
        Ok(risk_class(self.pdm.risk(&command)))
    }

    fn request(
        &mut self,
        request: TriggerRequest,
        confirmed: bool,
    ) -> Result<TriggerReply, UiError> {
        let lib = lib_request(request)?;
        // Validation first: an invalid request writes nothing.
        let command = self.command_of(&lib)?;
        let above = self.pdm.risk(&command) == pdm::Risk::Above5V;
        // The final gate: above 5 V without the user's confirmation is
        // refused before anything is written.
        if above && !confirmed {
            return Err(UiError::usage(
                "A request above 5 V needs your confirmation.",
            ));
        }
        let confirmation = above.then(|| HighVoltageConfirmation::for_command(&command));
        match lib {
            LibRequest::Pd(r) => {
                let outcome = pd::request(&mut self.pdm, &self.offers, &r, confirmation)
                    .map_err(|e| pd_error(&e))?;
                self.entered = Entered::Pd;
                self.pd_contract = true;
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
                let outcome = legacy::request(&mut self.pdm, &r, confirmation)
                    .map_err(|e| legacy_error(&e))?;
                self.entered = Entered::Legacy;
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

    fn back_to_5v(&mut self) -> Result<(), UiError> {
        self.return_to_5v()
    }

    fn message_risk(&mut self, message: PdMessageRequest) -> Result<RiskClass, UiError> {
        let command = lib_message(&message)
            .to_command()
            .map_err(|e| UiError::usage(e.to_string()))?;
        Ok(risk_class(self.pdm.risk(&command)))
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
        let above = self.pdm.risk(&command) == pdm::Risk::Above5V;
        // The final gate: above 5 V without the user's confirmation is
        // refused before anything is written.
        if above && !confirmed {
            return Err(UiError::usage(
                "A message that can raise VBUS above 5 V needs your confirmation.",
            ));
        }
        if self.entered == Entered::None {
            // Like `powerz pd send`: PD first, so the message has a PD
            // session and the way back to 5 V (`pd req=1`, Hard Reset as
            // the fallback) works.
            let negotiation = pd::enter(&mut self.pdm).map_err(|e| pd_error(&e))?;
            self.entered = Entered::Pd;
            self.offers = negotiation.offers;
        }
        if above {
            // It may change the contract: always end it through PD. Set
            // before the write, so a failure after it still undoes it.
            self.pd_contract = true;
        }
        let at_ms = self.pdm.now_ms();
        let sent = match above.then(|| HighVoltageConfirmation::for_command(&command)) {
            Some(c) => self.pdm.send_confirmed(&command, c),
            None => self.pdm.send(&command),
        };
        sent.map_err(|e| pdm_error(&e))?;
        // A Discover Identity to a cable plug: read its ACK, if any.
        let cable = if lib.is_cable_discover_identity() {
            message::await_cable_identity(&mut self.pdm, message::CABLE_WAIT)
                .map_err(|e| pdm_error(&e))?
                .map(|c| cable_view(&c))
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

    fn console_risk(&mut self, line: String) -> Result<RiskClass, UiError> {
        Ok(risk_class(self.pdm.risk(&console::command_of(&line))))
    }

    fn console_send(&mut self, line: String, confirmed: bool) -> Result<Vec<ConsoleLine>, UiError> {
        let command = console::command_of(&line);
        let above = self.pdm.risk(&command) == pdm::Risk::Above5V;
        // The final gate: above 5 V (any unparsable line too) without the
        // user's confirmation is refused before anything is written.
        if above && !confirmed {
            return Err(UiError::usage(
                "A line that can raise VBUS above 5 V needs your confirmation.",
            ));
        }
        let confirmation = above.then(|| HighVoltageConfirmation::for_command(&command));
        let sent = console::send_line(&mut self.pdm, &line, confirmation, console::LISTEN)
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

/// The library's cable identity as the UI's.
fn cable_view(c: &CableIdentity) -> CableView {
    CableView {
        vid: c.vid,
        pid: c.pid,
        active: c.active,
        max_voltage_v: c.max_voltage_v,
        max_current_ma: c.max_current_ma,
        epr_capable: c.epr_capable,
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
pub(crate) fn pdm_error(err: &pdm::Error) -> UiError {
    use pdm::Error as E;
    match err {
        E::Timeout { .. } => UiError {
            kind: ErrorKind::Timeout,
            message: "The meter did not answer in time.".to_owned(),
            hint: Some("Check the cable, then try again.".to_owned()),
        },
        E::Disconnected | E::NotFound => UiError {
            kind: ErrorKind::Disconnected,
            message: "The meter's serial port went away.".to_owned(),
            hint: Some("Plug the meter back in.".to_owned()),
        },
        E::HighVoltageNotConfirmed { .. } => {
            UiError::usage("A request above 5 V needs your confirmation.")
        }
        E::Refused { reason, .. } => UiError::usage(reason.clone()),
        E::Forbidden { .. } | E::UnsupportedOnCdc { .. } => {
            UiError::usage("The app refused a command that is not safe.")
        }
        E::Ambiguous { .. } => UiError::usage("Several meters are attached; connect only one."),
        E::Malformed { .. } | E::LineTooLong { .. } | E::UnexpectedReply { .. } => UiError {
            kind: ErrorKind::Protocol,
            message: "The meter sent a reply the app did not understand.".to_owned(),
            hint: None,
        },
        E::Io(_) => UiError {
            kind: ErrorKind::Internal,
            message: "Talking to the meter's serial port failed.".to_owned(),
            hint: Some("Replug the meter and try again.".to_owned()),
        },
    }
}

fn pd_error(err: &pd::Error) -> UiError {
    match err {
        pd::Error::Pdm(e) => pdm_error(e),
        other => UiError::usage(other.to_string()),
    }
}

fn legacy_error(err: &legacy::Error) -> UiError {
    match err {
        legacy::Error::Pdm(e) => pdm_error(e),
        other => UiError::usage(other.to_string()),
    }
}

/// The trigger session's thread. [`TriggerWorker::close`] or a drop
/// returns the charger to 5 V and sends `pdm close`.
#[derive(Debug)]
pub struct TriggerWorker {
    tx: mpsc::Sender<Msg>,
    thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for Msg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Msg::Job(_) => "Job",
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
                    entered: Entered::None,
                    pd_contract: false,
                };
                // Every handle gone without a close: still back to 5 V and close.
                let mut reply = None;
                while let Ok(msg) = rx.recv() {
                    match msg {
                        Msg::Job(job) => job(&mut session),
                        Msg::Close(r) => {
                            reply = Some(r);
                            break;
                        }
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
        }
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
            }),
        }
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
        assert_eq!(c.risk(pd(2)).expect("risk"), RiskClass::Above5V);
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
        assert_eq!(c.risk(req).expect("risk"), RiskClass::Above5V);
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
