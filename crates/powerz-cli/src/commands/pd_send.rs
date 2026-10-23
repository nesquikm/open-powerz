//! `powerz pd send ...` (FR DGSQ6T): a custom PD message through the
//! meter's PDM text session (`pd cmd=` / `pd data=`).

use std::io::{Read, Write};

use powerz_protocol::pdm::PdmSession;
use powerz_protocol::pdm::message::{
    self, CableIdentity, PdMessage, Preset, SopTarget, await_cable_identity,
};
use powerz_protocol::pdm::pd;
use powerz_protocol::session::Clock;

use super::close_after;
use super::trigger::{high_voltage_confirmation, return_to_5v};
use crate::error::CliError;
use crate::output::write_json;

/// What `pd send` sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdSendOptions<'a> {
    /// The message as hex (header LE, data objects LE; no SOP byte).
    pub hex: Option<&'a str>,
    /// A built-in message instead of `hex`.
    pub preset: Option<Preset>,
    /// Where `hex` goes.
    pub sop: SopTarget,
    /// `--allow-high-voltage` was given.
    pub allow_high_voltage: bool,
    /// Print JSON.
    pub json: bool,
}

/// The message `opts` describes; a malformed one is a usage error.
pub fn message_of(opts: &PdSendOptions<'_>) -> Result<PdMessage, CliError> {
    match (opts.preset, opts.hex) {
        // A preset names its own target; a different --sop would be ignored.
        (Some(preset), None) if opts.sop != SopTarget::Sop => Err(CliError::Usage(format!(
            "--sop does not apply to --preset {}: the preset sets its own target",
            preset.name()
        ))),
        (Some(preset), None) => Ok(preset.message()),
        (None, Some(hex)) => {
            PdMessage::from_hex(opts.sop, hex).map_err(|e| CliError::Usage(e.to_string()))
        }
        _ => Err(CliError::Usage("give either HEX or --preset".into())),
    }
}

/// `powerz pd send`: build the message (refused before anything is sent
/// when malformed), `entry pd`, send it (above 5 V only with
/// `--allow-high-voltage`), return to 5 V when it could have raised VBUS,
/// and close the session, on error too.
pub fn pd_send<P: Read + Write, C: Clock>(
    mut session: PdmSession<P, C>,
    opts: &PdSendOptions<'_>,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let built =
        message_of(opts).and_then(|m| m.to_command().map_err(|e| CliError::Usage(e.to_string())));
    let command = match built {
        Ok(c) => c,
        Err(e) => return close_after(session, Err(e)),
    };
    // A failed `entry pd` drops the session, which sends `pdm close`.
    let offers = pd::enter(&mut session)?.offers;
    let confirmation = match high_voltage_confirmation(&session, &command, opts.allow_high_voltage)
    {
        Ok(c) => c,
        Err(e) => return close_after(session, Err(e)),
    };
    let raises = confirmation.is_some();
    let sent = match confirmation {
        Some(c) => session.send_confirmed(&command, c),
        None => session.send(&command),
    }
    .map_err(CliError::from);
    // The cable's answer is read before anything else is sent, so the
    // return to 5 V cannot swallow it.
    let sent = sent.and_then(|()| match opts.preset {
        Some(Preset::DiscoverIdentityCable) => {
            await_cable_identity(&mut session, message::CABLE_WAIT)
                .map_err(CliError::from)?
                .map(Some)
                .ok_or_else(|| {
                    CliError::Device(
                        "no answer from the cable (SOP'): it needs an e-marked cable, VCONN and \
                     meter firmware newer than 2.0.4"
                            .into(),
                    )
                })
        }
        None => Ok(None),
    });
    let result = if raises {
        // Whatever happened, VBUS goes back to 5 V before the session closes;
        // a failed return outranks the send's own error.
        return_to_5v(&mut session, &offers).and(sent)
    } else {
        sent
    };
    let cable = close_after(session, result)?;
    let text = String::from_utf8_lossy(&command.render()).into_owned();
    if opts.json {
        let mut v = serde_json::json!({ "command": text });
        if let Some(c) = &cable {
            v["cable"] = cable_json(c);
        }
        return write_json(out, &v);
    }
    writeln!(out, "sent     {text}")?;
    if let Some(c) = &cable {
        write_cable(out, c)?;
    }
    Ok(())
}

/// The cable's e-marker details as JSON.
fn cable_json(c: &CableIdentity) -> serde_json::Value {
    serde_json::json!({
        "sop": match c.sop {
            SopTarget::Sop => "sop",
            SopTarget::SopPrime => "sop_prime",
            SopTarget::SopDoublePrime => "sop_double_prime",
        },
        "vid": c.vid,
        "pid": c.pid,
        "active": c.active,
        "max_voltage_v": c.max_voltage_v,
        "max_current_ma": c.max_current_ma,
        "epr_capable": c.epr_capable,
    })
}

/// The cable's e-marker details as text.
fn write_cable(out: &mut dyn Write, c: &CableIdentity) -> Result<(), CliError> {
    writeln!(
        out,
        "cable    {} e-marker",
        if c.active { "active" } else { "passive" }
    )?;
    writeln!(out, "  VID    {:#06X}", c.vid)?;
    if let Some(pid) = c.pid {
        writeln!(out, "  PID    {pid:#06X}")?;
    }
    if let Some(v) = c.max_voltage_v {
        writeln!(out, "  max    {v} V")?;
    }
    if let Some(ma) = c.max_current_ma {
        writeln!(out, "  current {} A", f64::from(ma) / 1000.0)?;
    }
    writeln!(
        out,
        "  EPR    {}",
        if c.epr_capable {
            "capable"
        } else {
            "not capable"
        }
    )?;
    Ok(())
}
