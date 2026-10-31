//! Firmware version parsing and the outdated-firmware warnings
//! (FR V9SK52, AC .4).

use std::fmt;

/// A dotted firmware version, `major.minor.patch`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    /// Major component.
    pub major: u32,
    /// Minor component.
    pub minor: u32,
    /// Patch component.
    pub patch: u32,
    /// False for a pre-release, which sorts below the release of its numbers.
    pub released: bool,
}

/// Oldest application firmware the app supports.
pub const MIN_SUPPORTED: Version = Version::new(1, 7, 0);
/// Newest application firmware the app knows of.
pub const KNOWN_LATEST: Version = Version::new(2, 0, 7);

impl Version {
    /// Build a version from its components.
    pub const fn new(major: u32, minor: u32, patch: u32) -> Version {
        Version {
            major,
            minor,
            patch,
            released: true,
        }
    }

    /// Parse a dotted version string.
    ///
    /// `major`, `major.minor` or `major.minor.patch` (missing components
    /// are 0). Surrounding whitespace and one leading `v` or `V` are dropped
    /// (`v2.0.7` is 2.0.7). A suffix from the first `-` on makes a
    /// pre-release, as in semver: `2.0.7-beta` sorts below 2.0.7, so it never
    /// hides a warning the release would get. A `+` suffix is build metadata
    /// and is ignored: `2.0.7+build5` is 2.0.7. Anything else is `None`, such
    /// as `2.x.7`, `2.0.7.1`, `vv2.0.7`, or a suffix with no numbers before it.
    pub fn parse(s: &str) -> Option<Version> {
        let s = s.trim();
        let s = s.strip_prefix(['v', 'V']).unwrap_or(s);
        let (numbers, suffix) = match s.find(['-', '+']) {
            Some(at) => s.split_at(at),
            None => (s, ""),
        };
        let mut parts = numbers.split('.').map(|p| p.parse::<u32>().ok());
        let major = parts.next()??;
        let minor = match parts.next() {
            Some(p) => p?,
            None => 0,
        };
        let patch = match parts.next() {
            Some(p) => p?,
            None => 0,
        };
        if parts.next().is_some() {
            return None;
        }
        Some(Version {
            released: !suffix.starts_with('-'),
            ..Version::new(major, minor, patch)
        })
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// A warning about the meter's firmware.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FirmwareWarning {
    /// Older than [`MIN_SUPPORTED`].
    TooOld,
    /// Older than [`KNOWN_LATEST`].
    Outdated,
    /// The HEAD reply says an update is required.
    UpdateRequired,
}

impl FirmwareWarning {
    /// Stable machine-readable code.
    pub fn code(self) -> &'static str {
        match self {
            FirmwareWarning::TooOld => "too_old",
            FirmwareWarning::Outdated => "outdated",
            FirmwareWarning::UpdateRequired => "update_required",
        }
    }
}

impl fmt::Display for FirmwareWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FirmwareWarning::TooOld => write!(
                f,
                "firmware is older than {MIN_SUPPORTED}, the oldest supported version; update the meter"
            ),
            FirmwareWarning::Outdated => write!(
                f,
                "firmware is older than {KNOWN_LATEST}, the newest version known"
            ),
            FirmwareWarning::UpdateRequired => {
                f.write_str("the meter reports update required (old firmware)")
            }
        }
    }
}

/// Warnings for application firmware `version`, plus the HEAD flag.
///
/// An unparseable version gives no version warning.
pub fn warnings(version: &str, update_required: bool) -> Vec<FirmwareWarning> {
    let mut w = Vec::new();
    match Version::parse(version) {
        Some(v) if v < MIN_SUPPORTED => w.push(FirmwareWarning::TooOld),
        Some(v) if v < KNOWN_LATEST => w.push(FirmwareWarning::Outdated),
        _ => {}
    }
    if update_required {
        w.push(FirmwareWarning::UpdateRequired);
    }
    w
}
