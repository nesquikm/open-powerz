//! The engine's plain-language errors and how library errors map to them.

use powerz_protocol::pdm::{self, legacy, pd};
use serde::Serialize;

/// Error category, serialized with the same names as the desktop app's
/// error kinds.
// Not `Copy`: a kind is read off its error, which then goes on whole.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerErrorKind {
    /// The meter's serial port is held by another program.
    DeviceBusy,
    /// The meter did not answer in time.
    Timeout,
    /// The meter answered something unexpected.
    Protocol,
    /// The meter's serial port went away.
    Disconnected,
    /// The request itself is not valid right now.
    Usage,
    /// The meter's text replies are out of step with the commands: it needs
    /// a power cycle, and the session refuses requests until reopened.
    OutOfSync,
    /// Anything else.
    Internal,
}

/// A plain-language trigger error: `{ kind, message, hint }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TriggerError {
    /// Category.
    pub kind: TriggerErrorKind,
    /// What happened, in plain language.
    pub message: String,
    /// What to try next, if anything.
    pub hint: Option<String>,
}

impl TriggerError {
    /// A usage error (the request is not valid right now).
    pub fn usage(message: impl Into<String>) -> TriggerError {
        TriggerError {
            kind: TriggerErrorKind::Usage,
            message: message.into(),
            hint: None,
        }
    }
}

impl std::fmt::Display for TriggerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for TriggerError {}

/// The meter's serial port is held by another program.
fn port_held() -> TriggerError {
    TriggerError {
        kind: TriggerErrorKind::DeviceBusy,
        message: "The meter's serial port is in use by another program.".to_owned(),
        hint: Some("Close that program, then try again.".to_owned()),
    }
}

/// The refusal of a request above 5 V that nothing confirmed.
pub(crate) fn not_confirmed() -> TriggerError {
    TriggerError::usage("A request above 5 V needs your confirmation.")
}

/// The refusal of a request held on offers that changed.
pub(crate) fn offers_changed() -> TriggerError {
    TriggerError::usage("The charger's offers changed, so the request was not sent. Press Detect.")
}

/// The refusal of a request after a collapse, until a PD listing.
pub(crate) fn charger_went_away() -> TriggerError {
    TriggerError::usage("The charger went away. Press Detect.")
}

/// A PDM session error, in plain language.
pub fn pdm_error(err: &pdm::Error) -> TriggerError {
    use pdm::Error as E;
    match err {
        E::Timeout { .. } => TriggerError {
            kind: TriggerErrorKind::Timeout,
            message: "The meter did not answer in time.".to_owned(),
            hint: Some("Check the cable, then try again.".to_owned()),
        },
        E::Disconnected | E::NotFound => TriggerError {
            kind: TriggerErrorKind::Disconnected,
            message: "The meter's serial port went away.".to_owned(),
            hint: Some("Plug the meter back in.".to_owned()),
        },
        E::PortHeld => port_held(),
        E::HighVoltageNotConfirmed { .. } => not_confirmed(),
        E::Refused { reason, .. } => TriggerError::usage(reason.clone()),
        E::Forbidden { .. } | E::UnsupportedOnCdc { .. } => {
            TriggerError::usage("The app refused a command that is not safe.")
        }
        E::Ambiguous { .. } => {
            TriggerError::usage("Several meters are attached; connect only one.")
        }
        E::OutOfSync { .. } => TriggerError {
            kind: TriggerErrorKind::OutOfSync,
            message: "The meter's replies are out of step with the commands, so no more \
                      requests are sent."
                .to_owned(),
            hint: Some(
                "Power-cycle the meter (unplug it and plug it back in), then open the \
                 session again."
                    .to_owned(),
            ),
        },
        E::Malformed { .. } | E::LineTooLong { .. } | E::UnexpectedReply { .. } => TriggerError {
            kind: TriggerErrorKind::Protocol,
            message: "The meter sent a reply the app did not understand.".to_owned(),
            hint: None,
        },
        E::Io(_) => TriggerError {
            kind: TriggerErrorKind::Internal,
            message: "Talking to the meter's serial port failed.".to_owned(),
            hint: Some("Replug the meter and try again.".to_owned()),
        },
    }
}

/// A PD request error, in plain language.
pub(crate) fn pd_error(err: &pd::Error) -> TriggerError {
    match err {
        pd::Error::Pdm(e) => pdm_error(e),
        pd::Error::RequestFailed { reply } => TriggerError {
            kind: TriggerErrorKind::Protocol,
            message: format!("The charger refused the request ({reply})."),
            hint: None,
        },
        other => TriggerError::usage(other.to_string()),
    }
}

/// A non-PD request error, in plain language.
pub(crate) fn legacy_error(err: &legacy::Error) -> TriggerError {
    match err {
        legacy::Error::Pdm(e) => pdm_error(e),
        other => TriggerError::usage(other.to_string()),
    }
}
