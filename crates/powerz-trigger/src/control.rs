//! The thin rules both front ends apply around the engine (FR W8SHXC: one
//! implementation). The desktop app's `TriggerControl` and the web edition's
//! binding call these, so the two cannot drift apart:
//!
//! - [`Confirm::of`]: the dialog's answer, "don't ask again" only when
//!   confirmed.
//! - [`guarded`]: a port failure (`disconnected` / `internal`) ends "don't
//!   ask again", since the meter may be gone.
//! - [`leave_view`]: leaving the trigger view ends "don't ask again" with its
//!   own reason, then goes back to 5 V without waiting for offers.
//! - [`console_refusal`]: a GoodCRC console line is refused before the line
//!   reaches the engine.

use powerz_protocol::pdm::{self, message};

use crate::error::{TriggerError, TriggerErrorKind};
use crate::session::Engine;
use crate::types::{ArmedEnd, Confirm};

impl Confirm {
    /// The above-5 V dialog's answer: `remember` ("don't ask again" checked)
    /// counts only with `confirmed`.
    pub fn of(confirmed: bool, remember: bool) -> Confirm {
        match (confirmed, remember) {
            (true, true) => Confirm::YesRemember,
            (confirmed, _) => Confirm::from(confirmed),
        }
    }
}

/// Whether `error` means the port failed: the meter may be gone.
pub fn port_failed(error: &TriggerError) -> bool {
    matches!(
        error.kind,
        TriggerErrorKind::Disconnected | TriggerErrorKind::Internal
    )
}

/// Run `job` on `engine`; a port failure ends "don't ask again".
pub fn guarded<E: Engine + ?Sized, T>(
    engine: &mut E,
    job: impl FnOnce(&mut E) -> Result<T, TriggerError>,
) -> Result<T, TriggerError> {
    let result = job(engine);
    if let Err(e) = &result
        && port_failed(e)
    {
        engine.disarm(ArmedEnd::PortFailed);
    }
    result
}

/// The trigger view was left: "don't ask again" ends with its own reason,
/// then back to 5 V (no wait for a Hard Reset's offers). The session stays
/// open for the close that follows.
pub fn leave_view<E: Engine + ?Sized>(engine: &mut E) -> Result<(), TriggerError> {
    engine.disarm(ArmedEnd::LeftView);
    engine.leave_to_5v()
}

/// Why the raw console line `line` is never sent (GoodCRC in any spelling),
/// as a usage error; `None` when the engine may judge it.
pub fn console_refusal(line: &str) -> Option<TriggerError> {
    message::refusal(&pdm::Command::parse(line.trim())).map(TriggerError::usage)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        CableRead, ConsoleLine, Detection, PdMessageRequest, PdSendReply, ProtocolSection,
        RiskClass, TriggerProtocol, TriggerReply, TriggerRequest,
    };

    /// An engine that records what was asked of it and answers `fail`.
    #[derive(Default)]
    struct Fake {
        calls: Vec<String>,
        fail: Option<TriggerErrorKind>,
    }

    impl Fake {
        fn answer(&mut self, call: &str) -> Result<(), TriggerError> {
            self.calls.push(call.to_owned());
            match self.fail.clone() {
                Some(kind) => Err(TriggerError {
                    kind,
                    message: "failed".to_owned(),
                    hint: None,
                }),
                None => Ok(()),
            }
        }
        fn unused<T>(&mut self) -> Result<T, TriggerError> {
            Err(TriggerError::usage("not used by these tests"))
        }
    }

    impl Engine for Fake {
        fn detect(&mut self) -> Result<Detection, TriggerError> {
            self.unused()
        }
        fn list(&mut self, _: TriggerProtocol) -> Result<ProtocolSection, TriggerError> {
            self.unused()
        }
        fn risk(&mut self, _: TriggerRequest) -> Result<RiskClass, TriggerError> {
            self.unused()
        }
        fn request(&mut self, _: TriggerRequest, _: Confirm) -> Result<TriggerReply, TriggerError> {
            self.unused()
        }
        fn armed(&mut self) -> bool {
            false
        }
        fn disarm(&mut self, why: ArmedEnd) {
            self.calls.push(format!("disarm {}", why.text()));
        }
        fn charger_gone(&mut self) -> bool {
            false
        }
        fn pd_entered(&mut self) -> bool {
            false
        }
        fn back_to_5v(&mut self) -> Result<(), TriggerError> {
            self.answer("back_to_5v")
        }
        fn leave_to_5v(&mut self) -> Result<(), TriggerError> {
            self.answer("leave_to_5v")
        }
        fn message_risk(&mut self, _: PdMessageRequest) -> Result<RiskClass, TriggerError> {
            self.unused()
        }
        fn send_message(
            &mut self,
            _: PdMessageRequest,
            _: bool,
        ) -> Result<PdSendReply, TriggerError> {
            self.unused()
        }
        fn cable_read(&mut self) -> Result<CableRead, TriggerError> {
            self.unused()
        }
        fn console_risk(&mut self, _: String) -> Result<RiskClass, TriggerError> {
            self.unused()
        }
        fn console_send(&mut self, _: String, _: bool) -> Result<Vec<ConsoleLine>, TriggerError> {
            self.unused()
        }
    }

    #[test]
    fn remember_counts_only_with_a_confirmation() {
        assert_eq!(Confirm::of(true, true), Confirm::YesRemember);
        assert_eq!(Confirm::of(true, false), Confirm::Yes);
        assert_eq!(Confirm::of(false, true), Confirm::No);
        assert_eq!(Confirm::of(false, false), Confirm::No);
    }

    #[test]
    fn a_port_failure_disarms_and_other_errors_do_not() {
        for (kind, disarms) in [
            (TriggerErrorKind::Disconnected, true),
            (TriggerErrorKind::Internal, true),
            (TriggerErrorKind::Usage, false),
            (TriggerErrorKind::Timeout, false),
            (TriggerErrorKind::Protocol, false),
            (TriggerErrorKind::OutOfSync, false),
            (TriggerErrorKind::DeviceBusy, false),
        ] {
            let mut fake = Fake {
                fail: Some(kind.clone()),
                ..Fake::default()
            };
            let result = guarded(&mut fake, |e| e.back_to_5v());
            assert_eq!(result.map_err(|e| e.kind), Err(kind.clone()));
            let mut want = vec!["back_to_5v".to_owned()];
            if disarms {
                want.push("disarm port failed".to_owned());
            }
            assert_eq!(fake.calls, want, "{kind:?}");
        }
    }

    #[test]
    fn a_success_does_not_disarm_and_works_through_dyn_engine() {
        let mut fake = Fake::default();
        let engine: &mut dyn Engine = &mut fake;
        assert_eq!(guarded(engine, |e| e.back_to_5v()), Ok(()));
        assert_eq!(fake.calls, ["back_to_5v"]);
    }

    #[test]
    fn leaving_the_view_disarms_first_then_goes_back_to_5v() {
        let mut fake = Fake::default();
        assert_eq!(leave_view(&mut fake), Ok(()));
        assert_eq!(fake.calls, ["disarm left the trigger view", "leave_to_5v"]);
    }

    #[test]
    fn a_good_crc_console_line_is_refused_as_a_usage_error() {
        for line in ["pd cmd=1", "  pd cmd=33 ", "pd data=004100"] {
            let refusal = console_refusal(line).expect("GoodCRC is refused");
            assert_eq!(refusal.kind, TriggerErrorKind::Usage, "{line}");
            assert_eq!(refusal.hint, None);
            assert!(!refusal.message.is_empty());
        }
        for line in ["pd cmd=13", "pd req=1", "pdm close"] {
            assert_eq!(console_refusal(line), None, "{line}");
        }
    }
}
