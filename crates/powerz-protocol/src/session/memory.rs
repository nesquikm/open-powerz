//! Memory read (0x44) and the device-info blocks built on it.

use super::{Clock, Error, Session};
use crate::crypto;
use std::time::Duration;

use crate::decode::head::{self, HeadReply};
use crate::decode::{DeviceInfoBlock, OtpBlock, Uid};
use crate::firmware::version::{self, FirmwareWarning};
use crate::transport::{self, Transport};

/// Hardware / boot-loader info block.
const HARDWARE_INFO: (u32, u32) = (0x0000_0420, 64);
/// Application firmware info block.
const FIRMWARE_INFO: (u32, u32) = (0x0000_4420, 64);
/// OTP block (serial number; its first 16 bytes are the level-2 credential).
pub(super) const OTP: (u32, u32) = (0x0300_0C00, 64);
/// MCU unique ID (the level-1 credential).
pub(crate) const UID: (u32, u32) = (0x4001_0450, 12);

/// Most bytes of a raw HEAD reply shown in a problem text.
const HEAD_HEX_MAX: usize = 32;

/// Pause between CONNECT and HEAD `[static]`.
const HEAD_DELAY: Duration = Duration::from_millis(50);

/// The only memory blocks [`Session::memory_read`] may read.
const MEMORY_READ_ALLOWED: [(u32, u32); 4] = [HARDWARE_INFO, FIRMWARE_INFO, OTP, UID];

/// Length of the plaintext confirmation frame: header + first 16 bytes of
/// the request plaintext.
const CONFIRMATION_LEN: usize = 20;

/// Model, versions and identity of the meter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    /// Model string (e.g. `KM003C`).
    pub model: String,
    /// Application firmware version (block 0x4420).
    pub firmware_version: String,
    /// Hardware / boot-loader version (block 0x420).
    pub hardware_version: String,
    /// Serial number from the OTP block.
    pub serial: String,
    /// MCU unique ID as lowercase hex.
    pub uid: String,
}

/// Both firmware info blocks and the HEAD old-firmware flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirmwareStatus {
    /// Bootloader info block (0x420), if not blank.
    pub bootloader: Option<DeviceInfoBlock>,
    /// Application info block (0x4420), if not blank.
    pub application: Option<DeviceInfoBlock>,
    /// Whether the HEAD reply asked for a firmware update: `Some(false)`
    /// for a field of 0; `None` when HEAD could not be read or its field was
    /// not recognised. Never `Some(true)` while the meaning of a non-zero
    /// field is unconfirmed on the device `[inferred]`.
    pub update_required: Option<bool>,
    /// What could not be read, one entry per failed step (a block, HEAD or
    /// the recovery CONNECT after one), in the order they failed.
    pub problems: Vec<FirmwareProblem>,
}

/// Which step of [`Session::read_firmware_status_or_unknown`] failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FirmwareProblemKind {
    /// The bootloader info block (0x420).
    BootloaderBlock,
    /// The application info block (0x4420).
    ApplicationBlock,
    /// The HEAD old-firmware check.
    Head,
    /// The recovery CONNECT after a failed step; the remaining steps were
    /// skipped.
    Recovery,
}

/// One step of the firmware status that could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirmwareProblem {
    /// The step that failed.
    pub kind: FirmwareProblemKind,
    /// One line saying what failed and why.
    pub text: String,
}

impl FirmwareProblem {
    fn new(kind: FirmwareProblemKind, text: String) -> Self {
        Self { kind, text }
    }
}

impl std::fmt::Display for FirmwareProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text)
    }
}

impl FirmwareStatus {
    /// The status when it could not be read: no blocks, an unknown update
    /// flag and no problems.
    pub fn unknown() -> Self {
        Self {
            bootloader: None,
            application: None,
            update_required: None,
            problems: Vec::new(),
        }
    }

    /// Firmware warnings for this status.
    pub fn warnings(&self) -> Vec<FirmwareWarning> {
        let app = self.application.as_ref().map_or("", |b| b.version());
        version::warnings(app, self.update_required == Some(true))
    }
}

/// Plaintext of a memory-read request: `addr, len, 0xFFFFFFFF,
/// crc32(first 12 bytes)`, padded with 0xFF to 32 bytes.
pub fn memory_read_plaintext(addr: u32, len: u32) -> [u8; 32] {
    let mut p = [0xFF; 32];
    p[0..4].copy_from_slice(&addr.to_le_bytes());
    p[4..8].copy_from_slice(&len.to_le_bytes());
    let crc = crypto::crc32(&p[..12]);
    p[12..16].copy_from_slice(&crc.to_le_bytes());
    p
}

/// Whole memory-read frame: `44 id 01 01` + AES(key 0) of the plaintext.
pub fn memory_read_request(id: u8, addr: u32, len: u32) -> Vec<u8> {
    let mut f = vec![0x44, id, 0x01, 0x01];
    f.extend(crypto::encrypt(
        &crypto::KEYS[0],
        &memory_read_plaintext(addr, len),
    ));
    f
}

impl<T: Transport, C: Clock> Session<T, C> {
    /// Read one of the four known memory blocks.
    ///
    /// Any other `(addr, len)` fails with [`Error::Forbidden`] before
    /// anything is sent.
    pub fn memory_read(&mut self, addr: u32, len: u32) -> Result<Vec<u8>, Error> {
        if !MEMORY_READ_ALLOWED.contains(&(addr, len)) {
            return Err(Error::Forbidden { addr, len });
        }
        let request = memory_read_request(self.counter.next(), addr, len);
        let reply = self.exchange(&request)?;
        let plaintext = memory_read_plaintext(addr, len);
        // FW 2.0.6 REJECTs memory reads until CONNECT has been sent.
        if reply.first().map(|b| b & 0x7F) == Some(0x06) {
            return Err(Error::Rejected {
                request_type: request[0],
            });
        }
        if reply.len() != CONFIRMATION_LEN || reply[0] != 0xC4 || reply[4..] != plaintext[..16] {
            return Err(Error::Protocol(format!(
                "bad memory-read confirmation for {len} bytes at {addr:#x}"
            )));
        }
        let want = (len as usize).div_ceil(crypto::BLOCK_LEN) * crypto::BLOCK_LEN;
        let timeout = transport::default_timeout(&request);
        let mut data = Vec::with_capacity(want);
        while data.len() < want {
            data.extend(self.transport.read(timeout)?);
        }
        if data.len() != want {
            return Err(Error::Protocol(format!(
                "memory read returned {} bytes, expected {want}",
                data.len()
            )));
        }
        let mut block = crypto::decrypt(&crypto::KEYS[0], &data);
        block.truncate(len as usize);
        Ok(block)
    }

    /// Read one of the known blocks given as `(addr, len)`.
    pub(super) fn read_block(&mut self, (addr, len): (u32, u32)) -> Result<Vec<u8>, Error> {
        self.memory_read(addr, len)
    }

    /// Model, versions, serial and UID; works at auth level 0.
    pub fn device_info(&mut self) -> Result<DeviceInfo, Error> {
        let hardware = DeviceInfoBlock::decode(&self.read_block(HARDWARE_INFO)?)?;
        let firmware = DeviceInfoBlock::decode(&self.read_block(FIRMWARE_INFO)?)?;
        let otp = OtpBlock::decode(&self.read_block(OTP)?)?;
        let uid = Uid::decode(&self.read_block(UID)?)?;
        let model = firmware
            .as_ref()
            .or(hardware.as_ref())
            .map(|b| b.model().to_owned())
            .ok_or_else(|| Error::Protocol("both device-info blocks are empty".into()))?;
        let version = |b: &Option<DeviceInfoBlock>| {
            b.as_ref()
                .map(|b| b.version().to_owned())
                .unwrap_or_default()
        };
        Ok(DeviceInfo {
            model,
            firmware_version: version(&firmware),
            hardware_version: version(&hardware),
            serial: otp.serial(),
            uid: uid.to_string(),
        })
    }

    /// Both info blocks and the HEAD old-firmware flag; read-only.
    ///
    /// Sends CONNECT, then reads the bootloader (0x420) and application
    /// (0x4420) info blocks, waits 50 ms on the session clock and sends
    /// HEAD (as the vendor app does `[static]`).
    pub fn firmware_status(&mut self) -> Result<FirmwareStatus, Error> {
        self.connect()?;
        self.read_firmware_status()
    }

    /// [`firmware_status`](Self::firmware_status) on a session that is
    /// already connected: reads both info blocks, waits 50 ms, then sends
    /// HEAD. Any failure is returned; a caller that sees an error here still
    /// has a connected session and may carry on without the firmware status.
    /// A non-zero HEAD field leaves `update_required` as `None` with a
    /// [`FirmwareProblemKind::Head`] problem.
    pub fn read_firmware_status(&mut self) -> Result<FirmwareStatus, Error> {
        let bootloader = DeviceInfoBlock::decode(&self.read_block(HARDWARE_INFO)?)?;
        let application = DeviceInfoBlock::decode(&self.read_block(FIRMWARE_INFO)?)?;
        let reply = self.head_exchange()?;
        let field = HeadReply::decode(&reply)?.old_firmware_field();
        let mut problems = Vec::new();
        let update_required = head_flag(field, &reply, &mut problems);
        Ok(FirmwareStatus {
            bootloader,
            application,
            update_required,
            problems,
        })
    }

    /// After a failed step: record `problem` and send one recovery CONNECT.
    /// Gives `false` (with a [`FirmwareProblemKind::Recovery`] problem)
    /// when that CONNECT failed and the remaining steps must be skipped;
    /// a link-fatal error is returned.
    fn record_and_recover(
        &mut self,
        problem: FirmwareProblem,
        problems: &mut Vec<FirmwareProblem>,
    ) -> Result<bool, Error> {
        problems.push(problem);
        match self.connect() {
            Ok(()) => Ok(true),
            Err(e) if e.is_link_fatal() => Err(e),
            Err(e) => {
                problems.push(FirmwareProblem::new(
                    FirmwareProblemKind::Recovery,
                    format!("recovery CONNECT failed: {e}"),
                ));
                Ok(false)
            }
        }
    }

    /// Read one info block for [`read_firmware_status_or_unknown`](Self::read_firmware_status_or_unknown):
    /// a non-link-fatal failure records a `kind` problem naming `name` and
    /// sends one recovery CONNECT. The flag is `false` when that CONNECT
    /// failed.
    fn info_block_or_problem(
        &mut self,
        block: (u32, u32),
        kind: FirmwareProblemKind,
        name: &str,
        problems: &mut Vec<FirmwareProblem>,
    ) -> Result<(Option<DeviceInfoBlock>, bool), Error> {
        let read = self
            .read_block(block)
            .and_then(|raw| Ok(DeviceInfoBlock::decode(&raw)?));
        match read {
            Ok(b) => Ok((b, true)),
            Err(e) if e.is_link_fatal() => Err(e),
            Err(e) => {
                let text = format!("{name} info block ({:#x}) not read: {e}", block.0);
                let go_on = self.record_and_recover(FirmwareProblem::new(kind, text), problems)?;
                Ok((None, go_on))
            }
        }
    }

    /// The HEAD step for [`read_firmware_status_or_unknown`](Self::read_firmware_status_or_unknown):
    /// a timeout, a REJECT or an undecodable reply records a problem
    /// (with up to 32 bytes of the reply's hex, when there is one), sends
    /// one recovery CONNECT and gives `None`. A decoded reply with a
    /// non-zero field records an unrecognised-reply problem without a
    /// recovery CONNECT (the pipe is in step) and gives `None`.
    fn head_or_problem(
        &mut self,
        problems: &mut Vec<FirmwareProblem>,
    ) -> Result<Option<bool>, Error> {
        let text = match self.head_exchange() {
            Ok(reply) => match head_reply_field(&reply) {
                Ok(field) => return Ok(head_flag(field, &reply, problems)),
                Err(reason) => format!("HEAD not read: {reason} (reply: {})", capped_hex(&reply)),
            },
            Err(e) if e.is_link_fatal() => return Err(e),
            Err(e) => format!("HEAD not read: {e}"),
        };
        self.record_and_recover(
            FirmwareProblem::new(FirmwareProblemKind::Head, text),
            problems,
        )?;
        Ok(None)
    }

    /// [`read_firmware_status`](Self::read_firmware_status) that only costs
    /// what failed: the bootloader block (0x420), the application block
    /// (0x4420) and HEAD are read in that order, and a step that timed out
    /// or did not decode is recorded in [`FirmwareStatus::problems`] and
    /// followed by one CONNECT (which skips a late reply and recovers a
    /// stuck pipe) before the next step runs. A failed HEAD leaves
    /// `update_required` as `None`. A recovery CONNECT that is rejected or
    /// times out is recorded as a [`FirmwareProblemKind::Recovery`] problem
    /// and the remaining steps are skipped. A [link-fatal](Error::is_link_fatal)
    /// error is returned.
    pub fn read_firmware_status_or_unknown(&mut self) -> Result<FirmwareStatus, Error> {
        use FirmwareProblemKind::{ApplicationBlock, BootloaderBlock};
        let mut st = FirmwareStatus::unknown();
        let (bootloader, go_on) = self.info_block_or_problem(
            HARDWARE_INFO,
            BootloaderBlock,
            "bootloader",
            &mut st.problems,
        )?;
        st.bootloader = bootloader;
        if !go_on {
            return Ok(st);
        }
        let (application, go_on) = self.info_block_or_problem(
            FIRMWARE_INFO,
            ApplicationBlock,
            "application",
            &mut st.problems,
        )?;
        st.application = application;
        if !go_on {
            return Ok(st);
        }
        st.update_required = self.head_or_problem(&mut st.problems)?;
        Ok(st)
    }

    /// The HEAD step that follows CONNECT: wait [`HEAD_DELAY`], send one
    /// HEAD (0x40) and return the raw reply, undecoded.
    ///
    /// In application mode the meter answers HEAD with type 0x40 and id 0
    /// `[device]`, so a type-0x40 id-0 first transfer is taken as the answer;
    /// anything else must carry the request id as usual.
    pub(super) fn head_exchange(&mut self) -> Result<Vec<u8>, Error> {
        self.sleep(HEAD_DELAY);
        let request = head::request(self.counter.next());
        let timeout = transport::default_timeout(&request);
        self.exchange_taking_first(&request, timeout, is_id_0_head)
    }
}

/// `reply` as spaced hex, cut after [`HEAD_HEX_MAX`] bytes with an ellipsis.
fn capped_hex(reply: &[u8]) -> String {
    if reply.len() > HEAD_HEX_MAX {
        format!("{}\u{2026}", head::hex(&reply[..HEAD_HEX_MAX], " "))
    } else {
        head::hex(reply, " ")
    }
}

/// Whether `reply` is a HEAD reply (type 0x40, extend bit ignored) with
/// id 0, as the meter sends in application mode `[device]`.
fn is_id_0_head(reply: &[u8]) -> bool {
    matches!(reply, [ty, 0, ..] if ty & 0x7F == 0x40)
}

/// The old-firmware field of a raw HEAD reply, or why it does not give one:
/// a REJECT from the meter or a reply that does not decode.
fn head_reply_field(reply: &[u8]) -> Result<u32, String> {
    if head::is_reject(reply) {
        return Err("the meter sent REJECT".into());
    }
    HeadReply::decode(reply)
        .map(|h| h.old_firmware_field())
        .map_err(|e| e.to_string())
}

/// The update flag for HEAD field `field`: `Some(false)` for 0 (current);
/// any other value is unrecognised until confirmed on the device, so it
/// records a [`FirmwareProblemKind::Head`] problem with the field and the
/// raw `reply`, and gives `None` (never "update required").
fn head_flag(field: u32, reply: &[u8], problems: &mut Vec<FirmwareProblem>) -> Option<bool> {
    if field == 0 {
        return Some(false);
    }
    problems.push(FirmwareProblem::new(
        FirmwareProblemKind::Head,
        format!(
            "HEAD reply not recognised: field {field:#010x} (reply: {})",
            capped_hex(reply)
        ),
    ));
    None
}
