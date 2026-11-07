//! Built-in open-powerz startup logos (FR 70X5YX).
//!
//! The two brand meter logos ship embedded in the library, so a logo can be
//! written without a file. Names, labels and the PNG bytes need no feature;
//! `BuiltinLogo::load` and `BuiltinLogo::encode` decode the PNG and sit
//! behind `image-files` like every other image decode.
#![cfg_attr(
    feature = "image-files",
    doc = "\nSee [`BuiltinLogo::load`] and [`BuiltinLogo::encode`]."
)]

use super::LogoMode;

/// One of the embedded brand logos.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinLogo {
    /// The electron-surf mascot (`assets/brand/meter-logo.png`).
    Surf,
    /// The mascot with the lettering (`assets/brand/meter-logo-wordmark.png`).
    SurfWordmark,
}

impl BuiltinLogo {
    /// Every built-in logo, in listing order.
    pub const ALL: [BuiltinLogo; 2] = [BuiltinLogo::Surf, BuiltinLogo::SurfWordmark];

    /// The stable name used on the CLI and over IPC.
    pub fn name(self) -> &'static str {
        match self {
            BuiltinLogo::Surf => "surf",
            BuiltinLogo::SurfWordmark => "surf-wordmark",
        }
    }

    /// A short human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            BuiltinLogo::Surf => "open-powerz surfer",
            BuiltinLogo::SurfWordmark => "open-powerz surfer with wordmark",
        }
    }

    /// The embedded PNG bytes.
    pub fn png(self) -> &'static [u8] {
        match self {
            BuiltinLogo::Surf => include_bytes!("../../../../assets/brand/meter-logo.png"),
            BuiltinLogo::SurfWordmark => {
                include_bytes!("../../../../assets/brand/meter-logo-wordmark.png")
            }
        }
    }

    /// The pixel mode the logo is written in: always transparent.
    pub fn mode(self) -> LogoMode {
        LogoMode::Alpha
    }

    /// Decodes the embedded PNG into RGBA8888.
    #[cfg(feature = "image-files")]
    pub fn load(self) -> Result<super::Rgba, super::LogoError> {
        super::load_image_bytes(self.png())
    }

    /// Encodes the logo into a blob in [`BuiltinLogo::mode`].
    #[cfg(feature = "image-files")]
    pub fn encode(self) -> Result<super::LogoBlob, super::LogoError> {
        super::encode(&self.load()?, self.mode())
    }
}

impl std::fmt::Display for BuiltinLogo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// A name that is not one of the built-in logos.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown built-in logo {0:?}; expected one of: surf, surf-wordmark")]
pub struct UnknownBuiltin(pub String);

impl std::str::FromStr for BuiltinLogo {
    type Err = UnknownBuiltin;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        BuiltinLogo::ALL
            .into_iter()
            .find(|b| b.name() == s)
            .ok_or_else(|| UnknownBuiltin(s.to_owned()))
    }
}
