//! Memory read (0x44) and the device-info blocks built on it.

use super::{Clock, Error, Session};
use crate::crypto;
use crate::decode::{DeviceInfoBlock, OtpBlock, Uid};
use crate::transport::{self, Transport};

/// Hardware / boot-loader info block.
const HARDWARE_INFO: (u32, u32) = (0x0000_0420, 64);
/// Application firmware info block.
const FIRMWARE_INFO: (u32, u32) = (0x0000_4420, 64);
/// OTP block (serial number; its first 16 bytes are the level-2 credential).
pub(super) const OTP: (u32, u32) = (0x0300_0C00, 64);
/// MCU unique ID (the level-1 credential).
pub(super) const UID: (u32, u32) = (0x4001_0450, 12);

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
}
