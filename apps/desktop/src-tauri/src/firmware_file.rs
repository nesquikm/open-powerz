//! Inspecting a local firmware container for the UI (FR V9SK53, AC .3).
//!
//! Reads the file, parses its header with
//! [`FirmwareContainer`](powerz_protocol::firmware::container::FirmwareContainer)
//! and returns the metadata with the English release notes. No meter is
//! involved and the image is never decrypted.

use std::path::Path;

use powerz_protocol::firmware::container::{FirmwareContainer, read_container_file};
use serde::Serialize;

use crate::ui_error::UiError;

/// A firmware container's metadata, as the UI shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FirmwareFileInfo {
    /// Vendor string.
    pub vendor: String,
    /// Container format version.
    pub container_version: String,
    /// Model the firmware is for.
    pub model: String,
    /// Firmware version.
    pub version: String,
    /// Release date as written in the file.
    pub date: String,
    /// CRC-32 of the image, 8 uppercase hex digits.
    pub crc: String,
    /// Declared image size, bytes.
    pub size: u32,
    /// English release notes.
    pub release_notes_en: String,
}

/// Read and parse the firmware container at `path`. A file that cannot be
/// read, or is not a valid container, is a usage error with the reason.
pub fn inspect_firmware_file(path: &Path) -> Result<FirmwareFileInfo, UiError> {
    let bytes = read_container_file(path).map_err(|e| UiError::usage(e.to_string()))?;
    let c = FirmwareContainer::parse(&bytes)
        .map_err(|e| UiError::usage(format!("{}: {e}", path.display())))?;
    Ok(FirmwareFileInfo {
        vendor: c.vendor().to_owned(),
        container_version: c.container_version().to_owned(),
        model: c.model().to_owned(),
        version: c.firmware_version().to_owned(),
        date: c.date().to_owned(),
        crc: c.crc_hex(),
        size: c.size(),
        release_notes_en: c.release_notes_en().to_owned(),
    })
}
