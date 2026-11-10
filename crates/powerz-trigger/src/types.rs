//! What the engine takes from and gives to the UI (serde shapes shared by
//! the desktop app's IPC and the web edition's worker).

use std::time::Duration;

use serde::{Deserialize, Serialize};

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

/// One VBUS reading: the stream's latest record.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VbusReading {
    /// VBUS, V.
    pub volts: f64,
    /// CC1, V: a charger attached pulls one CC line up (FR TFA5N7).
    pub cc1_v: f64,
    /// CC2, V.
    pub cc2_v: f64,
    /// The record's device time, ms: the same time is the same record again.
    pub time_ms: u32,
    /// How far apart the stream's records are.
    pub period: Duration,
}

impl VbusReading {
    /// The reading of the stream record `r`.
    pub fn of(r: &powerz_protocol::decode::QueueRecord) -> VbusReading {
        VbusReading {
            volts: r.vbus_v(),
            cc1_v: r.cc1_v(),
            cc2_v: r.cc2_v(),
            time_ms: r.time_ms(),
            period: Duration::from_millis(u64::from(1000 / r.rate().sps().max(1))),
        }
    }
}

/// Why "don't ask again" ended, for the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum ArmedEnd {
    Cleared,
    BackTo5V,
    LeftView,
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
    /// The reason as the log line says it.
    pub fn text(self) -> &'static str {
        match self {
            ArmedEnd::Cleared => "cleared",
            ArmedEnd::BackTo5V => "back to 5 V",
            ArmedEnd::LeftView => "left the trigger view",
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
