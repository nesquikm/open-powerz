//! `powerz list` and choosing the meter for the other subcommands.

use std::io::Write;

use powerz_protocol::transport::discovery::{DeviceEntry, Model};

use crate::error::CliError;
use crate::output::{MASK, write_json};

/// Pick the device to use: the one whose serial is `serial`, or the only
/// attached one. Several attached without `serial` is a usage error naming
/// the serials; no match is [`CliError::NoDevice`].
pub fn select_device<'a>(
    devices: &'a [DeviceEntry],
    serial: Option<&str>,
) -> Result<&'a DeviceEntry, CliError> {
    select_device_with(devices, serial, false)
}

/// [`select_device`] for a command that may run with `--hide-serial`: its
/// errors then count meters and repeat no serial.
pub fn select_device_with<'a>(
    devices: &'a [DeviceEntry],
    serial: Option<&str>,
    hide_serial: bool,
) -> Result<&'a DeviceEntry, CliError> {
    if let Some(serial) = serial {
        return devices
            .iter()
            .find(|d| d.serial.as_deref() == Some(serial))
            .ok_or_else(|| {
                CliError::NoDevice(if hide_serial {
                    "no POWER-Z device with that serial".to_owned()
                } else {
                    format!("no POWER-Z device with serial {serial}")
                })
            });
    }
    match devices {
        [] => Err(CliError::NoDevice("no POWER-Z devices found".to_string())),
        [only] => Ok(only),
        several if hide_serial => Err(CliError::Usage(format!(
            "{} POWER-Z devices attached; choose one with --device SERIAL",
            several.len()
        ))),
        several => {
            let serials: Vec<&str> = several
                .iter()
                .map(|d| d.serial.as_deref().unwrap_or("-"))
                .collect();
            Err(CliError::Usage(format!(
                "several POWER-Z devices attached; choose one with --device SERIAL: {}",
                serials.join(", ")
            )))
        }
    }
}

/// `powerz list`: one line per meter (model, serial, bus/address), or a JSON
/// array of `{model, serial, bus, address}`. With no meter, prints
/// `no POWER-Z devices found` to `err`.
pub fn list(
    devices: &[DeviceEntry],
    json: bool,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), CliError> {
    list_with(devices, json, false, out, err)
}

/// [`list`], printing `***` for every serial when `hide_serial` is set.
pub fn list_with(
    devices: &[DeviceEntry],
    json: bool,
    hide_serial: bool,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), CliError> {
    if devices.is_empty() {
        writeln!(err, "no POWER-Z devices found")?;
    }
    if json {
        let arr: Vec<serde_json::Value> = devices
            .iter()
            .map(|d| {
                serde_json::json!({
                    "model": model_name(d.model),
                    "serial": shown_serial(d, hide_serial),
                    "bus": d.bus,
                    "address": d.address,
                })
            })
            .collect();
        return write_json(out, &serde_json::Value::Array(arr));
    }
    for d in devices {
        writeln!(
            out,
            "{}  serial {}  bus {} address {}",
            model_name(d.model),
            shown_serial(d, hide_serial).unwrap_or("-"),
            d.bus,
            d.address
        )?;
    }
    Ok(())
}

/// The serial `list` prints for `d`.
fn shown_serial(d: &DeviceEntry, hide_serial: bool) -> Option<&str> {
    if hide_serial {
        Some(MASK)
    } else {
        d.serial.as_deref()
    }
}

/// Display name of a model (`KM003C`, `KM002C`, or `unknown (PID 0x....)`).
fn model_name(model: Model) -> String {
    match model {
        Model::Km003c => "KM003C".to_string(),
        Model::Km002c => "KM002C".to_string(),
        Model::Unknown(pid) => format!("unknown (PID 0x{pid:04x})"),
    }
}
