//! `powerz detect` (FR DGSQ6R): charger protocol detection through the
//! meter's PDM text session. Every command it sends stays at 5 V.

use std::io::{Read, Write};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use powerz_protocol::pdm::PdmSession;
use powerz_protocol::pdm::detect::{
    self, Check, Hint, ListEntry, ListOptions, ListResult, Support,
};
use powerz_protocol::session::Clock;
use serde_json::{Value, json};

use super::close_after;
use crate::error::CliError;
use crate::output::write_json;

/// What `powerz detect` runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DetectOptions {
    /// Run the automatic list.
    pub list: bool,
    /// Use `entry list+` for the list.
    pub plus: bool,
    /// Silence that ends the list with `reset`.
    pub stall_limit: Duration,
    /// Print one JSON object instead of labelled lines.
    pub json: bool,
}

/// `powerz detect`: `entry bc`, `entry apple`, then (on request) the
/// automatic list, each list line printed as it arrives, and the D+ / D-
/// `hint`; the session is closed with `pdm close`, on error too.
pub fn detect<P: Read + Write, C: Clock>(
    mut session: PdmSession<P, C>,
    opts: &DetectOptions,
    hint: &Hint,
    stop: &AtomicBool,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let result = run(&mut session, opts, stop, out);
    let (bc12, apple, list) = close_after(session, result)?;
    let disagreements = list
        .as_ref()
        .map(|l| detect::disagreements(bc12, apple, l))
        .unwrap_or_default();
    if opts.json {
        let d: Vec<Value> = disagreements
            .iter()
            .map(|d| {
                json!({
                    "check": d.check.name(),
                    "check_says": d.check_says.name(),
                    "entry": d.entry,
                    "list_says": d.list_says.name(),
                })
            })
            .collect();
        let v = json!({
            "bc12": bc12.name(),
            "apple": apple.name(),
            "list": list.as_ref().map(list_json),
            "disagreements": d,
            "hint": hint_json(hint),
        });
        return write_json(out, &v);
    }
    if let Some(list) = list {
        if list.complete {
            writeln!(out, "list complete")?;
        } else {
            writeln!(out, "list incomplete (stalled or cut off; reset sent)")?;
        }
    }
    for d in &disagreements {
        writeln!(out, "disagree {}", d.label())?;
    }
    writeln!(out, "{}", hint.label())?;
    Ok(())
}

/// `{"dp_mv", "dm_mv", "modes": [name], "label"}`.
fn hint_json(hint: &Hint) -> Value {
    let modes: Vec<&str> = hint.modes.iter().map(|m| m.name()).collect();
    json!({
        "dp_mv": hint.dp_mv,
        "dm_mv": hint.dm_mv,
        "modes": modes,
        "label": hint.label(),
    })
}

type Detected = (Support, Support, Option<ListResult>);

/// The detection steps; text lines are printed as results arrive.
fn run<P: Read + Write, C: Clock>(
    session: &mut PdmSession<P, C>,
    opts: &DetectOptions,
    stop: &AtomicBool,
    out: &mut dyn Write,
) -> Result<Detected, CliError> {
    if opts.list {
        // No command of ours asks for more than 5 V here, but the meter's own
        // auto-detect probes protocols (`entry list+` even needs a dummy load
        // [doc]), and probing may briefly raise VBUS on some chargers. Said
        // before anything is sent. [inferred]
        eprintln!(
            "note: the automatic list lets the meter probe protocols, which can briefly \
             raise VBUS above 5 V; keep anything that cannot take it off the output"
        );
    }
    let bc12 = detect::bc12(session)?;
    if !opts.json {
        writeln!(out, "BC1.2    {} ({})", bc12.name(), Check::Bc12.name())?;
    }
    let apple = detect::apple(session)?;
    if !opts.json {
        writeln!(out, "Apple    {} ({})", apple.name(), Check::Apple.name())?;
    }
    if !opts.list {
        return Ok((bc12, apple, None));
    }
    let list_opts = ListOptions {
        plus: opts.plus,
        stall_limit: opts.stall_limit,
    };
    let mut write_err = None;
    let json = opts.json;
    let list = detect::list_until(
        session,
        &list_opts,
        &mut |e| {
            if !json
                && write_err.is_none()
                && let Err(err) = writeln!(out, "{}", entry_line(e))
            {
                write_err = Some(err);
            }
        },
        stop,
    )?;
    if let Some(err) = write_err {
        return Err(err.into());
    }
    Ok((bc12, apple, Some(list)))
}

/// One list entry as a text line, e.g. `bc1.2    supported 5203 mV 0 mA`.
fn entry_line(e: &ListEntry) -> String {
    let mut line = format!("{:<8} {}", e.name, e.support.name());
    if let Some(mv) = e.vbus_mv {
        line.push_str(&format!(" {mv} mV"));
    }
    if let Some(ma) = e.ibus_ma {
        line.push_str(&format!(" {ma} mA"));
    }
    line
}

/// `{"complete", "entries": [{"name", "support", "vbus_mv", "ibus_ma"}]}`.
fn list_json(list: &ListResult) -> Value {
    let entries: Vec<Value> = list
        .entries
        .iter()
        .map(|e| {
            json!({
                "name": e.name,
                "support": e.support.name(),
                "vbus_mv": e.vbus_mv,
                "ibus_ma": e.ibus_ma,
            })
        })
        .collect();
    json!({ "complete": list.complete, "entries": entries })
}
