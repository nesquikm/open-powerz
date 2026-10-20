//! `powerz list` and choosing the meter for the other subcommands.

use std::io::Write;

use powerz_protocol::transport::discovery::{DeviceEntry, Model};

use crate::error::CliError;
use crate::output::write_json;

/// Pick the device to use: the one whose serial is `serial`, or the only
/// attached one. Several attached without `serial` is a usage error naming
/// the serials; no match is [`CliError::NoDevice`].
pub fn select_device<'a>(
    devices: &'a [DeviceEntry],
    serial: Option<&str>,
) -> Result<&'a DeviceEntry, CliError> {
    if let Some(serial) = serial {
        return devices
            .iter()
            .find(|d| d.serial.as_deref() == Some(serial))
            .ok_or_else(|| CliError::NoDevice(format!("no POWER-Z device with serial {serial}")));
    }
    match devices {
        [] => Err(CliError::NoDevice("no POWER-Z devices found".to_string())),
        [only] => Ok(only),
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
    if devices.is_empty() {
        writeln!(err, "no POWER-Z devices found")?;
    }
    if json {
        let arr: Vec<serde_json::Value> = devices
            .iter()
            .map(|d| {
                serde_json::json!({
                    "model": model_name(d.model),
                    "serial": d.serial,
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
            d.serial.as_deref().unwrap_or("-"),
            d.bus,
            d.address
        )?;
    }
    Ok(())
}

/// Display name of a model (`KM003C`, `KM002C`, or `unknown (PID 0x....)`).
fn model_name(model: Model) -> String {
    match model {
        Model::Km003c => "KM003C".to_string(),
        Model::Km002c => "KM002C".to_string(),
        Model::Unknown(pid) => format!("unknown (PID 0x{pid:04x})"),
    }
}
