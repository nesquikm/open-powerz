//! The protocol trigger engine (FR DGSQ6S, shared by FR W8SHXC): a PDM text
//! session on the meter's virtual serial port. It detects protocols, lists
//! offers / limits / PD emulation settings, and requests voltages; every
//! command above 5 V needs the user's explicit confirmation, and the engine
//! is the final gate (the library risk classifier decides). Closing returns
//! the charger to 5 V where the protocol allows it and sends `pdm close`.
//!
//! The engine is blocking and I/O-agnostic: any `Read + Write` port and any
//! [`Clock`](powerz_protocol::session::Clock). The desktop app runs it on a
//! thread of its own next to the measurement stream; the web edition runs the
//! same code as wasm in a worker. It has no native transport, so it builds
//! for wasm32.
//!
//! - [`Session`] is the engine; its calls are the [`Engine`] trait, which
//!   erases the port and clock types.
//! - The request / reply shapes ([`TriggerRequest`], [`ProtocolSection`],
//!   [`TriggerReply`], ...) are what both front ends send over their IPC.
//! - Errors are plain-language [`TriggerError`]s.
//! - Every command written is logged through a [`SharedLog`] (see
//!   [`set_log_sink`]); [`Session::watch_vbus`] lets a collapse end what
//!   stood on the charger with no press.
//! - The thin rules both front ends apply around the engine ([`guarded`],
//!   [`leave_view`], [`console_refusal`], [`Confirm::of`]) live here too, so
//!   the desktop app and the web edition share one implementation.

mod control;
mod convert;
mod error;
mod log;
mod session;
mod types;

pub use control::{console_refusal, guarded, leave_view, port_failed};
pub use convert::{cable_view, command_text, is_cable_read, lib_message, risk_class};
pub use error::{TriggerError, TriggerErrorKind, pdm_error};
pub use log::{LogSink, LoggedPort, SessionLogLines, SharedLog, log_shared, set_log_sink};
pub use session::{
    CC_GONE_V, Engine, Session, VBUS_GONE_V, VBUS_LOW_SPELL, VBUS_RISE_WAIT, WatchClock,
};
pub use types::{
    ArmedEnd, CableRead, CableView, Confirm, ConsoleDirection, ConsoleLine, DetectEntry, Detection,
    LimitsView, MessageSop, OfferView, PdEmulation, PdMessageRequest, PdSendReply, ProtocolSection,
    RiskClass, TriggerProtocol, TriggerReply, TriggerRequest, VbusReading,
};
