//! Firmware version parsing and the outdated-firmware warnings
//! (FR V9SK52, AC .4).

use std::cmp::Ordering;
use std::fmt;

/// A dotted firmware version, `major.minor.patch`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Version {
    /// Major component.
    pub major: u32,
    /// Minor component.
    pub minor: u32,
    /// Patch component.
    pub patch: u32,
    /// The pre-release tag, without its `-`. A tagged version sorts below the
    /// release of its numbers; `None` is the release.
    pub tag: Option<String>,
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
            tag: None,
        }
    }

    /// Parse a dotted version string.
    ///
    /// `major`, `major.minor` or `major.minor.patch` (missing components
    /// are 0). Surrounding whitespace and one leading `v` or `V` are dropped
    /// (`v2.0.7` is 2.0.7). A suffix from the first `-` on makes a
    /// pre-release, as in semver: `2.0.7-beta` sorts below 2.0.7, so it never
    /// hides a warning the release would get, and it keeps its tag. A `+`
    /// suffix is build metadata and is ignored: `2.0.7+build5` is 2.0.7, and
    /// `2.0.7-beta+5` is 2.0.7-beta. Anything else is `None`, such as `2.x.7`,
    /// `2.0.7.1`, `vv2.0.7`, a suffix with no numbers before it, a number
    /// with a leading `+` such as `+2.0.7`, or the empty tag of `2.0.7-`.
    pub fn parse(s: &str) -> Option<Version> {
        let s = s.trim();
        let s = s.strip_prefix(['v', 'V']).unwrap_or(s);
        let s = s.split_once('+').map_or(s, |(before, _build)| before);
        let (numbers, tag) = match s.split_once('-') {
            Some((_, "")) => return None,
            Some((numbers, tag)) => (numbers, Some(tag.to_owned())),
            None => (s, None),
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
            tag,
            ..Version::new(major, minor, patch)
        })
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Version) -> Ordering {
        let numbers = |v: &Version| (v.major, v.minor, v.patch);
        numbers(self)
            .cmp(&numbers(other))
            .then_with(|| match (&self.tag, &other.tag) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(a), Some(b)) => a.split('.').map(tag_key).cmp(b.split('.').map(tag_key)),
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Version) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Sort key of one dot-separated part of a tag, in semver order: numbers by
/// value and below words (`rc.2` sorts below `rc.10`). The part itself comes
/// last, so that only equal versions compare equal (`rc.1` and `rc.01` differ).
fn tag_key(part: &str) -> (bool, usize, &str, &str) {
    if !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()) {
        let digits = part.trim_start_matches('0');
        (false, digits.len(), digits, part)
    } else {
        (true, 0, part, part)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        match &self.tag {
            Some(tag) => write!(f, "-{tag}"),
            None => Ok(()),
        }
    }
}

/// A warning about the meter's firmware.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FirmwareWarning {
    /// Older than [`MIN_SUPPORTED`].
    TooOld,
    /// Older than [`KNOWN_LATEST`].
    Outdated,
}

impl FirmwareWarning {
    /// Stable machine-readable code.
    pub fn code(self) -> &'static str {
        match self {
            FirmwareWarning::TooOld => "too_old",
            FirmwareWarning::Outdated => "outdated",
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
        }
    }
}

/// Warnings for application firmware `version`.
///
/// An unparseable version gives no warning.
pub fn warnings(version: &str) -> Vec<FirmwareWarning> {
    match Version::parse(version) {
        Some(v) if v < MIN_SUPPORTED => vec![FirmwareWarning::TooOld],
        Some(v) if v < KNOWN_LATEST => vec![FirmwareWarning::Outdated],
        _ => Vec::new(),
    }
}
