//! Synthetic firmware containers for the tests (FR V9SK53, AC .4).
//!
//! Writes the documented container layout (`docs/protocol.md`): a Qt data
//! stream, big-endian. A `u32` string count (10), then that many strings,
//! each a `u32` byte length followed by UTF-16BE code units (length
//! `0xFFFFFFFF` is a null string), then a byte array: a `u32` length and
//! that many bytes (the encrypted payload). Every value here is made up by
//! the test suite; no vendor file is read or copied.
//!
//! Shared by the protocol, CLI and desktop tests through `#[path]`, so it
//! never ships in a library build.

#![allow(dead_code)]

/// Field order of the ten header strings.
pub const MAGIC: usize = 0;
pub const VENDOR: usize = 1;
pub const CONTAINER_VERSION: usize = 2;
pub const MODEL: usize = 3;
pub const FIRMWARE_VERSION: usize = 4;
pub const DATE: usize = 5;
pub const CRC: usize = 6;
pub const SIZE: usize = 7;
pub const NOTES_CN: usize = 8;
pub const NOTES_EN: usize = 9;

/// Synthetic English release notes (multi-line, like the real ones).
pub const EN_NOTES: &str = "1. Synthetic note one.\n2. Synthetic note two.";
/// Synthetic Chinese release notes (non-ASCII, to exercise UTF-16).
pub const CN_NOTES: &str = "1. 测试说明一。\n2. 测试说明二。";

/// A builder for synthetic firmware containers.
#[derive(Debug, Clone)]
pub struct ContainerBuilder {
    /// The header strings in order; `None` is a null QString.
    pub strings: Vec<Option<String>>,
    /// The string count written up front; `None` writes `strings.len()`.
    pub count: Option<u32>,
    /// The encrypted payload bytes.
    pub payload: Vec<u8>,
    /// The byte-array length written up front; `None` writes `payload.len()`.
    pub payload_len: Option<u32>,
    /// Raw UTF-16 code units written in place of a header string (by
    /// index), e.g. an unpaired surrogate no Rust `String` can hold.
    pub raw_units: Vec<(usize, Vec<u16>)>,
}

impl Default for ContainerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ContainerBuilder {
    /// A valid KM003C 2.0.7 container with a 64-byte synthetic payload
    /// whose declared size is 48 (the payload is 16 bytes longer, as in
    /// the real file).
    pub fn new() -> ContainerBuilder {
        let payload: Vec<u8> = (0..64u32).map(|i| (i * 37 + 11) as u8).collect();
        ContainerBuilder {
            strings: [
                "Firmware file",
                "ChargerLab",
                "V1.0",
                "KM003C",
                "2.0.7",
                "2026.9.29",
                "A175E4A0",
                "48",
                CN_NOTES,
                EN_NOTES,
            ]
            .iter()
            .map(|s| Some((*s).to_owned()))
            .collect(),
            count: None,
            payload,
            payload_len: None,
            raw_units: Vec::new(),
        }
    }

    /// Replace header string `index`.
    pub fn field(mut self, index: usize, value: &str) -> ContainerBuilder {
        self.strings[index] = Some(value.to_owned());
        self
    }

    /// Make header string `index` a null QString.
    pub fn null_field(mut self, index: usize) -> ContainerBuilder {
        self.strings[index] = None;
        self
    }

    /// Write header string `index` as these raw UTF-16 code units.
    pub fn raw_field(mut self, index: usize, units: &[u16]) -> ContainerBuilder {
        self.raw_units.retain(|(i, _)| *i != index);
        self.raw_units.push((index, units.to_vec()));
        self
    }

    /// The code units written for header string `index` (`None` = null).
    fn units(&self, index: usize) -> Option<Vec<u16>> {
        if let Some((_, u)) = self.raw_units.iter().find(|(i, _)| *i == index) {
            return Some(u.clone());
        }
        self.strings[index]
            .as_ref()
            .map(|s| s.encode_utf16().collect())
    }

    /// Byte offset where header string `index`'s length prefix starts.
    pub fn string_offset(&self, index: usize) -> usize {
        4 + (0..index)
            .map(|i| 4 + self.units(i).map_or(0, |u| u.len() * 2))
            .sum::<usize>()
    }

    /// Replace the payload, keeping the declared size text.
    pub fn payload(mut self, payload: &[u8]) -> ContainerBuilder {
        self.payload = payload.to_vec();
        self
    }

    /// Replace the payload and declare its full length as the size.
    pub fn payload_sized(self, payload: &[u8]) -> ContainerBuilder {
        let n = payload.len().to_string();
        self.payload(payload).field(SIZE, &n)
    }

    /// Write `count` as the string count (the strings are unchanged).
    pub fn count(mut self, count: u32) -> ContainerBuilder {
        self.count = Some(count);
        self
    }

    /// Keep only the first `n` strings (and write `n` as the count).
    pub fn strings_only(mut self, n: usize) -> ContainerBuilder {
        self.strings.truncate(n);
        self
    }

    /// Byte offset of the payload's first byte in [`build`](Self::build).
    pub fn payload_offset(&self) -> usize {
        self.string_offset(self.strings.len()) + 4
    }

    /// Serialise the container.
    pub fn build(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let count = self.count.unwrap_or(self.strings.len() as u32);
        out.extend_from_slice(&count.to_be_bytes());
        for index in 0..self.strings.len() {
            match self.units(index) {
                None => out.extend_from_slice(&u32::MAX.to_be_bytes()),
                Some(units) => {
                    out.extend_from_slice(&((units.len() * 2) as u32).to_be_bytes());
                    for u in units {
                        out.extend_from_slice(&u.to_be_bytes());
                    }
                }
            }
        }
        let len = self.payload_len.unwrap_or(self.payload.len() as u32);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(&self.payload);
        out
    }
}
