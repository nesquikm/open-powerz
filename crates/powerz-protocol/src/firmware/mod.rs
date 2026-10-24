//! Firmware facts that need only read access: the device mode (application
//! or bootloader) and the firmware version checks. See `docs/protocol.md`.

pub mod container;
pub mod version;

/// What the meter is running, from the interface string of its vendor
/// interface (string index 2, `APP` in application mode `[device]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceMode {
    /// Normal application firmware (interface string `APP`).
    Application,
    /// The bootloader / DFU loader.
    Bootloader,
    /// No interface string, or one we do not recognise.
    Unknown,
}

impl DeviceMode {
    /// Classify the USB interface string of the vendor interface.
    ///
    /// `APP` (surrounding whitespace ignored) is the application firmware
    /// `[device]`; a string naming `boot` or `dfu` (any case) is the
    /// bootloader `[inferred]`; anything else, or no string, is unknown.
    pub fn from_interface_string(interface: Option<&str>) -> DeviceMode {
        let Some(s) = interface.map(str::trim) else {
            return DeviceMode::Unknown;
        };
        if s == "APP" {
            return DeviceMode::Application;
        }
        let lower = s.to_ascii_lowercase();
        if lower.contains("boot") || lower.contains("dfu") {
            DeviceMode::Bootloader
        } else {
            DeviceMode::Unknown
        }
    }

    /// Lowercase name: `application`, `bootloader` or `unknown`.
    pub fn name(self) -> &'static str {
        match self {
            DeviceMode::Application => "application",
            DeviceMode::Bootloader => "bootloader",
            DeviceMode::Unknown => "unknown",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_boot_and_unknown() {
        assert_eq!(
            DeviceMode::from_interface_string(Some("APP")),
            DeviceMode::Application
        );
        assert_eq!(
            DeviceMode::from_interface_string(Some("DFU")),
            DeviceMode::Bootloader
        );
        assert_eq!(DeviceMode::from_interface_string(None), DeviceMode::Unknown);
    }
}
