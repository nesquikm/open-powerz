//! Firmware facts that need only read access: the device mode (application
//! or bootloader) and the firmware version checks. See `docs/protocol.md`.

pub mod container;
#[cfg(feature = "bulk")]
pub mod flash;
pub mod version;

/// What the meter is running, from the interface string of its vendor
/// interface (string index 2, `APP` in application mode `[device]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceMode {
    /// Normal application firmware (interface string `APP`).
    Application,
    /// The bootloader / DFU loader.
    Bootloader,
    /// The old (2.1) bootloader: a vendor interface named after the product,
    /// with no HID interface beside it `[device]` (field report).
    LegacyBootloader,
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

    /// Classify a device from its vendor-interface string and whether it has
    /// a HID (0x03) interface: what [`DeviceMode::from_interface_string`]
    /// calls unknown, a string with no HID interface beside it, is the old
    /// bootloader; a missing string stays unknown. The 2.1 shape is
    /// `[device]` (one field report); treating every such string as that
    /// bootloader is `[inferred]`: no other POWER-Z shape without HID is known.
    pub fn classify(interface: Option<&str>, has_hid: bool) -> DeviceMode {
        match DeviceMode::from_interface_string(interface) {
            DeviceMode::Unknown if interface.is_some() && !has_hid => DeviceMode::LegacyBootloader,
            mode => mode,
        }
    }

    /// Lowercase name: `application`, `bootloader`, `legacy_bootloader` or
    /// `unknown`.
    pub fn name(self) -> &'static str {
        match self {
            DeviceMode::Application => "application",
            DeviceMode::Bootloader => "bootloader",
            DeviceMode::LegacyBootloader => "legacy_bootloader",
            DeviceMode::Unknown => "unknown",
        }
    }

    /// Human-readable name for text output: [`DeviceMode::name`], except the
    /// old bootloader reads `bootloader (old, 2.1-style; experimental CLI-only
    /// flash with --experimental-old-bootloader)`.
    pub fn label(self) -> &'static str {
        match self {
            DeviceMode::LegacyBootloader => {
                "bootloader (old, 2.1-style; experimental CLI-only flash with --experimental-old-bootloader)"
            }
            mode => mode.name(),
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

    #[test]
    fn classify_names_the_old_bootloader_only_without_hid() {
        let legacy = Some("POWER-Z KM003C");
        assert_eq!(
            DeviceMode::classify(legacy, false),
            DeviceMode::LegacyBootloader
        );
        assert_eq!(DeviceMode::classify(legacy, true), DeviceMode::Unknown);
        assert_eq!(DeviceMode::classify(None, false), DeviceMode::Unknown);
        assert_eq!(
            DeviceMode::classify(Some("BOOT"), false),
            DeviceMode::Bootloader
        );
        assert_eq!(
            DeviceMode::classify(Some("APP"), true),
            DeviceMode::Application
        );
    }

    #[test]
    fn label_differs_from_name_only_for_the_old_bootloader() {
        assert_eq!(DeviceMode::LegacyBootloader.name(), "legacy_bootloader");
        assert_eq!(
            DeviceMode::LegacyBootloader.label(),
            "bootloader (old, 2.1-style; experimental CLI-only flash with --experimental-old-bootloader)"
        );
        for mode in [
            DeviceMode::Application,
            DeviceMode::Bootloader,
            DeviceMode::Unknown,
        ] {
            assert_eq!(mode.label(), mode.name());
        }
    }
}
