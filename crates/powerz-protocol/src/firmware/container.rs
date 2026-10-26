//! The vendor firmware container (FR V9SK53).
//!
//! A Qt data stream, big-endian `[static]`: a `u32` string count (10), then
//! that many strings, each a `u32` byte length followed by UTF-16BE code
//! units (length `0xFFFFFFFF` is a null string), then a byte array: a `u32`
//! length and that many bytes, the encrypted firmware image. The ten
//! strings are, in order: magic (`Firmware file`), vendor, container
//! version, model, firmware version, date, CRC-32 (8 hex digits), size
//! (decimal), Chinese release notes and English release notes `[static]`.
//! The CRC and size describe the decrypted image the meter checks
//! `[inferred]`; the payload is never decrypted here and is borrowed from
//! the input unchanged. See `docs/protocol.md`.

/// The magic string every container starts with `[static]`.
pub const MAGIC: &str = "Firmware file";

/// Number of header strings in a container `[static]`.
pub const STRING_COUNT: u32 = 10;

/// Largest container accepted, in bytes. Real images are far smaller; the
/// cap bounds memory before anything is parsed.
pub const MAX_CONTAINER_BYTES: usize = 8 * 1024 * 1024;

/// Length prefix of a null string in a Qt data stream.
const NULL_LEN: u32 = u32::MAX;

/// Most characters of file text echoed back in a refusal.
const ECHO_MAX_CHARS: usize = 32;

/// `text` cut to [`ECHO_MAX_CHARS`] characters, marked with `…` when cut.
fn echo(text: &str) -> String {
    let mut chars = text.chars();
    let mut out: String = chars.by_ref().take(ECHO_MAX_CHARS).collect();
    if chars.next().is_some() {
        out.push('…');
    }
    out
}

/// The part of a container a truncated file ends inside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerField {
    /// The leading string count.
    StringCount,
    /// A header string, by 0-based index (0 = magic).
    HeaderString(usize),
    /// The payload length prefix.
    PayloadLength,
    /// The payload bytes.
    Payload,
}

impl std::fmt::Display for ContainerField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StringCount => f.write_str("string count"),
            Self::HeaderString(index) => write!(f, "header string {index}"),
            Self::PayloadLength => f.write_str("payload length"),
            Self::Payload => f.write_str("payload"),
        }
    }
}

/// Why a firmware container was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ContainerError {
    /// The input is larger than [`MAX_CONTAINER_BYTES`].
    #[error(
        "The file is too large to be a firmware container: {len} bytes, at most {max} allowed."
    )]
    TooLarge {
        /// The input length.
        len: usize,
        /// The cap, [`MAX_CONTAINER_BYTES`].
        max: usize,
    },
    /// The input ends before the field being read.
    #[error("The file is truncated: it ends inside the {what} at byte {offset}.")]
    Truncated {
        /// The field being read.
        what: ContainerField,
        /// Byte offset where the field starts.
        offset: usize,
    },
    /// The string count is not the documented ten.
    #[error("The file is not a firmware container: it holds {count} header strings, 10 expected.")]
    StringCount {
        /// The count read from the file.
        count: u32,
    },
    /// A header string has an odd byte length, so it is not UTF-16.
    #[error("The file is corrupt: header string {index} has an odd byte length.")]
    OddStringLength {
        /// Index of the header string (0 = magic).
        index: usize,
    },
    /// A header string is not valid UTF-16 (e.g. a lone surrogate).
    #[error("The file is corrupt: header string {index} is not valid UTF-16 text.")]
    BadString {
        /// Index of the header string (0 = magic).
        index: usize,
    },
    /// The first header string is not the firmware magic.
    #[error("The file is not a firmware file: its magic is {found:?}, \"Firmware file\" expected.")]
    BadMagic {
        /// The magic read from the file (empty for a null string), cut to 32
        /// characters.
        found: String,
    },
    /// The CRC text is not 8 hex digits.
    #[error("The file is corrupt: its CRC {text:?} is not 8 hex digits.")]
    BadCrc {
        /// The CRC text read from the file, cut to 32 characters.
        text: String,
    },
    /// The size text is not a decimal `u32`.
    #[error("The file is corrupt: its size {text:?} is not a decimal number.")]
    BadSize {
        /// The size text read from the file, cut to 32 characters.
        text: String,
    },
    /// The payload is shorter than the declared size.
    #[error(
        "The file is corrupt: it declares a {declared}-byte image but holds only {actual} bytes."
    )]
    PayloadTooShort {
        /// The size declared in the header.
        declared: u32,
        /// The payload length actually present.
        actual: usize,
    },
    /// The file is for a different model than the connected meter.
    #[error("The firmware file is for a {file}, but the connected meter is a {meter}.")]
    ModelMismatch {
        /// The model named in the file.
        file: String,
        /// The model of the connected meter.
        meter: String,
    },
}

/// A parsed firmware container. The header strings are decoded; the
/// payload borrows the input byte for byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirmwareContainer<'a> {
    magic: String,
    vendor: String,
    container_version: String,
    model: String,
    firmware_version: String,
    date: String,
    crc: u32,
    size: u32,
    release_notes_cn: String,
    release_notes_en: String,
    payload: &'a [u8],
}

/// A big-endian reader over the input that never reads past its end.
struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn truncated(&self, what: ContainerField) -> ContainerError {
        ContainerError::Truncated {
            what,
            offset: self.pos,
        }
    }

    fn take(&mut self, n: usize, what: ContainerField) -> Result<&'a [u8], ContainerError> {
        let out = self
            .pos
            .checked_add(n)
            .and_then(|end| self.bytes.get(self.pos..end))
            .ok_or_else(|| self.truncated(what))?;
        self.pos += n;
        Ok(out)
    }

    fn u32(&mut self, what: ContainerField) -> Result<u32, ContainerError> {
        let b = self.take(4, what)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// A Qt string; a null string reads as empty.
    fn string(&mut self, index: usize) -> Result<String, ContainerError> {
        let what = ContainerField::HeaderString(index);
        let len = self.u32(what)?;
        if len == NULL_LEN {
            return Ok(String::new());
        }
        if len % 2 != 0 {
            return Err(ContainerError::OddStringLength { index });
        }
        let len = usize::try_from(len).map_err(|_| self.truncated(what))?;
        let raw = self.take(len, what)?;
        let units: Vec<u16> = raw
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_be_bytes(*c))
            .collect();
        String::from_utf16(&units).map_err(|_| ContainerError::BadString { index })
    }
}

fn parse_crc(text: &str) -> Result<u32, ContainerError> {
    let bad = || ContainerError::BadCrc { text: echo(text) };
    if text.len() != 8 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(bad());
    }
    u32::from_str_radix(text, 16).map_err(|_| bad())
}

fn parse_size(text: &str) -> Result<u32, ContainerError> {
    let bad = || ContainerError::BadSize { text: echo(text) };
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(bad());
    }
    text.parse::<u32>().map_err(|_| bad())
}

impl<'a> FirmwareContainer<'a> {
    /// Parse a container, validating the magic, CRC and size text and that
    /// the payload holds at least the declared size.
    /// Inputs over [`MAX_CONTAINER_BYTES`] are refused before parsing.
    pub fn parse(bytes: &'a [u8]) -> Result<FirmwareContainer<'a>, ContainerError> {
        if bytes.len() > MAX_CONTAINER_BYTES {
            return Err(ContainerError::TooLarge {
                len: bytes.len(),
                max: MAX_CONTAINER_BYTES,
            });
        }
        let mut r = Reader { bytes, pos: 0 };
        let count = r.u32(ContainerField::StringCount)?;
        if count != STRING_COUNT {
            return Err(ContainerError::StringCount { count });
        }
        let mut s: [String; STRING_COUNT as usize] = Default::default();
        for (index, slot) in s.iter_mut().enumerate() {
            *slot = r.string(index)?;
        }
        let len = r.u32(ContainerField::PayloadLength)?;
        let len = usize::try_from(len).map_err(|_| r.truncated(ContainerField::Payload))?;
        let payload = r.take(len, ContainerField::Payload)?;

        let [
            magic,
            vendor,
            container_version,
            model,
            firmware_version,
            date,
            crc,
            size,
            release_notes_cn,
            release_notes_en,
        ] = s;
        if magic != MAGIC {
            return Err(ContainerError::BadMagic {
                found: echo(&magic),
            });
        }
        let crc = parse_crc(&crc)?;
        let size = parse_size(&size)?;
        if payload.len() < size as usize {
            return Err(ContainerError::PayloadTooShort {
                declared: size,
                actual: payload.len(),
            });
        }
        Ok(FirmwareContainer {
            magic,
            vendor,
            container_version,
            model,
            firmware_version,
            date,
            crc,
            size,
            release_notes_cn,
            release_notes_en,
            payload,
        })
    }

    /// The magic string (`Firmware file`).
    pub fn magic(&self) -> &str {
        &self.magic
    }

    /// The vendor name.
    pub fn vendor(&self) -> &str {
        &self.vendor
    }

    /// The container format version (e.g. `V1.0`).
    pub fn container_version(&self) -> &str {
        &self.container_version
    }

    /// The meter model the image is for (e.g. `KM003C`).
    pub fn model(&self) -> &str {
        &self.model
    }

    /// The firmware version (e.g. `2.0.7`).
    pub fn firmware_version(&self) -> &str {
        &self.firmware_version
    }

    /// The release date as written (e.g. `2026.9.29`).
    pub fn date(&self) -> &str {
        &self.date
    }

    /// The CRC-32 of the decrypted image `[inferred]`.
    pub fn crc(&self) -> u32 {
        self.crc
    }

    /// The CRC-32 as 8 uppercase hex digits, the form the CLI and the
    /// desktop app show.
    pub fn crc_hex(&self) -> String {
        format!("{:08X}", self.crc)
    }

    /// The size of the decrypted image in bytes `[inferred]`.
    pub fn size(&self) -> u32 {
        self.size
    }

    /// The Chinese release notes.
    pub fn release_notes_cn(&self) -> &str {
        &self.release_notes_cn
    }

    /// The English release notes.
    pub fn release_notes_en(&self) -> &str {
        &self.release_notes_en
    }

    /// The encrypted payload, borrowed from the input unchanged.
    pub fn payload(&self) -> &'a [u8] {
        self.payload
    }

    /// Refuse the file when its model differs from the connected meter's.
    pub fn check_model(&self, meter: &str) -> Result<(), ContainerError> {
        if self.model == meter {
            Ok(())
        } else {
            Err(ContainerError::ModelMismatch {
                file: self.model.clone(),
                meter: meter.to_owned(),
            })
        }
    }
}

/// Why [`read_container_file`] gave up. Each message names the file.
#[derive(Debug, thiserror::Error)]
pub enum ContainerFileError {
    /// The file could not be opened or read.
    #[error("Could not read {path}: {source}")]
    Io {
        /// The file, as displayed.
        path: String,
        /// The OS error.
        source: std::io::Error,
    },
    /// The file was read but refused as a container (e.g. too large).
    #[error("{path}: {source}")]
    Container {
        /// The file, as displayed.
        path: String,
        /// Why it was refused.
        source: ContainerError,
    },
}

/// Read the file at `path` for [`FirmwareContainer::parse`], reading at
/// most [`MAX_CONTAINER_BYTES`] + 1 bytes. A file whose size (or content)
/// exceeds the cap is refused as [`ContainerError::TooLarge`] before it is
/// read in full.
pub fn read_container_file(path: &std::path::Path) -> Result<Vec<u8>, ContainerFileError> {
    use std::io::Read;

    let shown = path.display().to_string();
    let io = |source| ContainerFileError::Io {
        path: shown.clone(),
        source,
    };
    let too_large = |len: usize| ContainerFileError::Container {
        path: shown.clone(),
        source: ContainerError::TooLarge {
            len,
            max: MAX_CONTAINER_BYTES,
        },
    };
    let file = std::fs::File::open(path).map_err(io)?;
    let len = file.metadata().map_err(io)?.len();
    if len > MAX_CONTAINER_BYTES as u64 {
        return Err(too_large(usize::try_from(len).unwrap_or(usize::MAX)));
    }
    let mut bytes = Vec::new();
    file.take(MAX_CONTAINER_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    if bytes.len() > MAX_CONTAINER_BYTES {
        return Err(too_large(bytes.len()));
    }
    Ok(bytes)
}
