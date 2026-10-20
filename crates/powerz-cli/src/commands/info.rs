//! `powerz info`.

use std::io::Write;

use powerz_protocol::session::{Clock, Session};
use powerz_protocol::transport::Transport;

use crate::error::CliError;
use crate::output::write_json;

/// `powerz info`: model, firmware version, hardware version, serial and UID;
/// JSON keys `model`, `firmware_version`, `hardware_version`, `serial`, `uid`.
pub fn info<T: Transport, C: Clock>(
    session: &mut Session<T, C>,
    json: bool,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let d = session.device_info()?;
    if json {
        return write_json(
            out,
            &serde_json::json!({
                "model": d.model,
                "firmware_version": d.firmware_version,
                "hardware_version": d.hardware_version,
                "serial": d.serial,
                "uid": d.uid,
            }),
        );
    }
    writeln!(out, "Model:     {}", d.model)?;
    writeln!(out, "Firmware:  {}", d.firmware_version)?;
    writeln!(out, "Hardware:  {}", d.hardware_version)?;
    writeln!(out, "Serial:    {}", d.serial)?;
    writeln!(out, "UID:       {}", d.uid)?;
    Ok(())
}
