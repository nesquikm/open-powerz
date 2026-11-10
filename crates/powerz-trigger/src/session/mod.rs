//! The PDM session the engine runs: detection, listing, requests, the
//! above-5 V gate and the way back to 5 V.

mod watch;

use std::io::{Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use powerz_protocol::pdm::console;
use powerz_protocol::pdm::detect::{self, ListOptions};
use powerz_protocol::pdm::legacy::{self, LegacyProtocol};
use powerz_protocol::pdm::message;
use powerz_protocol::pdm::{self, HighVoltageConfirmation, PdmSession, pd};
use powerz_protocol::session::Clock;

pub use self::watch::{CC_GONE_V, VBUS_GONE_V, VBUS_LOW_SPELL, VBUS_RISE_WAIT, WatchClock};
use self::watch::{Powered, Spell, VbusReader};
use crate::convert::{
    CABLE_READ, LibRequest, cable_view, command_text, console_line, is_cable_read, legacy_of,
    lib_message, lib_request, limits_view, offer_view, offers_line, risk_class, unsupported,
};
use crate::error::{
    TriggerError, charger_went_away, legacy_error, not_confirmed, offers_changed, pd_error,
    pdm_error,
};
use crate::log::{LoggedPort, SharedLog, log_shared};
use crate::types::{
    ArmedEnd, CableRead, CableView, Confirm, ConsoleDirection, ConsoleLine, DetectEntry, Detection,
    PdEmulation, PdMessageRequest, PdSendReply, ProtocolSection, RiskClass, TriggerProtocol,
    TriggerReply, TriggerRequest,
};

/// The session's calls (erases the port / clock types): the desktop app runs
/// them on its session thread, the web edition in its worker.
pub trait Engine {
    fn detect(&mut self) -> Result<Detection, TriggerError>;
    fn list(&mut self, protocol: TriggerProtocol) -> Result<ProtocolSection, TriggerError>;
    fn risk(&mut self, request: TriggerRequest) -> Result<RiskClass, TriggerError>;
    fn request(
        &mut self,
        request: TriggerRequest,
        confirm: Confirm,
    ) -> Result<TriggerReply, TriggerError>;
    fn armed(&mut self) -> bool;
    fn disarm(&mut self, why: ArmedEnd);
    fn charger_gone(&mut self) -> bool;
    fn pd_entered(&mut self) -> bool;
    fn back_to_5v(&mut self) -> Result<(), TriggerError>;
    /// [`Self::back_to_5v`] for a session about to close: no wait for the
    /// Hard Reset's offers (FR ZRJ8W6).
    fn leave_to_5v(&mut self) -> Result<(), TriggerError>;
    fn message_risk(&mut self, message: PdMessageRequest) -> Result<RiskClass, TriggerError>;
    fn send_message(
        &mut self,
        message: PdMessageRequest,
        confirmed: bool,
    ) -> Result<PdSendReply, TriggerError>;
    fn cable_read(&mut self) -> Result<CableRead, TriggerError>;
    fn console_risk(&mut self, line: String) -> Result<RiskClass, TriggerError>;
    fn console_send(
        &mut self,
        line: String,
        confirmed: bool,
    ) -> Result<Vec<ConsoleLine>, TriggerError>;
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
pub struct Session<P: Read + Write, C: Clock> {
    pdm: PdmSession<LoggedPort<P>, C>,
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
    /// The port's command count right after a way back to 5 V succeeded:
    /// while it is unchanged nothing went out since, so the close need not
    /// send another (AC-AXHK2P.15).
    sent: Arc<AtomicUsize>,
    back_at: Option<usize>,
    /// "Don't ask again" (FR AXHK2N): set only by a request that succeeded
    /// and qualified. It covers only what `pd::rememberable` allows.
    armed: bool,
    /// Whether a low VBUS reading would mean anything.
    powered: Powered,
    /// The current spell of records below [`VBUS_GONE_V`], V; a reading at or
    /// above it ends the spell.
    low_spell: Spell<f64>,
    /// The last record counted: its device time, and when it was first read.
    last_record: Option<(u32, u64)>,
    /// The current spell of records with both CC lines below [`CC_GONE_V`]:
    /// CC1 and CC2, V (FR TFA5N7).
    cc_spell: Spell<(f64, f64)>,
    /// VBUS collapsed under an accepted contract: the offers are forgotten and
    /// every request is refused until a PD listing.
    gone: bool,
    vbus: Option<VbusReader>,
    /// What times the VBUS watch, ms; the session clock when unset.
    watch_clock: Option<WatchClock>,
    log: SharedLog,
}

impl<P: Read + Write, C: Clock> Session<P, C> {
    /// Open the PDM session on `port` (handshake included), its log kept
    /// until a sink is set with [`crate::set_log_sink`].
    pub fn open(port: P, clock: C) -> Result<Session<P, C>, TriggerError> {
        Session::open_logged(port, clock, SharedLog::default())
    }

    /// [`Session::open`], every command logged to `log`.
    pub fn open_logged(port: P, clock: C, log: SharedLog) -> Result<Session<P, C>, TriggerError> {
        let port = LoggedPort::new(port, Arc::clone(&log));
        let sent = port.sent();
        let pdm = PdmSession::open_with_clock(port, clock).map_err(|e| pdm_error(&e))?;
        Ok(Session {
            pdm,
            offers: Vec::new(),
            shown: Vec::new(),
            entered: Entered::None,
            pd_contract: false,
            vbus_unconfirmed: false,
            sent,
            back_at: None,
            armed: false,
            powered: Powered::No,
            low_spell: Spell::new(),
            cc_spell: Spell::new(),
            last_record: None,
            gone: false,
            vbus: None,
            watch_clock: None,
            log,
        })
    }
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
        log_shared(&self.log, line);
    }

    /// Back to 5 V where the protocol allows it: PD `pd req=1` (Hard Reset
    /// as the fallback), the others `reset`. With the charger gone the PD
    /// way back is skipped (FR JV54SQ); `reset` still goes out. Out of sync,
    /// the library's safety path writes `pd hrst` / `reset` blind and `pdm
    /// close` follows. Disarms before anything is written, so a failed way
    /// back ends "don't ask again" too. `closing` (leaving the tab, the
    /// close): the Hard Reset fallback goes out on the safety path and its
    /// offers are not waited for (FR ZRJ8W6).
    fn return_to_5v(&mut self, closing: bool) -> Result<(), TriggerError> {
        self.end_armed(ArmedEnd::BackTo5V);
        // Until it is accepted: a Hard Reset takes VBUS down on purpose.
        self.powered = Powered::No;
        if let Err(e) = self.pdm.check_in_step() {
            return Err(self.blind_return_to_5v(&e));
        }
        // With the charger gone (FR JV54SQ) there is no contract to end: a
        // 5 V request and a Hard Reset would only wait for a charger that
        // is not there. One plugged in later starts at 5 V on its own.
        let pd = if !self.gone && (self.pd_contract || self.entered == Entered::Pd) {
            let back = if closing {
                pd::return_to_5v_closing(&mut self.pdm, &self.offers)
            } else {
                pd::return_to_5v(&mut self.pdm, &self.offers)
            };
            back.map(|back| matches!(back, pd::BackTo5V::Requested))
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
        let back = pd.map(|_| ()).and(legacy);
        // A way back that ran to its end counts as sent: the close does not
        // repeat one that leaving the tab sent, whatever its outcome
        // (AC-ZRJ8W6.4). Pressing Back to 5 V keeps the old rule.
        if back.is_ok() || closing {
            self.back_at = Some(self.sent.load(Ordering::Relaxed));
        }
        back
    }

    /// A way back to 5 V went out (succeeded, or ran to its end on the way
    /// to the close) and nothing was written since.
    fn at_5v(&self) -> bool {
        self.back_at == Some(self.sent.load(Ordering::Relaxed))
    }

    /// The meter is out of step, so no reply can confirm 5 V: send the way
    /// back blind (`pd hrst` for PD, `reset` for the others), close the PDM
    /// session, and say what was done. Sent once; later calls only report.
    fn blind_return_to_5v(&mut self, err: &pdm::Error) -> TriggerError {
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

    /// Back to 5 V, then `pdm close`; the first error wins. The way back is
    /// skipped when one already went out and nothing was written since
    /// (leaving the tab sends it just before the close).
    pub fn shutdown(mut self) -> Result<(), TriggerError> {
        self.end_armed(ArmedEnd::SessionClosed);
        let back = if self.at_5v() {
            Ok(())
        } else {
            self.return_to_5v(true)
        };
        let closed = self.pdm.close().map_err(|e| pdm_error(&e));
        back.and(closed)
    }

    fn list_pd(&mut self) -> Result<ProtocolSection, TriggerError> {
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
    ) -> Result<ProtocolSection, TriggerError> {
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
    /// refreshed offers replace the old ones. (AC-DGSQ6S.13) An entry whose
    /// offers do not come in time is sent once more: the charger is sometimes
    /// slower straight after a listing. \[inferred\] (AC-AHTZW1.1)
    fn enter_pd(&mut self) -> Result<(), TriggerError> {
        let entered = match pd::ensure_entered(&mut self.pdm) {
            Err(pd::Error::Pdm(pdm::Error::Timeout { .. })) => pd::enter(&mut self.pdm).map(Some),
            other => other,
        };
        if let Some(negotiation) = entered.map_err(|e| pd_error(&e))? {
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
    fn await_cable(&mut self) -> Result<Option<CableView>, TriggerError> {
        let cable = message::await_cable_identity(&mut self.pdm, message::CABLE_WAIT)
            .map_err(|e| pdm_error(&e))?;
        Ok(cable.map(|c| cable_view(&c)))
    }

    /// Validate `request` against what the charger offered and render its
    /// command; pure, nothing is written.
    fn command_of(&self, request: &LibRequest) -> Result<pdm::Command, TriggerError> {
        match request {
            LibRequest::Pd(r) => pd::validate(&self.offers, r).map_err(|e| pd_error(&e)),
            LibRequest::Legacy(r) => {
                legacy::validate(self.pdm.capabilities(), r).map_err(|e| legacy_error(&e))
            }
        }
    }

    /// The refusals of `request`; nothing is written and `armed` stands.
    fn judge(&mut self, request: TriggerRequest, confirm: Confirm) -> Result<Judged, TriggerError> {
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
            return Err(TriggerError::usage("This request cannot be remembered."));
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
    ) -> Result<TriggerReply, TriggerError> {
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
    fn detect(&mut self) -> Result<Detection, TriggerError> {
        // Before the first write, so a failed detection ends it too. The
        // automatic list drops VBUS while it probes the charger.
        self.end_armed(ArmedEnd::Detect);
        self.powered = Powered::No;
        // The probes pull the CC lines about and the stream pauses meanwhile:
        // a CC spell from before says nothing (AC-TFA5N7.2).
        self.end_cc_spell();
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

    fn list(&mut self, protocol: TriggerProtocol) -> Result<ProtocolSection, TriggerError> {
        match legacy_of(protocol) {
            None => self.list_pd(),
            Some(lp) => self.list_legacy(protocol, lp),
        }
    }

    fn risk(&mut self, request: TriggerRequest) -> Result<RiskClass, TriggerError> {
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
    ) -> Result<TriggerReply, TriggerError> {
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

    fn pd_entered(&mut self) -> bool {
        // The same test as the cable read's.
        self.pdm.entered() == Some(pdm::Protocol::Pd)
    }

    fn back_to_5v(&mut self) -> Result<(), TriggerError> {
        self.return_to_5v(false)
    }

    fn leave_to_5v(&mut self) -> Result<(), TriggerError> {
        self.return_to_5v(true)
    }

    fn message_risk(&mut self, message: PdMessageRequest) -> Result<RiskClass, TriggerError> {
        let command = lib_message(&message)
            .to_command()
            .map_err(|e| TriggerError::usage(e.to_string()))?;
        Ok(risk_class(self.message_class(&command)))
    }

    fn send_message(
        &mut self,
        message: PdMessageRequest,
        confirmed: bool,
    ) -> Result<PdSendReply, TriggerError> {
        // Building first: GoodCRC and malformed messages write nothing.
        let lib = lib_message(&message);
        let command = lib
            .to_command()
            .map_err(|e| TriggerError::usage(e.to_string()))?;
        let above = self.message_class(&command) == pdm::Risk::Above5V;
        // The final gate: above 5 V without the user's confirmation is
        // refused before anything is written.
        if above && !confirmed {
            return Err(TriggerError::usage(
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

    fn cable_read(&mut self) -> Result<CableRead, TriggerError> {
        // What the meter entered last, not what this session last listed: the
        // read goes out only on a PD session already open.
        if self.pdm.entered() != Some(pdm::Protocol::Pd) {
            return Err(TriggerError::usage(
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

    fn console_risk(&mut self, line: String) -> Result<RiskClass, TriggerError> {
        Ok(risk_class(self.pdm.risk(&console::command_of(&line))))
    }

    fn console_send(
        &mut self,
        line: String,
        confirmed: bool,
    ) -> Result<Vec<ConsoleLine>, TriggerError> {
        let command = console::command_of(&line);
        let risk = self.pdm.risk(&command);
        let above = risk == pdm::Risk::Above5V;
        // The final gate: above 5 V (any unparsable line too) without the
        // user's confirmation is refused before anything is written.
        if above && !confirmed {
            return Err(TriggerError::usage(
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

/// How long a quiet port is read for an unasked block before an armed request.
const UNASKED_WAIT: Duration = Duration::from_millis(20);
/// The most items read that way, so a chatty port cannot hold a request.
const UNASKED_ITEMS: usize = 64;
