//! `powerz info`.

use std::io::Write;

use powerz_protocol::decode::DeviceInfoBlock;
use powerz_protocol::firmware::DeviceMode;
use powerz_protocol::session::{Clock, FirmwareProblemKind, Session};
use powerz_protocol::transport::Transport;

use crate::error::CliError;
use crate::output::write_json;

/// `powerz info`: model, mode, application and bootloader versions with
/// their dates, serial and UID.
///
/// JSON keys `model`, `mode`, `firmware_version`, `hardware_version`,
/// `application` / `bootloader` (`{model, version, date}` or `null`),
/// `update_required` (`null` when HEAD was not read), `warnings` (codes),
/// `firmware_problems` (what could not be read, one line each), `serial`,
/// `uid`. Text output adds `Update check: unavailable (<reason>)` when
/// HEAD was not read and one `Firmware status: <problem>` line per other
/// status part (info block, recovery) that could not be read.
///
/// Reports application mode: an open session is only answered by the
/// application firmware. [`info_in_mode`] reports the opened meter's mode.
pub fn info<T: Transport, C: Clock>(
    session: &mut Session<T, C>,
    json: bool,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    info_in_mode(session, DeviceMode::Application, json, out)
}

/// [`info`] reporting `mode`, the opened meter's mode from its vendor
/// interface's USB string.
///
/// Outside application mode only the mode is reported and nothing is sent.
/// In application mode the session is already connected: the device info,
/// then both info blocks and HEAD are read. A HEAD or info block that timed
/// out or did not decode costs only that part, and is shown as a problem.
pub fn info_in_mode<T: Transport, C: Clock>(
    session: &mut Session<T, C>,
    mode: DeviceMode,
    json: bool,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    if mode != DeviceMode::Application {
        if json {
            return write_json(
                out,
                &serde_json::json!({
                    "model": null,
                    "mode": mode.name(),
                    "firmware_version": null,
                    "hardware_version": null,
                    "application": null,
                    "bootloader": null,
                    "update_required": null,
                    "warnings": [],
                    "firmware_problems": [],
                    "serial": null,
                    "uid": null,
                }),
            );
        }
        writeln!(out, "Mode:       {}", mode.name())?;
        return Ok(());
    }
    let d = session.device_info()?;
    let fw = session.read_firmware_status_or_unknown()?;
    if json {
        let block = |b: &Option<DeviceInfoBlock>| {
            b.as_ref().map(|b| {
                serde_json::json!({
                    "model": b.model(),
                    "version": b.version(),
                    "date": b.date(),
                })
            })
        };
        let warnings: Vec<&str> = fw.warnings().into_iter().map(|w| w.code()).collect();
        return write_json(
            out,
            &serde_json::json!({
                "model": d.model,
                "mode": mode.name(),
                "firmware_version": d.firmware_version,
                "hardware_version": d.hardware_version,
                "application": block(&fw.application),
                "bootloader": block(&fw.bootloader),
                "update_required": fw.update_required,
                "warnings": warnings,
                "firmware_problems": fw.problems.iter().map(|p| p.text.as_str()).collect::<Vec<_>>(),
                "serial": d.serial,
                "uid": d.uid,
            }),
        );
    }
    // Without an info block (status unread) fall back to the device info.
    let with_date = |b: &Option<DeviceInfoBlock>, fallback: &str| match b {
        Some(b) if !b.date().is_empty() => format!("{} ({})", b.version(), b.date()),
        Some(b) => b.version().to_owned(),
        None => fallback.to_owned(),
    };
    writeln!(out, "Model:      {}", d.model)?;
    writeln!(out, "Mode:       {}", mode.name())?;
    writeln!(
        out,
        "Firmware:   {}",
        with_date(&fw.application, &d.firmware_version)
    )?;
    writeln!(
        out,
        "Bootloader: {}",
        with_date(&fw.bootloader, &d.hardware_version)
    )?;
    writeln!(out, "Hardware:   {}", d.hardware_version)?;
    writeln!(out, "Serial:     {}", d.serial)?;
    writeln!(out, "UID:        {}", d.uid)?;
    if fw.update_required.is_none() {
        let reason = fw
            .problems
            .iter()
            .find(|p| p.kind == FirmwareProblemKind::Head)
            .map_or("HEAD not read", |p| p.text.as_str());
        writeln!(out, "Update check: unavailable ({reason})")?;
    }
    for p in fw
        .problems
        .iter()
        .filter(|p| p.kind != FirmwareProblemKind::Head)
    {
        writeln!(out, "Firmware status: {p}")?;
    }
    for w in fw.warnings() {
        writeln!(out, "Warning: {w}")?;
    }
    Ok(())
}
