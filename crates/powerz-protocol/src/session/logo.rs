//! Startup-logo upload, Remove and read in the session (FR TZK5K4). See
//! `docs/protocol.md` "Startup logo".
//!
//! Every write goes through one of the transport's narrow logo paths
//! (chunk 0x4A, flag SET_DATA 0x48 att 0x1C) and every read through
//! [`Session::logo_memory_read`]; nothing is retried. \[static\]

use std::fmt;
use std::time::Duration;

use super::{Clock, Error, Session};
use crate::decode::records::MEMORY_READ_MAX_LEN;
use crate::decode::settings::StartupLogo;
use crate::frame::{self, Reply};
use crate::logo::{self, CHUNK, HEADER_LEN, LogoBlob, LogoMode, Rgba};
use crate::transport::Transport;
use crate::transport::guard::{
    LOGO_CHUNK_TYPE, LOGO_FLAG_ATT, LOGO_REGION_START, LOGO_WINDOW_START,
};

/// Which part of a logo action a [`LogoProgress`] counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogoStep {
    /// Chunk writes.
    Chunks,
    /// Read-back reads.
    ReadBack,
}

/// Progress of a logo action: `done` of `total` for `step`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogoProgress {
    /// What is being counted.
    pub step: LogoStep,
    /// Units done so far.
    pub done: usize,
    /// Units in all.
    pub total: usize,
}

/// What a logo upload or Remove left on the meter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogoWritten {
    /// The startup-screen duration from the session's SETTINGS read.
    pub startup_duration: StartupLogo,
    /// The startup screen is off, so the logo will not show. \[inferred\]
    pub startup_screen_off: bool,
}

/// What the logo area holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogoStatus {
    /// All 0xFF (erased flash). \[inferred\]
    Empty,
    /// A logo with a valid header.
    Logo {
        /// Width in pixels.
        width: u32,
        /// Height in pixels.
        height: u32,
        /// Pixel format.
        mode: LogoMode,
    },
    /// Neither empty nor a recognised logo header.
    Unrecognised,
}

/// Why a logo upload or Remove was refused before anything was sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogoRefusal {
    /// The session has not authenticated.
    NotAuthenticated,
    /// SETTINGS was not read successfully in this session.
    SettingsNotRead,
    /// The device model was not read in this session.
    ModelNotRead,
    /// The model is not a KM003C.
    WrongModel(String),
}

impl fmt::Display for LogoRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LogoRefusal::NotAuthenticated => f.write_str("the session has not authenticated"),
            LogoRefusal::SettingsNotRead => f.write_str("SETTINGS was not read in this session"),
            LogoRefusal::ModelNotRead => {
                f.write_str("the device model was not read in this session")
            }
            LogoRefusal::WrongModel(model) => {
                write!(f, "the meter is a {model}, logo upload needs a KM003C")
            }
        }
    }
}

/// The step of a logo Remove.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogoRemoveStep {
    /// The 16 bytes of 0xFF at 0x300000.
    Clear,
    /// The flag write of 0.
    Flag,
    /// The 16-byte read-back.
    ReadBack,
}

impl fmt::Display for LogoRemoveStep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            LogoRemoveStep::Clear => "clear",
            LogoRemoveStep::Flag => "flag",
            LogoRemoveStep::ReadBack => "read-back",
        })
    }
}

/// How long each chunk waits for FINISHED. \[static\]
const CHUNK_TIMEOUT: Duration = Duration::from_millis(1000);

/// Least gap between two read-back reads. \[inferred\]
const READ_GAP: Duration = Duration::from_millis(50);

/// Flag value written after a logo upload. \[static\]
const FLAG_LOGO: u32 = 2;

/// Flag value written by a logo Remove. \[static\]
const FLAG_NONE: u32 = 0;

/// Bytes of 0xFF a Remove writes at the region start. \[static\]
const REMOVE_LEN: usize = 16;

/// Plaintext logo chunk frame (type 0x4A) for `data` at flash `addr`:
/// main header `0x4A | id << 8 | 1 << 16 | (len + 16) << 22`, a 16-byte
/// block header (address, `len`, 0xFFFFFFFF, CRC-32 of those 12 bytes),
/// then `data` \[static\]. The transport encrypts it before sending.
pub fn logo_chunk_plaintext(id: u8, addr: u32, data: &[u8]) -> Vec<u8> {
    let len = data.len() as u32;
    let main = u32::from(LOGO_CHUNK_TYPE) | (u32::from(id) << 8) | (1 << 16) | ((len + 16) << 22);
    let mut header = Vec::with_capacity(16);
    header.extend(addr.to_le_bytes());
    header.extend(len.to_le_bytes());
    header.extend(0xFFFF_FFFFu32.to_le_bytes());
    let crc = crate::crypto::crc32(&header);
    header.extend(crc.to_le_bytes());
    let mut f = main.to_le_bytes().to_vec();
    f.extend(header);
    f.extend_from_slice(data);
    f
}

/// The logo flag write (SET_DATA 0x48, attribute 0x1C) of `value`: 2 after
/// an upload, 0 for a Remove.
///
/// Main header `0x48 | id << 8 | 3 << 22`, one extended section header
/// (`att` 0x1C, `size` 4), then `value` as a little-endian u32 \[static\].
pub fn logo_flag_request(id: u8, value: u32) -> Vec<u8> {
    let main = 0x48u32 | (u32::from(id) << 8) | (3 << 22);
    let ext = u32::from(LOGO_FLAG_ATT) | (4 << 22);
    let mut f = main.to_le_bytes().to_vec();
    f.extend(ext.to_le_bytes());
    f.extend(value.to_le_bytes());
    f
}

/// What 16 logo-area bytes say the area holds.
fn logo_status(head: &[u8]) -> LogoStatus {
    if !head.is_empty() && head.iter().all(|&b| b == 0xFF) {
        return LogoStatus::Empty;
    }
    match logo::parse_header(head) {
        Ok(h) => LogoStatus::Logo {
            width: h.width,
            height: h.height,
            mode: h.mode,
        },
        Err(_) => LogoStatus::Unrecognised,
    }
}

/// `Ok` when `reply` is FINISHED (or ACCEPT, if `accept_ok`), otherwise a
/// short reason for the error.
fn write_outcome(reply: &[u8], accept_ok: bool) -> Result<(), String> {
    match frame::classify_reply(reply) {
        Ok(Reply::Finished(_)) => Ok(()),
        Ok(Reply::Accept(_)) if accept_ok => Ok(()),
        Ok(Reply::Reject(_)) => Err("REJECT".into()),
        Ok(other) => Err(format!("unexpected reply {other:?}")),
        Err(e) => Err(e.to_string()),
    }
}

impl<T: Transport, C: Clock> Session<T, C> {
    /// Upload `blob`: every 256-byte chunk of the padded blob from 0x300000,
    /// then flag 2, then a full read-back compared byte for byte.
    ///
    /// Refused with [`Error::LogoRefused`] before anything is sent unless
    /// this session authenticated, read SETTINGS and read a KM003C model.
    /// The first failed chunk stops the upload ([`Error::LogoChunk`], no
    /// flag written); a failed flag is [`Error::LogoFlag`] and a read-back
    /// difference [`Error::LogoMismatch`]. Nothing is retried. `progress`
    /// sees each chunk, then each read-back read.
    pub fn logo_upload(
        &mut self,
        blob: &LogoBlob,
        progress: &mut dyn FnMut(LogoProgress),
    ) -> Result<LogoWritten, Error> {
        self.logo_preconditions()?;
        // Decode the duration before any write, so a written meter is never
        // reported as a failure afterwards.
        let written = self.logo_written()?;
        let padded = blob.padded();
        let total = padded.len() / CHUNK;
        for (k, data) in padded.chunks(CHUNK).enumerate() {
            let addr = LOGO_REGION_START + (k * CHUNK) as u32;
            self.logo_write_chunk(addr, data)
                .map_err(|reason| Error::LogoChunk { index: k, reason })?;
            progress(LogoProgress {
                step: LogoStep::Chunks,
                done: k + 1,
                total,
            });
        }
        self.logo_write_flag(FLAG_LOGO)
            .map_err(|reason| Error::LogoFlag { reason })?;
        let read = self.logo_read_back(padded.len(), progress)?;
        if let Some(offset) = read.iter().zip(&padded).position(|(a, b)| a != b) {
            return Err(Error::LogoMismatch { offset });
        }
        if read.len() != padded.len() {
            return Err(Error::LogoMismatch {
                offset: read.len().min(padded.len()),
            });
        }
        Ok(written)
    }

    /// Remove the logo: 16 bytes of 0xFF at 0x300000, flag 0, then a
    /// 16-byte read-back that must be all 0xFF.
    ///
    /// Refused like [`logo_upload`](Self::logo_upload); a failure names its
    /// step in [`Error::LogoRemove`]. Nothing is retried.
    pub fn logo_remove(
        &mut self,
        progress: &mut dyn FnMut(LogoProgress),
    ) -> Result<LogoWritten, Error> {
        let fail = |step, reason| Error::LogoRemove { step, reason };
        self.logo_preconditions()?;
        // Decode the duration before any write, so a written meter is never
        // reported as a failure afterwards.
        let written = self.logo_written()?;
        self.logo_write_chunk(LOGO_REGION_START, &[0xFF; REMOVE_LEN])
            .map_err(|r| fail(LogoRemoveStep::Clear, r))?;
        progress(LogoProgress {
            step: LogoStep::Chunks,
            done: 1,
            total: 1,
        });
        self.logo_write_flag(FLAG_NONE)
            .map_err(|r| fail(LogoRemoveStep::Flag, r))?;
        let read = self
            .logo_read_back(REMOVE_LEN, progress)
            .map_err(|e| fail(LogoRemoveStep::ReadBack, e.to_string()))?;
        if read.len() != REMOVE_LEN || read.iter().any(|&b| b != 0xFF) {
            return Err(fail(
                LogoRemoveStep::ReadBack,
                format!("expected {REMOVE_LEN} bytes of 0xFF, read {read:02X?}"),
            ));
        }
        Ok(written)
    }

    /// What the logo area holds (one 16-byte read; no write, no
    /// precondition beyond what [`logo_memory_read`](Self::logo_memory_read)
    /// needs).
    pub fn logo_read(&mut self) -> Result<LogoStatus, Error> {
        let head = self.logo_read_head()?;
        Ok(logo_status(&head))
    }

    /// The logo decoded to RGBA: a 16-byte header read, then a full read of
    /// the padded blob it describes. A logo area without a recognised header
    /// is an [`Error::Protocol`].
    pub fn logo_read_full(&mut self) -> Result<Rgba, Error> {
        let head = self.logo_read_head()?;
        let header = logo::parse_header(&head)
            .map_err(|_| Error::Protocol("the logo area holds no recognised logo".into()))?;
        let total = HEADER_LEN + header.byte_count as usize;
        self.sleep(READ_GAP);
        let data = self.logo_read_back(logo::padded_len(total), &mut |_| {})?;
        logo::decode(&data).map_err(|e| Error::Protocol(format!("logo decode: {e}")))
    }

    /// Refuse a logo write unless this session authenticated, read SETTINGS
    /// and read the model, and the model is a KM003C. Sends nothing, so a
    /// caller can check before asking the user to confirm.
    pub fn logo_preconditions(&self) -> Result<(), Error> {
        let refusal = if self.auth_level == 0 {
            Some(LogoRefusal::NotAuthenticated)
        } else if self.settings_read.is_none() {
            Some(LogoRefusal::SettingsNotRead)
        } else {
            match &self.model {
                None => Some(LogoRefusal::ModelNotRead),
                Some(m) if !m.trim().eq_ignore_ascii_case("KM003C") => {
                    Some(LogoRefusal::WrongModel(m.clone()))
                }
                Some(_) => None,
            }
        };
        refusal.map_or(Ok(()), |r| Err(Error::LogoRefused(r)))
    }

    /// Send one chunk and wait up to [`CHUNK_TIMEOUT`] for FINISHED;
    /// anything else is returned as the reason, with no retry.
    fn logo_write_chunk(&mut self, addr: u32, data: &[u8]) -> Result<(), String> {
        let plain = logo_chunk_plaintext(self.counter.next(), addr, data);
        let reply = self
            .transport
            .transact_logo_chunk(&plain, CHUNK_TIMEOUT)
            .map_err(|e| e.to_string())?;
        write_outcome(&reply, false)
    }

    /// Send the logo flag `value`; `Ok` on FINISHED or ACCEPT, otherwise
    /// the reason.
    fn logo_write_flag(&mut self, value: u32) -> Result<(), String> {
        let frame = logo_flag_request(self.counter.next(), value);
        let timeout = crate::transport::default_timeout(&frame);
        let reply = self
            .transport
            .transact_logo_flag(&frame, timeout)
            .map_err(|e| e.to_string())?;
        write_outcome(&reply, true)
    }

    /// The 16-byte header read at the logo window start.
    fn logo_read_head(&mut self) -> Result<Vec<u8>, Error> {
        self.logo_read_back(HEADER_LEN, &mut |_| {})
    }

    /// Read `len` bytes from the logo window start in reads of at most
    /// [`MEMORY_READ_MAX_LEN`] bytes, at least [`READ_GAP`] apart on the
    /// session clock.
    fn logo_read_back(
        &mut self,
        len: usize,
        progress: &mut dyn FnMut(LogoProgress),
    ) -> Result<Vec<u8>, Error> {
        let step = MEMORY_READ_MAX_LEN as usize;
        let total = len.div_ceil(step);
        let mut out = Vec::with_capacity(len);
        let mut last: Option<u64> = None;
        for k in 0..total {
            let off = k * step;
            let n = step.min(len - off);
            if let Some(at) = last {
                let due = at + READ_GAP.as_millis() as u64;
                let now = self.now_ms();
                if now < due {
                    self.sleep(Duration::from_millis(due - now));
                }
            }
            last = Some(self.now_ms());
            let data = self.logo_memory_read(LOGO_WINDOW_START + off as u32, n as u32)?;
            out.extend_from_slice(data.get(..n).unwrap_or(&data));
            progress(LogoProgress {
                step: LogoStep::ReadBack,
                done: k + 1,
                total,
            });
        }
        Ok(out)
    }

    /// The startup-screen duration from this session's SETTINGS read.
    fn logo_written(&self) -> Result<LogoWritten, Error> {
        let settings = self
            .settings_read
            .as_ref()
            .ok_or(Error::LogoRefused(LogoRefusal::SettingsNotRead))?;
        let startup_duration = settings.meter_settings()?.startup_logo.value;
        Ok(LogoWritten {
            startup_duration,
            startup_screen_off: startup_duration == StartupLogo::Off,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto;
    use crate::decode::settings::WritableField;
    use crate::logo;
    use crate::transport::guard::{check_logo_chunk, check_logo_flag_write};
    use crate::transport::mock::MockTransport;

    const BASE: u32 = 0x0030_0000;

    /// A 48×48 opaque test image.
    fn blob() -> LogoBlob {
        let (w, h) = (48u32, 48u32);
        let mut px = Vec::new();
        for y in 0..h {
            for x in 0..w {
                px.extend([(x * 5) as u8, (y * 3) as u8, ((x + y) * 2) as u8, 255]);
            }
        }
        logo::encode(&Rgba { w, h, px }, LogoMode::Opaque).unwrap()
    }

    // ---- AC-TZK5K4.1: chunk frames ----

    #[test]
    fn tzk5k4_chunk_plaintext_matches_the_documented_layout() {
        let padded = blob().padded();
        let data = &padded[..256];
        let f = logo_chunk_plaintext(1, BASE, data);
        // Main header 0x4A | id << 8 | 1 << 16 | (256 + 16) << 22.
        assert_eq!(&f[..4], &[0x4A, 0x01, 0x01, 0x44]);
        // Block header of the worked example: address, len, 0xFFFFFFFF, CRC.
        assert_eq!(
            &f[4..20],
            &[
                0x00, 0x00, 0x30, 0x00, 0x00, 0x01, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x6F,
                0xEA, 0x6B
            ]
        );
        assert_eq!(&f[20..], data);
        assert_eq!(check_logo_chunk(&f), Ok(()));

        let next = logo_chunk_plaintext(0x37, BASE + 256, &padded[256..512]);
        assert_eq!(&next[..4], &[0x4A, 0x37, 0x01, 0x44]);
        assert_eq!(
            &next[4..20],
            &[
                0x00, 0x01, 0x30, 0x00, 0x00, 0x01, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xC0, 0xB0,
                0x64, 0xAA
            ]
        );
        assert_eq!(check_logo_chunk(&next), Ok(()));
    }

    #[test]
    fn tzk5k4_remove_chunk_is_16_bytes_of_ff_at_the_region_start() {
        let f = logo_chunk_plaintext(1, BASE, &[0xFF; 16]);
        assert_eq!(&f[..4], &[0x4A, 0x01, 0x01, 0x08]);
        assert_eq!(
            &f[4..20],
            &[
                0x00, 0x00, 0x30, 0x00, 0x10, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0x9F, 0x55,
                0x26, 0xB1
            ]
        );
        assert_eq!(&f[20..], &[0xFF; 16]);
        assert_eq!(check_logo_chunk(&f), Ok(()));
        // The CRC covers the first 12 header bytes.
        assert_eq!(
            crypto::crc32(&f[4..16]).to_le_bytes(),
            [f[16], f[17], f[18], f[19]]
        );
    }

    // ---- AC-TZK5K4.2 / .3: flag frames ----

    #[test]
    fn tzk5k4_flag_request_matches_the_documented_example() {
        assert_eq!(
            logo_flag_request(1, 2),
            vec![
                0x48, 0x01, 0xC0, 0x00, 0x1C, 0x00, 0x00, 0x01, 0x02, 0x00, 0x00, 0x00
            ]
        );
        let off = logo_flag_request(0x2A, 0);
        assert_eq!(
            off,
            vec![
                0x48, 0x2A, 0xC0, 0x00, 0x1C, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00
            ]
        );
        assert_eq!(check_logo_flag_write(&off), Ok(()));
    }

    // ---- AC-TZK5K4.5: refused before anything is sent ----

    #[test]
    fn tzk5k4_upload_on_a_fresh_session_is_refused_without_sending() {
        let mut s = Session::open(MockTransport::new(vec![]));
        let mut reports = Vec::new();
        let r = s.logo_upload(&blob(), &mut |p| reports.push(p));
        assert_eq!(r, Err(Error::LogoRefused(LogoRefusal::NotAuthenticated)));
        assert!(s.transport().written().is_empty(), "nothing is sent");
        assert!(reports.is_empty(), "no progress for a refused upload");
    }

    #[test]
    fn tzk5k4_remove_on_a_fresh_session_is_refused_without_sending() {
        let mut s = Session::open(MockTransport::new(vec![]));
        let r = s.logo_remove(&mut |_| {});
        assert_eq!(r, Err(Error::LogoRefused(LogoRefusal::NotAuthenticated)));
        assert!(s.transport().written().is_empty(), "nothing is sent");
    }

    #[test]
    fn tzk5k4_refusals_name_the_missing_step_or_the_model() {
        let text = |r: LogoRefusal| Error::LogoRefused(r).to_string().to_lowercase();
        assert!(text(LogoRefusal::NotAuthenticated).contains("authenticat"));
        assert!(text(LogoRefusal::SettingsNotRead).contains("settings"));
        assert!(text(LogoRefusal::ModelNotRead).contains("model"));
        let wrong = text(LogoRefusal::WrongModel("KM002C".into()));
        assert!(
            wrong.contains("km002c") && wrong.contains("km003c"),
            "{wrong}"
        );
    }

    #[test]
    fn tzk5k4_chunk_and_remove_errors_name_the_chunk_and_step() {
        let chunk = Error::LogoChunk {
            index: 7,
            reason: "REJECT".into(),
        };
        assert!(chunk.to_string().contains("chunk 7"), "{chunk}");
        for (step, word) in [
            (LogoRemoveStep::Clear, "clear"),
            (LogoRemoveStep::Flag, "flag"),
            (LogoRemoveStep::ReadBack, "read"),
        ] {
            let e = Error::LogoRemove {
                step,
                reason: "x".into(),
            };
            assert!(e.to_string().to_lowercase().contains(word), "{e}");
        }
        let m = Error::LogoMismatch { offset: 0x123 };
        assert!(m.to_string().contains("0x123"), "{m}");
    }

    // ---- AC-TZK5K4.8: startup duration is a writable field ----

    #[test]
    fn tzk5k4_startup_duration_is_a_writable_field_on_key_0x06() {
        let f = WritableField::StartupDuration;
        assert_eq!(f.key(), 0x06);
        assert_eq!(f.range(), 0..=3);
        assert!(WritableField::ALL.contains(&f));
        assert_eq!(WritableField::from_name(f.name()), Some(f));
    }
}
