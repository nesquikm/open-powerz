//! `powerz trigger pd ...` (FR DGSQ6P): USB PD triggering through the
//! meter's PDM text session on its virtual serial port.

use std::io::{Read, Write};
use std::time::Duration;

use powerz_protocol::pdm::legacy::{self, LegacyProtocol, Request as LegacyRequest};
use powerz_protocol::pdm::pd::{
    self, Emarker, Offer, OfferKind, PdSettings, PdType, Reading, Request, RequestOutcome, SinkCaps,
};
use powerz_protocol::pdm::{Command, HighVoltageConfirmation, PdmSession, Risk};
use powerz_protocol::session::Clock;

use super::close_after;
use crate::error::CliError;
use crate::output::write_json;

/// `powerz trigger pd list`: `entry pd`, then the charger's offers decoded,
/// one line per position (or one JSON object with `epr` and `offers`).
pub fn trigger_pd_list<P: Read + Write, C: Clock>(
    mut session: PdmSession<P, C>,
    json: bool,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let negotiation = pd::enter(&mut session)?;
    session.close()?;
    write_offers(&negotiation.offers, json, out)
}

/// `powerz trigger pd reset`: `entry pd`, then `pd hrst`; print the offers
/// of the renegotiation and VBUS once ready (back at 5 V), and close the
/// session, on error too.
pub fn trigger_pd_reset<P: Read + Write, C: Clock>(
    mut session: PdmSession<P, C>,
    json: bool,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    // A failed `entry pd` drops the session, which sends `pdm close`.
    pd::enter(&mut session)?;
    let result = pd::hard_reset(&mut session).map_err(CliError::from);
    let negotiation = close_after(session, result)?;
    let vbus = negotiation.vbus;
    if json {
        let mut v = offers_json(&negotiation.offers);
        v["vbus_mv"] = vbus.voltage_mv.into();
        v["vbus_ma"] = vbus.current_ma.into();
        return write_json(out, &v);
    }
    write_offers(&negotiation.offers, false, out)?;
    writeln!(out, "VBUS     {}", volts_amps(&vbus))?;
    Ok(())
}

/// `powerz trigger pd settings` changes, by name; `None` keeps the current value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PdSettingsChange<'a> {
    /// `--type`.
    pub pd_type: Option<&'a str>,
    /// `--em`.
    pub emarker: Option<&'a str>,
    /// `--sink`.
    pub sink: Option<&'a str>,
}

/// `powerz trigger pd settings`: `pdm parm`, then (when a flag is given)
/// `pdm set` with the changed values and `pdm parm` again; print the
/// settings by name and close the session, on error too.
pub fn trigger_pd_settings<P: Read + Write, C: Clock>(
    mut session: PdmSession<P, C>,
    change: &PdSettingsChange<'_>,
    json: bool,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    // Names are checked before anything is sent.
    let pd_type = change
        .pd_type
        .map(|n| named(n, PdType::from_name))
        .transpose()?;
    let emarker = change
        .emarker
        .map(|n| named(n, Emarker::from_name))
        .transpose()?;
    let sink = change
        .sink
        .map(|n| named(n, SinkCaps::from_name))
        .transpose()?;
    let result = (|| -> Result<PdSettings, CliError> {
        let current = pd::read_settings(&mut session)?;
        if pd_type.is_none() && emarker.is_none() && sink.is_none() {
            return Ok(current);
        }
        // A reserved e-marker code is never written, not even back: its
        // firmware effect is unknown. [static]
        if emarker.is_none() && current.emarker.name().starts_with("reserved") {
            return Err(CliError::Usage(format!(
                "the meter's e-marker emulation is {}; pass --em to choose a known value",
                current.emarker.name()
            )));
        }
        let want = PdSettings {
            pd_type: pd_type.unwrap_or(current.pd_type),
            emarker: emarker.unwrap_or(current.emarker),
            sink: sink.unwrap_or(current.sink),
        };
        Ok(pd::write_settings(&mut session, &want)?)
    })();
    let settings = close_after(session, result)?;
    write_settings(&settings, json, out)
}

/// The value named `name`, or a usage error.
fn named<T>(name: &str, from_name: fn(&str) -> Option<T>) -> Result<T, CliError> {
    from_name(name).ok_or_else(|| CliError::Usage(format!("unknown value {name:?}")))
}

/// The settings by name, as labelled lines or `{"type", "em", "sink"}`.
fn write_settings(s: &PdSettings, json: bool, out: &mut dyn Write) -> Result<(), CliError> {
    if json {
        return write_json(
            out,
            &serde_json::json!({
                "type": s.pd_type.name(),
                "em": s.emarker.name(),
                "sink": s.sink.name(),
            }),
        );
    }
    writeln!(out, "Type      {}", s.pd_type.name())?;
    writeln!(out, "E-marker  {}", s.emarker.name())?;
    writeln!(out, "Sink      {}", s.sink.name())?;
    Ok(())
}

/// `powerz trigger pd request` options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PdRequestOptions {
    /// Object position.
    pub position: u8,
    /// Voltage for a PPS / AVS offer, mV.
    pub voltage_mv: Option<u32>,
    /// Current, mA.
    pub current_ma: Option<u32>,
    /// `--allow-high-voltage` was given.
    pub allow_high_voltage: bool,
    /// How long to hold the voltage before returning to 5 V.
    pub hold: Duration,
    /// Print JSON.
    pub json: bool,
}

/// `powerz trigger pd request`: `entry pd`, validate and send the request
/// (above 5 V only with `--allow-high-voltage`), report the `req:` /
/// `ready:` replies, hold, then return to 5 V (`pd req=1`) and close the
/// session, on error too.
pub fn trigger_pd_request<P: Read + Write, C: Clock>(
    mut session: PdmSession<P, C>,
    opts: &PdRequestOptions,
    hold: &mut dyn FnMut(Duration),
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let request = request_of(opts)?;
    // A failed `entry pd` drops the session, which sends `pdm close`.
    let offers = pd::enter(&mut session)?.offers;
    let result = request_and_hold(&mut session, &offers, &request, opts, hold, out);
    let back = return_to_5v(&mut session, &offers);
    // A failed return to 5 V outranks the request's own error: it is the
    // one that says VBUS may still be high.
    let result = match back {
        Err(e) => Err(e),
        Ok(()) => result,
    };
    close_after(session, result)
}

/// Back to 5 V whatever happened: `pd req=1` (a 5 V request needs no
/// confirmation); if that fails, a Hard Reset, which also ends at 5 V.
/// When both fail, say loudly that VBUS may still be high.
fn return_to_5v<P: Read + Write, C: Clock>(
    session: &mut PdmSession<P, C>,
    offers: &[Offer],
) -> Result<(), CliError> {
    let to_5v = Request::Fixed {
        position: 1,
        current_ma: None,
    };
    let Err(first) = pd::request(session, offers, &to_5v, None) else {
        return Ok(());
    };
    match pd::hard_reset(session) {
        Ok(_) => {
            eprintln!("warning: return to 5 V failed ({first}); sent a Hard Reset instead");
            Ok(())
        }
        Err(second) => {
            eprintln!(
                "WARNING: could not return the charger to 5 V ({first}; Hard Reset: {second}). \
                 VBUS may still be above 5 V: unplug the charger from the meter."
            );
            Err(CliError::Device(format!(
                "could not return the charger to 5 V ({first}; Hard Reset: {second}); \
                 VBUS may still be above 5 V"
            )))
        }
    }
}

/// The library request for the command-line options.
fn request_of(opts: &PdRequestOptions) -> Result<Request, CliError> {
    match (opts.voltage_mv, opts.current_ma) {
        (Some(voltage_mv), Some(current_ma)) => Ok(Request::Programmable {
            position: opts.position,
            voltage_mv,
            current_ma,
        }),
        (Some(_), None) => Err(CliError::Usage(
            "--voltage-mv needs --current-ma as well".into(),
        )),
        (None, current_ma) => Ok(Request::Fixed {
            position: opts.position,
            current_ma,
        }),
    }
}

/// Validate, check the high-voltage flag, send, report and hold.
fn request_and_hold<P: Read + Write, C: Clock>(
    session: &mut PdmSession<P, C>,
    offers: &[Offer],
    request: &Request,
    opts: &PdRequestOptions,
    hold: &mut dyn FnMut(Duration),
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let command = pd::validate(offers, request)?;
    let confirmation = high_voltage_confirmation(session, &command, opts.allow_high_voltage)?;
    let outcome = pd::request(session, offers, request, confirmation)?;
    write_outcome(&outcome, opts.json, out)?;
    hold_for(opts.hold, hold);
    Ok(())
}

/// The confirmation `command` needs: `None` at or below 5 V; above 5 V
/// one only with `--allow-high-voltage`, otherwise a usage error before
/// anything is written.
fn high_voltage_confirmation<P: Read + Write, C: Clock>(
    session: &PdmSession<P, C>,
    command: &Command,
    allow_high_voltage: bool,
) -> Result<Option<HighVoltageConfirmation>, CliError> {
    if session.risk(command) != Risk::Above5V {
        return Ok(None);
    }
    if !allow_high_voltage {
        return Err(CliError::Usage(format!(
            "`{}` can raise VBUS above 5 V; pass --allow-high-voltage to send it",
            String::from_utf8_lossy(&command.render())
        )));
    }
    Ok(Some(HighVoltageConfirmation::for_command(command)))
}

/// Hold the requested voltage for `duration` (nothing for zero).
fn hold_for(duration: Duration, hold: &mut dyn FnMut(Duration)) {
    if !duration.is_zero() {
        hold(duration);
    }
}

/// The command and the `req:` / `ready:` readings, as lines or JSON.
fn write_outcome(o: &RequestOutcome, json: bool, out: &mut dyn Write) -> Result<(), CliError> {
    let command = String::from_utf8_lossy(&o.command.render()).into_owned();
    if json {
        return write_json(
            out,
            &serde_json::json!({
                "command": command,
                "requested_mv": o.requested.voltage_mv,
                "requested_ma": o.requested.current_ma,
                "delivered_mv": o.delivered.voltage_mv,
                "delivered_ma": o.delivered.current_ma,
            }),
        );
    }
    writeln!(out, "Command    {command}")?;
    writeln!(out, "Requested  {}", volts_amps(&o.requested))?;
    writeln!(out, "Delivered  {}", volts_amps(&o.delivered))?;
    Ok(())
}

/// `powerz trigger qc|qc3|fcp|afc|sfcp|scp|vfcp|tfcp|ufcs ...` options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegacyRequestOptions {
    /// The protocol to enter.
    pub protocol: LegacyProtocol,
    /// The request.
    pub request: LegacyRequest,
    /// `--allow-high-voltage` was given.
    pub allow_high_voltage: bool,
    /// How long to hold the voltage before returning to 5 V.
    pub hold: Duration,
    /// Print JSON.
    pub json: bool,
}

/// `powerz trigger qc|qc3|fcp|afc|sfcp|scp|vfcp|tfcp|ufcs ...`: `entry
/// <protocol>`, validate and send the request (above 5 V only with
/// `--allow-high-voltage`), report it, hold, then return to 5 V (`reset`)
/// and close the session, on error too.
pub fn trigger_legacy_request<P: Read + Write, C: Clock>(
    mut session: PdmSession<P, C>,
    opts: &LegacyRequestOptions,
    hold: &mut dyn FnMut(Duration),
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let result = (|| -> Result<(), CliError> {
        legacy::enter(&mut session, opts.protocol)?;
        let command = legacy::validate(session.capabilities(), &opts.request)?;
        let confirmation = high_voltage_confirmation(&session, &command, opts.allow_high_voltage)?;
        let outcome = legacy::request(&mut session, &opts.request, confirmation)?;
        write_legacy_outcome(&outcome, opts.json, out)?;
        // No `ready`: the charger may have ignored or refused it, so this is
        // not reported as a success. [inferred]
        if outcome.reply.is_none() {
            return Err(CliError::Device(format!(
                "the charger did not confirm `{}` (no ready reply)",
                String::from_utf8_lossy(&outcome.command.render())
            )));
        }
        hold_for(opts.hold, hold);
        Ok(())
    })();
    finish_at_5v(session, result)
}

/// `powerz trigger scp|vfcp|tfcp list`: `entry <protocol>`, then the
/// decoded limits of its capability block; back to 5 V (`reset`) and close
/// the session, on error too.
pub fn trigger_legacy_list<P: Read + Write, C: Clock>(
    mut session: PdmSession<P, C>,
    protocol: LegacyProtocol,
    json: bool,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let result = legacy::enter(&mut session, protocol).map_err(CliError::from);
    let entered = finish_at_5v(session, result)?;
    write_limits(protocol, &entered.limits, json, out)
}

/// Back to 5 V (`reset`) whatever `result` is, then close the session. A
/// failed return outranks `result`'s own error, since it is the one that
/// says VBUS may still be high, but that earlier error is kept in the text.
fn finish_at_5v<T, P: Read + Write, C: Clock>(
    mut session: PdmSession<P, C>,
    result: Result<T, CliError>,
) -> Result<T, CliError> {
    let result = match legacy::return_to_5v(&mut session) {
        Err(e) => {
            eprintln!(
                "WARNING: could not return the charger to 5 V ({e}). \
                 VBUS may still be above 5 V: unplug the charger from the meter."
            );
            let earlier = result
                .err()
                .map(|r| format!("; before that: {r}"))
                .unwrap_or_default();
            Err(CliError::Device(format!(
                "could not return the charger to 5 V ({e}); VBUS may still be above 5 V{earlier}"
            )))
        }
        Ok(()) => result,
    };
    close_after(session, result)
}

/// Limits as lines (V / A / W) or `{"protocol": .., "limits": [..]}`.
fn write_limits(
    protocol: LegacyProtocol,
    limits: &[legacy::Limits],
    json: bool,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    if json {
        let limits: Vec<_> = limits
            .iter()
            .map(|l| {
                serde_json::json!({
                    "index": l.index,
                    "min_voltage_mv": l.min_voltage_mv,
                    "max_voltage_mv": l.max_voltage_mv,
                    "min_current_ma": l.min_current_ma,
                    "max_current_ma": l.max_current_ma,
                    "max_power_mw": l.max_power_mw,
                })
            })
            .collect();
        return write_json(
            out,
            &serde_json::json!({ "protocol": protocol.name(), "limits": limits }),
        );
    }
    let milli = |v: u32| f64::from(v) / 1000.0;
    for l in limits {
        if let Some(i) = l.index {
            write!(out, "#{i}  ")?;
        }
        write!(
            out,
            "Voltage {:.2}-{:.2} V  Current {:.2}-{:.2} A",
            milli(l.min_voltage_mv),
            milli(l.max_voltage_mv),
            milli(l.min_current_ma),
            milli(l.max_current_ma)
        )?;
        if let Some(p) = l.max_power_mw {
            write!(out, "  Max power {} W", milli(p))?;
        }
        writeln!(out)?;
    }
    Ok(())
}

/// The command and the meter's reply, as lines or JSON.
fn write_legacy_outcome(
    o: &legacy::Outcome,
    json: bool,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let command = String::from_utf8_lossy(&o.command.render()).into_owned();
    if json {
        return write_json(
            out,
            &serde_json::json!({ "command": command, "reply": o.reply }),
        );
    }
    writeln!(out, "Command  {command}")?;
    writeln!(out, "Reply    {}", o.reply.as_deref().unwrap_or("(none)"))?;
    Ok(())
}

/// The offers as a table, or as `{"epr": bool, "offers": [...]}`.
fn write_offers(offers: &[Offer], json: bool, out: &mut dyn Write) -> Result<(), CliError> {
    if json {
        return write_json(out, &offers_json(offers));
    }
    let epr = pd::epr_entered(offers);
    writeln!(out, "EPR mode: {}", if epr { "yes" } else { "no" })?;
    writeln!(out, "Pos  Type      Voltage          Maximum")?;
    for o in offers {
        writeln!(
            out,
            "{:>3}  {:<8}  {:<15}  {}",
            o.position,
            kind_label(o.kind),
            voltage_text(o),
            limit_text(o)
        )?;
    }
    Ok(())
}

/// `{"epr": bool, "offers": [...]}`.
fn offers_json(offers: &[Offer]) -> serde_json::Value {
    let list: Vec<serde_json::Value> = offers
        .iter()
        .map(|o| {
            serde_json::json!({
                "position": o.position,
                "kind": o.kind.name(),
                "min_voltage_mv": o.min_voltage_mv,
                "max_voltage_mv": o.max_voltage_mv,
                "max_current_ma": o.max_current_ma,
                "max_power_mw": o.max_power_mw,
            })
        })
        .collect();
    serde_json::json!({ "epr": pd::epr_entered(offers), "offers": list })
}

/// Display name of an offer kind.
fn kind_label(kind: OfferKind) -> &'static str {
    match kind {
        OfferKind::Fixed => "Fixed",
        OfferKind::Battery => "Battery",
        OfferKind::Variable => "Variable",
        OfferKind::Pps => "PPS",
        OfferKind::SprAvs => "SPR AVS",
        OfferKind::EprAvs => "EPR AVS",
    }
}

/// "5.00 V" for a fixed offer, "15.00-28.00 V" for a range.
fn voltage_text(o: &Offer) -> String {
    if o.min_voltage_mv == o.max_voltage_mv {
        format!("{} V", two_decimals(o.max_voltage_mv))
    } else {
        format!(
            "{}-{} V",
            two_decimals(o.min_voltage_mv),
            two_decimals(o.max_voltage_mv)
        )
    }
}

/// "3.00 A" for a current limit, "140 W" for a power limit.
fn limit_text(o: &Offer) -> String {
    match (o.max_current_ma, o.max_power_mw) {
        (Some(ma), _) => format!("{} A", two_decimals(ma)),
        (None, Some(mw)) if mw % 1000 == 0 => format!("{} W", mw / 1000),
        (None, Some(mw)) => format!("{} W", two_decimals(mw)),
        (None, None) => String::new(),
    }
}

/// "5.16 V  0.00 A".
fn volts_amps(r: &Reading) -> String {
    format!(
        "{} V  {} A",
        two_decimals(r.voltage_mv),
        two_decimals(r.current_ma)
    )
}

/// Milli-units as "x.yy".
fn two_decimals(milli: u32) -> String {
    format!("{}.{:02}", milli / 1000, (milli % 1000) / 10)
}

#[cfg(test)]
mod tests {
    use super::*;
    use powerz_sim::{SimCharger, SimClock, SimPdm, SimPdmPort};

    /// A sim port whose meter stays silent on `pd req=1`.
    struct SilentOnReturn(SimPdmPort);

    impl Read for SilentOnReturn {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.0.read(buf)
        }
    }

    impl Write for SilentOnReturn {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if buf == b"pd req=1" {
                return Ok(buf.len());
            }
            self.0.write(buf)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            self.0.flush()
        }
    }

    // ---- AC-DGSQ6P.4: a reserved e-marker code is never written back ----

    #[test]
    fn reserved_current_emarker_is_not_written_back() {
        use powerz_protocol::pdm::Command;
        let clock = SimClock::new(1_790_000_000_000);
        let sim = SimPdm::new(SimCharger::Epr140W, clock.clone());
        let mut session = PdmSession::open_with_clock(sim.open_port(), clock).expect("open");
        // Someone else left the meter on reserved code 4.
        session
            .send(&Command::PdmSet {
                ty: 0,
                em: 4,
                sink: 0,
            })
            .unwrap();
        let change = PdSettingsChange {
            sink: Some("pps-5a"),
            ..PdSettingsChange::default()
        };
        let err = trigger_pd_settings(session, &change, false, &mut Vec::new()).unwrap_err();
        assert!(matches!(err, CliError::Usage(_)), "{err:?}");
        let sets = sim
            .writes()
            .iter()
            .filter(|w| w.starts_with("pdm set"))
            .count();
        assert_eq!(sets, 1, "only the test's own pdm set: {:?}", sim.writes());
        assert_eq!(sim.writes().last().map(String::as_str), Some("pdm close"));
    }

    /// A sim port whose meter never hears `qc 9V` (so it never says `ready`).
    struct DeafToQc9(SimPdmPort);

    impl Read for DeafToQc9 {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.0.read(buf)
        }
    }

    impl Write for DeafToQc9 {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if buf == b"qc 9V" {
                return Ok(buf.len());
            }
            self.0.write(buf)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            self.0.flush()
        }
    }

    // ---- AC-DGSQ6Q.6: a request the charger never confirms is not a success ----

    #[test]
    fn legacy_request_without_a_ready_reply_fails_after_returning_to_5v() {
        use powerz_protocol::pdm::FixedProtocol;
        let clock = SimClock::new(1_790_000_000_000);
        let sim = SimPdm::new(SimCharger::MultiProtocol, clock.clone());
        let session = PdmSession::open_with_clock(DeafToQc9(sim.open_port()), clock).expect("open");
        let opts = LegacyRequestOptions {
            protocol: LegacyProtocol::Qc,
            request: LegacyRequest::Fixed {
                protocol: FixedProtocol::Qc,
                volts: 9,
            },
            allow_high_voltage: true,
            hold: Duration::ZERO,
            json: false,
        };
        let err = trigger_legacy_request(session, &opts, &mut |_| {}, &mut Vec::new())
            .expect_err("no ready reply");
        assert!(matches!(err, CliError::Device(_)), "{err:?}");
        let writes = sim.writes();
        let tail: Vec<&str> = writes
            .iter()
            .rev()
            .take(2)
            .rev()
            .map(String::as_str)
            .collect();
        assert_eq!(tail, ["reset", "pdm close"], "{writes:?}");
    }

    // ---- AC-DGSQ6P.6: the meter always ends at 5 V ----

    #[test]
    fn failed_return_to_5v_falls_back_to_a_hard_reset() {
        let clock = SimClock::new(1_790_000_000_000);
        let sim = SimPdm::new(SimCharger::Epr140W, clock.clone());
        let session =
            PdmSession::open_with_clock(SilentOnReturn(sim.open_port()), clock).expect("open");
        let opts = PdRequestOptions {
            position: 4,
            voltage_mv: None,
            current_ma: None,
            allow_high_voltage: true,
            hold: Duration::ZERO,
            json: false,
        };
        let mut out = Vec::new();
        trigger_pd_request(session, &opts, &mut |_| {}, &mut out).expect("ends at 5 V");
        let writes = sim.writes();
        let tail: Vec<&str> = writes
            .iter()
            .rev()
            .take(2)
            .rev()
            .map(String::as_str)
            .collect();
        assert_eq!(tail, ["pd hrst", "pdm close"], "{writes:?}");
        assert!(sim.vbus_mv() < 5500, "VBUS {} mV", sim.vbus_mv());
    }
}
