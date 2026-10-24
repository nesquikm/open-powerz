//! `powerz firmware info FILE` (FR V9SK53).

use std::io::Write;

use powerz_protocol::firmware::container::FirmwareContainer;

use crate::error::CliError;
use crate::output::write_json;

/// `powerz firmware info`: the metadata of the firmware container in
/// `bytes` (read from the file `name`) and its English release notes.
///
/// JSON keys `vendor`, `container_version`, `model`, `version`, `date`,
/// `crc` (8 uppercase hex digits), `size` (declared image bytes) and
/// `release_notes_en`. A refused container is a usage error naming the file
/// and the reason; nothing is written to `out` then. The payload is never
/// decrypted.
pub fn firmware_info(
    bytes: &[u8],
    name: &str,
    json: bool,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let c = FirmwareContainer::parse(bytes).map_err(|e| CliError::Usage(format!("{name}: {e}")))?;
    let crc = c.crc_hex();
    if json {
        return write_json(
            out,
            &serde_json::json!({
                "vendor": c.vendor(),
                "container_version": c.container_version(),
                "model": c.model(),
                "version": c.firmware_version(),
                "date": c.date(),
                "crc": crc,
                "size": c.size(),
                "release_notes_en": c.release_notes_en(),
            }),
        );
    }
    writeln!(out, "Model:      {}", c.model())?;
    writeln!(out, "Version:    {}", c.firmware_version())?;
    writeln!(out, "Date:       {}", c.date())?;
    writeln!(out, "CRC:        {crc}")?;
    writeln!(out, "Size:       {} bytes", c.size())?;
    writeln!(out, "Container:  {}", c.container_version())?;
    writeln!(out, "Vendor:     {}", c.vendor())?;
    writeln!(out, "Release notes:")?;
    for line in c.release_notes_en().lines() {
        writeln!(out, "  {line}")?;
    }
    Ok(())
}
