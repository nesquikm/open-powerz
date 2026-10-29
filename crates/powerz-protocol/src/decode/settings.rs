//! SETTINGS block (`ATT_SETTINGS`, 180 bytes, two CRC-32 blocks).
//! See `docs/protocol.md`.

use std::ops::RangeInclusive;

use serde::Serialize;

use super::{DecodeError, Rate, c_str, need, u32_at};

/// Length of the SETTINGS section payload.
const LEN: usize = 180;

/// The 180-byte SETTINGS payload with its CRC verdicts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    raw: [u8; LEN],
    block_a_crc_ok: bool,
    block_b_crc_ok: bool,
}

/// CRC-32 of `b[data]` equals the little-endian u32 stored at `crc_off`.
fn crc_ok(b: &[u8], data: core::ops::Range<usize>, crc_off: usize) -> bool {
    crc32fast::hash(&b[data]) == u32_at(b, crc_off)
}

/// RAM overlay inside block A (device session 1, FW 2.0.6) [inferred].
const OVERLAY: std::ops::Range<usize> = 0x10..0x14;

impl Settings {
    /// Decode the 180-byte SETTINGS section payload.
    pub fn decode(payload: &[u8]) -> Result<Settings, DecodeError> {
        need("SETTINGS", payload, LEN)?;
        let mut raw = [0u8; LEN];
        raw.copy_from_slice(&payload[..LEN]);
        // Bytes 0x10..0x14 are a RAM overlay (the boot time after a reboot)
        // that the stored CRC does not cover; it was computed over 0xFFFFFFFF.
        let mut flash = raw;
        flash[OVERLAY].fill(0xFF);
        Ok(Settings {
            block_a_crc_ok: crc_ok(&raw, 0x00..0x5C, 0x5C) || crc_ok(&flash, 0x00..0x5C, 0x5C),
            block_b_crc_ok: crc_ok(&raw, 0x60..0xB0, 0xB0),
            raw,
        })
    }

    /// Block A (0x00..0x5B) matches the CRC-32 at 0x5C.
    pub fn block_a_crc_ok(&self) -> bool {
        self.block_a_crc_ok
    }

    /// Block B (0x60..0xAF) matches the CRC-32 at 0xB0.
    pub fn block_b_crc_ok(&self) -> bool {
        self.block_b_crc_ok
    }

    /// Meter queue rate code: bits 2..3 of the u32 at 0x60.
    pub fn rate_code(&self) -> u8 {
        ((u32_at(&self.raw, 0x60) >> 2) & 0b11) as u8
    }

    /// NUL-terminated device name at 0x70.
    pub fn name(&self) -> String {
        c_str(&self.raw[0x70..0xB0])
    }

    /// The raw 180 payload bytes.
    pub fn raw(&self) -> &[u8] {
        &self.raw
    }

    /// Typed fields, or [`DecodeError::Crc`] if either CRC-32 block fails.
    pub fn meter_settings(&self) -> Result<MeterSettings, DecodeError> {
        if !self.block_a_crc_ok {
            return Err(DecodeError::Crc {
                what: "SETTINGS block A",
            });
        }
        if !self.block_b_crc_ok {
            return Err(DecodeError::Crc {
                what: "SETTINGS block B",
            });
        }
        let w0 = u32_at(&self.raw, 0x00);
        let w60 = u32_at(&self.raw, 0x60);
        let i32_at = |off: usize| u32_at(&self.raw, off) as i32;
        let cal5 = |base: usize| core::array::from_fn(|i| i32_at(base + 4 * i));
        // Field map: research control notes §3.4 (PDM settings dialog,
        // static analysis). The 2026-10-27 screen check confirmed language,
        // brightness, dim timeout, startup logo, theme, auto-rotate and sample
        // rate [device]. Rotation stays [inferred]: the screen showed 0° with
        // auto-rotate on while bits 0..1 of 0x60 read 90°. The calibration
        // values are not shown on the meter and stay [inferred].
        Ok(MeterSettings {
            language: Tagged {
                value: if w0 & 1 == 0 {
                    Language::Chinese
                } else {
                    Language::English
                },
                tag: Tag::Confirmed,
            },
            brightness: Tagged {
                value: ((w0 >> 3) & 0x7F) as u8,
                tag: Tag::Confirmed,
            },
            dim_timeout: Tagged {
                value: match (w0 >> 10) & 0b11 {
                    0 => DimTimeout::Off,
                    1 => DimTimeout::Secs15,
                    2 => DimTimeout::Secs30,
                    _ => DimTimeout::Secs60,
                },
                tag: Tag::Confirmed,
            },
            startup_logo: Tagged {
                value: match (w0 >> 14) & 0b11 {
                    0 => StartupLogo::Off,
                    1 => StartupLogo::Secs1,
                    2 => StartupLogo::Secs3,
                    _ => StartupLogo::Secs5,
                },
                tag: Tag::Confirmed,
            },
            theme: Tagged {
                value: match ((w0 >> 16) & 0b111) as u8 {
                    0 => Theme::Blue,
                    1 => Theme::Green,
                    2 => Theme::Pink,
                    3 => Theme::Orange,
                    4 => Theme::Yellow,
                    5 => Theme::DarkGreen,
                    6 => Theme::Cyan,
                    other => Theme::Unknown(other),
                },
                tag: Tag::Confirmed,
            },
            auto_rotate: Tagged {
                value: (w0 >> 27) & 1 == 1,
                tag: Tag::Confirmed,
            },
            rotation: Tagged {
                value: match w60 & 0b11 {
                    0 => Rotation::Deg0,
                    1 => Rotation::Deg90,
                    2 => Rotation::Deg180,
                    _ => Rotation::Deg270,
                },
                tag: Tag::Inferred,
            },
            sample_rate: Tagged {
                value: match (w60 >> 2) & 0b11 {
                    0 => Rate::Sps2,
                    1 => Rate::Sps10,
                    2 => Rate::Sps50,
                    _ => Rate::Sps1000,
                },
                tag: Tag::Confirmed,
            },
            voltage_cal: Tagged {
                value: cal5(0x1C),
                tag: Tag::Inferred,
            },
            current_cal: Tagged {
                value: cal5(0x30),
                tag: Tag::Inferred,
            },
            offset_cal: Tagged {
                value: i32_at(0x58),
                tag: Tag::Inferred,
            },
        })
    }
}

/// Confidence of a decoded settings field; serializes as [`Tag::as_str`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Tag {
    /// Checked against the meter's screen.
    Confirmed,
    /// From static analysis only.
    Inferred,
}

impl Tag {
    /// Lower-case name (`"confirmed"` / `"inferred"`).
    pub fn as_str(self) -> &'static str {
        match self {
            Tag::Confirmed => "confirmed",
            Tag::Inferred => "inferred",
        }
    }
}

/// A decoded value with its confidence tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tagged<T> {
    /// The decoded value.
    pub value: T,
    /// Its confidence.
    pub tag: Tag,
}

/// Display language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    /// Code 0.
    Chinese,
    /// Code 1.
    English,
}

/// Screen dim timeout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DimTimeout {
    /// Never dims.
    Off,
    /// 15 s.
    Secs15,
    /// 30 s.
    Secs30,
    /// 60 s.
    Secs60,
}

/// Startup logo duration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupLogo {
    /// No logo.
    Off,
    /// 1 s.
    Secs1,
    /// 3 s.
    Secs3,
    /// 5 s.
    Secs5,
}

/// UI colour theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    /// Code 0.
    Blue,
    /// Code 1.
    Green,
    /// Code 2.
    Pink,
    /// Code 3.
    Orange,
    /// Code 4.
    Yellow,
    /// Code 5.
    DarkGreen,
    /// Code 6.
    Cyan,
    /// Any other code, kept raw.
    Unknown(u8),
}

/// Screen rotation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rotation {
    /// 0°.
    Deg0,
    /// 90°.
    Deg90,
    /// 180°.
    Deg180,
    /// 270°.
    Deg270,
}

/// One displayable settings row: the single formatting source for the CLI
/// and the desktop panel (serializes as `{key, label, value, tag}`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Row {
    /// Stable key.
    pub key: &'static str,
    /// Human label.
    pub label: &'static str,
    /// Formatted value.
    pub value: String,
    /// Confidence.
    pub tag: Tag,
}

/// Typed fields of a CRC-checked SETTINGS block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeterSettings {
    /// Display language.
    pub language: Tagged<Language>,
    /// Brightness, percent (raw 7-bit value).
    pub brightness: Tagged<u8>,
    /// Dim timeout.
    pub dim_timeout: Tagged<DimTimeout>,
    /// Startup logo duration.
    pub startup_logo: Tagged<StartupLogo>,
    /// Colour theme.
    pub theme: Tagged<Theme>,
    /// Auto-rotate.
    pub auto_rotate: Tagged<bool>,
    /// Screen rotation.
    pub rotation: Tagged<Rotation>,
    /// Queue sample rate.
    pub sample_rate: Tagged<Rate>,
    /// Voltage calibration gains (raw).
    pub voltage_cal: Tagged<[i32; 5]>,
    /// Current calibration gains (raw).
    pub current_cal: Tagged<[i32; 5]>,
    /// Offset calibration (raw).
    pub offset_cal: Tagged<i32>,
}

impl MeterSettings {
    /// Decode a 180-byte SETTINGS payload; fails on length or CRC mismatch.
    pub fn decode(payload: &[u8]) -> Result<MeterSettings, DecodeError> {
        Settings::decode(payload)?.meter_settings()
    }

    /// Displayable rows: every field with its label, formatted value and tag.
    /// Calibration values are shown raw and read-only.
    pub fn rows(&self) -> Vec<Row> {
        fn row<T>(key: &'static str, label: &'static str, f: &Tagged<T>, value: String) -> Row {
            Row {
                key,
                label,
                value,
                tag: f.tag,
            }
        }
        fn join(v: &[i32]) -> String {
            v.iter()
                .map(|x| x.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        }
        let on_off = |b: bool| if b { "on" } else { "off" }.to_string();
        vec![
            row(
                "language",
                "Language",
                &self.language,
                match self.language.value {
                    Language::Chinese => "Chinese",
                    Language::English => "English",
                }
                .to_string(),
            ),
            row(
                "brightness",
                "Brightness",
                &self.brightness,
                format!("{}%", self.brightness.value),
            ),
            row(
                "dim_timeout",
                "Dim screen after",
                &self.dim_timeout,
                match self.dim_timeout.value {
                    DimTimeout::Off => "off",
                    DimTimeout::Secs15 => "15 s",
                    DimTimeout::Secs30 => "30 s",
                    DimTimeout::Secs60 => "60 s",
                }
                .to_string(),
            ),
            row(
                "startup_logo",
                "Startup logo",
                &self.startup_logo,
                match self.startup_logo.value {
                    StartupLogo::Off => "off",
                    StartupLogo::Secs1 => "1 s",
                    StartupLogo::Secs3 => "3 s",
                    StartupLogo::Secs5 => "5 s",
                }
                .to_string(),
            ),
            row(
                "theme",
                "Theme",
                &self.theme,
                match self.theme.value {
                    Theme::Blue => "blue".to_string(),
                    Theme::Green => "green".to_string(),
                    Theme::Pink => "pink".to_string(),
                    Theme::Orange => "orange".to_string(),
                    Theme::Yellow => "yellow".to_string(),
                    Theme::DarkGreen => "dark green".to_string(),
                    Theme::Cyan => "cyan".to_string(),
                    Theme::Unknown(c) => format!("unknown ({c})"),
                },
            ),
            row(
                "auto_rotate",
                "Auto-rotate",
                &self.auto_rotate,
                on_off(self.auto_rotate.value),
            ),
            row(
                "rotation",
                "Rotation",
                &self.rotation,
                match self.rotation.value {
                    Rotation::Deg0 => "0°",
                    Rotation::Deg90 => "90°",
                    Rotation::Deg180 => "180°",
                    Rotation::Deg270 => "270°",
                }
                .to_string(),
            ),
            row(
                "sample_rate",
                "Sample rate",
                &self.sample_rate,
                format!("{} SPS", self.sample_rate.value.sps()),
            ),
            row(
                "voltage_cal",
                "Voltage calibration (raw)",
                &self.voltage_cal,
                join(&self.voltage_cal.value),
            ),
            row(
                "current_cal",
                "Current calibration (raw)",
                &self.current_cal,
                join(&self.current_cal.value),
            ),
            row(
                "offset_cal",
                "Offset calibration (raw)",
                &self.offset_cal,
                self.offset_cal.value.to_string(),
            ),
        ]
    }
}

// ---- Settings write (type 0x0A), see docs/protocol.md "Settings write" ----

/// Key of the startup logo time: sent at its read value, never editable.
/// \[static\]
pub const STARTUP_LOGO_KEY: u16 = 0x06;

/// Key of the trailing "save" entry. \[static\] (meaning \[inferred\])
pub const SAVE_MARKER_KEY: u16 = 0x7FFF;

/// Value of the trailing "save" entry. \[static\]
pub const SAVE_MARKER: i32 = 0x1234;

/// Every key of a settings write, in the order sent. \[static\]
pub const SETTINGS_WRITE_KEYS: [u16; 8] = [0x01, 0x03, 0x04, 0x06, 0x07, 0x14, 0x15, 0x7FFF];

/// Message type of the settings write. \[static\]
pub const SETTINGS_WRITE_TYPE: u8 = 0x0A;

/// Where the value of one settings-write key lives in the SETTINGS block:
/// `width` bits at `shift` of the little-endian u32 at `offset`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyField {
    /// Settings-write key.
    pub key: u16,
    /// Byte offset of the u32 in the 180-byte block.
    pub offset: usize,
    /// Lowest bit of the field.
    pub shift: u32,
    /// Width in bits.
    pub width: u32,
}

/// Every settings-write key but the save marker, with the block bits the
/// meter stores its value in. Keys 0x01..0x07 round-trip as the vendor
/// dialog reads and writes them \[static\] \[device\]. Keys 0x14 and 0x15 are
/// the other way round from the vendor dialog, which pairs 0x14 with bits
/// 2..3 of 0x60 and 0x15 with bits 0..1 \[static\]: on the KM003C (FW 2.0.6,
/// 2026-10-29) a write of 0x14 = 2, 0x15 = 1 read back as bits 0..1 = 2 and
/// rate bits 2..3 = 1 \[device\].
pub const SETTINGS_KEY_FIELDS: [KeyField; 7] = [
    KeyField {
        key: 0x01,
        offset: 0x00,
        shift: 0,
        width: 1,
    },
    KeyField {
        key: 0x03,
        offset: 0x00,
        shift: 3,
        width: 7,
    },
    KeyField {
        key: 0x04,
        offset: 0x00,
        shift: 10,
        width: 2,
    },
    KeyField {
        key: 0x06,
        offset: 0x00,
        shift: 14,
        width: 2,
    },
    KeyField {
        key: 0x07,
        offset: 0x00,
        shift: 16,
        width: 3,
    },
    KeyField {
        key: 0x14,
        offset: 0x60,
        shift: 0,
        width: 2,
    },
    KeyField {
        key: 0x15,
        offset: 0x60,
        shift: 2,
        width: 2,
    },
];

/// Store the values of a settings write in `block` (a 180-byte SETTINGS
/// payload) the way the meter does, per [`SETTINGS_KEY_FIELDS`]; the CRCs are
/// left as they were. A frame that is not a full write in the documented
/// layout (every key of [`SETTINGS_WRITE_KEYS`] in order), or a block too
/// short for a field, changes nothing and gives `false`.
pub fn apply_settings_write(block: &mut [u8], frame: &[u8]) -> bool {
    let n = SETTINGS_WRITE_KEYS.len();
    if frame.len() != 4 + 8 * n || frame[0] & 0x7F != SETTINGS_WRITE_TYPE || block.len() < LEN {
        return false;
    }
    let entry = |i: usize| {
        let at = 4 + 8 * i;
        let word = u32_at(frame, at);
        (word, u32_at(frame, at + 4))
    };
    if (0..n).any(|i| Some(entry(i).0) != settings_entry_word(i)) {
        return false;
    }
    for (i, &key) in SETTINGS_WRITE_KEYS.iter().enumerate() {
        let Some(f) = SETTINGS_KEY_FIELDS.iter().find(|f| f.key == key) else {
            continue;
        };
        let mask = ((1u32 << f.width) - 1) << f.shift;
        let w = (u32_at(block, f.offset) & !mask) | ((entry(i).1 << f.shift) & mask);
        block[f.offset..f.offset + 4].copy_from_slice(&w.to_le_bytes());
    }
    true
}

/// A settings field the app may change. Auto-rotate has no recovered key,
/// the startup logo is sent only at its read value, and calibration is
/// never written. \[static\]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WritableField {
    /// Key 0x01: 0 = Chinese, 1 = English.
    Language,
    /// Key 0x03: 1..100.
    Brightness,
    /// Key 0x04: 0..3 (off, 15 s, 30 s, 60 s).
    DimTimeout,
    /// Key 0x07: 0..6.
    Theme,
    /// Key 0x15: rate code 0..3 \[device\].
    SampleRate,
    /// Key 0x14: 0..3 (0°, 90°, 180°, 270°) \[device\] (key); the meaning
    /// of the codes stays \[inferred\].
    Rotation,
}

impl WritableField {
    /// Every writable field, in key order.
    pub const ALL: [WritableField; 6] = [
        WritableField::Language,
        WritableField::Brightness,
        WritableField::DimTimeout,
        WritableField::Theme,
        WritableField::SampleRate,
        WritableField::Rotation,
    ];

    /// Stable name (the [`Row::key`] of the field).
    pub fn name(self) -> &'static str {
        match self {
            WritableField::Language => "language",
            WritableField::Brightness => "brightness",
            WritableField::DimTimeout => "dim_timeout",
            WritableField::Theme => "theme",
            WritableField::SampleRate => "sample_rate",
            WritableField::Rotation => "rotation",
        }
    }

    /// The field for an exact [`name`](Self::name); `None` for anything else.
    pub fn from_name(name: &str) -> Option<WritableField> {
        WritableField::ALL.into_iter().find(|f| f.name() == name)
    }

    /// The field written under `key`; `None` for the startup logo, the save
    /// marker and every key outside [`SETTINGS_WRITE_KEYS`].
    pub fn from_key(key: u16) -> Option<WritableField> {
        WritableField::ALL.into_iter().find(|f| f.key() == key)
    }

    /// Key word of the field in a settings write. \[static\]
    pub fn key(self) -> u16 {
        match self {
            WritableField::Language => 0x01,
            WritableField::Brightness => 0x03,
            WritableField::DimTimeout => 0x04,
            WritableField::Theme => 0x07,
            WritableField::SampleRate => 0x15,
            WritableField::Rotation => 0x14,
        }
    }

    /// Values the meter accepts. \[static\]
    pub fn range(self) -> RangeInclusive<i32> {
        match self {
            WritableField::Language => 0..=1,
            WritableField::Brightness => 1..=100,
            WritableField::DimTimeout | WritableField::SampleRate | WritableField::Rotation => {
                0..=3
            }
            WritableField::Theme => 0..=6,
        }
    }

    /// The code a write of `changes` over `current` leaves in this field:
    /// the last change to it, else its read code.
    pub fn code_after(self, current: &MeterSettings, changes: &[(WritableField, i32)]) -> i32 {
        changes
            .iter()
            .rev()
            .find(|(c, _)| *c == self)
            .map_or_else(|| self.code(current), |&(_, v)| v)
    }

    /// The field's current wire code in `settings`.
    pub fn code(self, settings: &MeterSettings) -> i32 {
        match self {
            WritableField::Language => match settings.language.value {
                Language::Chinese => 0,
                Language::English => 1,
            },
            WritableField::Brightness => i32::from(settings.brightness.value),
            WritableField::DimTimeout => match settings.dim_timeout.value {
                DimTimeout::Off => 0,
                DimTimeout::Secs15 => 1,
                DimTimeout::Secs30 => 2,
                DimTimeout::Secs60 => 3,
            },
            WritableField::Theme => i32::from(match settings.theme.value {
                Theme::Blue => 0,
                Theme::Green => 1,
                Theme::Pink => 2,
                Theme::Orange => 3,
                Theme::Yellow => 4,
                Theme::DarkGreen => 5,
                Theme::Cyan => 6,
                Theme::Unknown(c) => c,
            }),
            WritableField::SampleRate => i32::from(settings.sample_rate.value.code()),
            WritableField::Rotation => match settings.rotation.value {
                Rotation::Deg0 => 0,
                Rotation::Deg90 => 1,
                Rotation::Deg180 => 2,
                Rotation::Deg270 => 3,
            },
        }
    }
}

/// Startup logo wire code (0..3) in `settings`.
fn startup_logo_code(settings: &MeterSettings) -> i32 {
    match settings.startup_logo.value {
        StartupLogo::Off => 0,
        StartupLogo::Secs1 => 1,
        StartupLogo::Secs3 => 2,
        StartupLogo::Secs5 => 3,
    }
}

/// Values a settings write may carry for `key`; `None` for a key outside
/// [`SETTINGS_WRITE_KEYS`]. \[static\]
pub fn settings_key_range(key: u16) -> Option<RangeInclusive<i32>> {
    match key {
        STARTUP_LOGO_KEY => Some(0..=3),
        SAVE_MARKER_KEY => Some(SAVE_MARKER..=SAVE_MARKER),
        _ => WritableField::from_key(key).map(WritableField::range),
    }
}

/// Key word of entry `index` of a settings write: `0x01008000 | key`, with
/// the `next` bit (15) clear on the last entry only; `None` past the last
/// key of [`SETTINGS_WRITE_KEYS`]. \[static\]
pub fn settings_entry_word(index: usize) -> Option<u32> {
    let key = *SETTINGS_WRITE_KEYS.get(index)?;
    let next = if index + 1 < SETTINGS_WRITE_KEYS.len() {
        1u32 << 15
    } else {
        0
    };
    Some(0x0100_0000 | next | u32::from(key))
}

/// A settings change the builder refuses; nothing is sent.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SettingsWriteError {
    /// The new value is outside the field's range.
    #[error("{field} = {value} is out of range ({min}..={max})")]
    OutOfRange {
        /// Field name.
        field: &'static str,
        /// Refused value.
        value: i32,
        /// Lowest allowed value.
        min: i32,
        /// Highest allowed value.
        max: i32,
    },
}

/// The type-0x0A frame that writes `changes` over `current` (the settings
/// just read): main header `0x0A | id << 8 | n << 23`, then one 8-byte entry
/// per key of [`SETTINGS_WRITE_KEYS`] (key word `0x01008000 | key`, `next`
/// clear on the last; i32 value). Changed fields go out at their new values,
/// every other key at its read value, the startup logo always at its read
/// value, and the save marker last. \[static\]
///
/// A value outside its field's [`range`](WritableField::range) is refused.
pub fn settings_write_request(
    id: u8,
    current: &MeterSettings,
    changes: &[(WritableField, i32)],
) -> Result<Vec<u8>, SettingsWriteError> {
    for &(f, value) in changes {
        let range = f.range();
        if !range.contains(&value) {
            return Err(SettingsWriteError::OutOfRange {
                field: f.name(),
                value,
                min: *range.start(),
                max: *range.end(),
            });
        }
    }
    let value_of = |key: u16| -> i32 {
        match key {
            STARTUP_LOGO_KEY => startup_logo_code(current),
            SAVE_MARKER_KEY => SAVE_MARKER,
            _ => WritableField::from_key(key).map_or(0, |f| f.code_after(current, changes)),
        }
    };
    let n = SETTINGS_WRITE_KEYS.len() as u32;
    let mut frame = Vec::with_capacity(4 + 8 * SETTINGS_WRITE_KEYS.len());
    frame.extend((u32::from(SETTINGS_WRITE_TYPE) | (u32::from(id) << 8) | (n << 23)).to_le_bytes());
    for (i, &key) in SETTINGS_WRITE_KEYS.iter().enumerate() {
        let word = settings_entry_word(i).unwrap_or_default();
        frame.extend(word.to_le_bytes());
        frame.extend(value_of(key).to_le_bytes());
    }
    Ok(frame)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::test_util::section_payload;

    const SETTINGS_188: &str = include_str!("../../tests/vectors/settings_188.hex");

    fn captured() -> Vec<u8> {
        let p = section_payload(SETTINGS_188);
        assert_eq!(p.len(), 180);
        p
    }

    // ---- Hardening: RAM overlay at 0x10..0x14 (device session 1) ----

    #[test]
    fn decode_block_a_crc_ignores_the_boot_time_overlay() {
        // The captured block has 0xFFFFFFFF at 0x10; after a reboot the meter
        // shows its boot time there while the stored CRC is unchanged.
        let mut p = captured();
        assert_eq!(&p[0x10..0x14], &[0xFF; 4]);
        p[0x10..0x14].copy_from_slice(&0x6AD7_84A2u32.to_le_bytes());
        let s = Settings::decode(&p).unwrap();
        assert!(s.block_a_crc_ok());
        assert_eq!(&s.raw()[0x10..0x14], &0x6AD7_84A2u32.to_le_bytes());
    }

    #[test]
    fn decode_block_a_real_corruption_still_fails_with_overlay_present() {
        let mut p = captured();
        p[0x10..0x14].copy_from_slice(&0x6AD7_84A2u32.to_le_bytes());
        p[0x20] ^= 0x01;
        assert!(!Settings::decode(&p).unwrap().block_a_crc_ok());
    }

    // ---- AC-E88WR5.6: CRCs, rate code, name, raw bytes ----

    #[test]
    fn decode_captured_settings_passes_both_crcs() {
        let p = captured();
        let s = Settings::decode(&p).unwrap();
        assert!(s.block_a_crc_ok());
        assert!(s.block_b_crc_ok());
        assert_eq!(s.rate_code(), 2);
        assert_eq!(s.name(), "POWER-Z");
        assert_eq!(s.raw(), &p[..]);
    }

    #[test]
    fn decode_corrupt_block_a_fails_only_crc_a() {
        let mut p = captured();
        p[0x30] ^= 0x01;
        let s = Settings::decode(&p).unwrap();
        assert!(!s.block_a_crc_ok());
        assert!(s.block_b_crc_ok());

        let mut p = captured();
        p[0x5C] ^= 0x80; // the stored CRC itself
        let s = Settings::decode(&p).unwrap();
        assert!(!s.block_a_crc_ok());
        assert!(s.block_b_crc_ok());
    }

    #[test]
    fn decode_corrupt_block_b_fails_only_crc_b() {
        let mut p = captured();
        p[0x7A] ^= 0x01; // inside the name area, past the NUL
        let s = Settings::decode(&p).unwrap();
        assert!(s.block_a_crc_ok());
        assert!(!s.block_b_crc_ok());

        let mut p = captured();
        p[0xB3] ^= 0x01;
        let s = Settings::decode(&p).unwrap();
        assert!(s.block_a_crc_ok());
        assert!(!s.block_b_crc_ok());
    }

    #[test]
    fn decode_flags_change_breaks_only_crc_b() {
        // 0x60 flags lie in block B; changing them keeps block A valid.
        let mut p = captured();
        p[0x60] ^= 0x01;
        let s = Settings::decode(&p).unwrap();
        assert!(s.block_a_crc_ok());
        assert!(!s.block_b_crc_ok());
    }

    #[test]
    fn decode_rate_code_reads_bits_2_and_3_of_0x60() {
        let mut p = captured();
        for code in 0..=3u32 {
            let flags = 0xFFFF_FFF3 | (code << 2);
            p[0x60..0x64].copy_from_slice(&flags.to_le_bytes());
            assert_eq!(Settings::decode(&p).unwrap().rate_code(), code as u8);
        }
    }

    #[test]
    fn decode_name_stops_at_nul() {
        let mut p = captured();
        p[0x70..0x80].copy_from_slice(b"ABC\0XYZ\0\0\0\0\0\0\0\0\0");
        assert_eq!(Settings::decode(&p).unwrap().name(), "ABC");
    }

    #[test]
    fn decode_short_settings_is_error() {
        let p = captured();
        for len in [0usize, 4, 96, 179] {
            assert!(
                matches!(
                    Settings::decode(&p[..len]),
                    Err(DecodeError::Length { got, .. }) if got == len
                ),
                "len {len} should be a length error"
            );
        }
    }
}
