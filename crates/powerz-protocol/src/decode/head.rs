//! HEAD (0x40): the read-only old-firmware check sent after CONNECT
//! (FR V9SK52, AC .3).

use super::{DecodeError, need};

/// HEAD message type `[static]`.
const HEAD: u8 = 0x40;
/// Shortest HEAD reply: main header plus the u32 field `[inferred]`.
const MIN_LEN: usize = 8;

/// HEAD request with id `id`.
///
/// `40 id 00 01` followed by 16 zero bytes `[static]`.
pub fn request(id: u8) -> [u8; 20] {
    let mut r = [0u8; 20];
    r[..4].copy_from_slice(&[HEAD, id, 0x00, 0x01]);
    r
}

/// A decoded HEAD reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeadReply {
    field: u32,
}

impl HeadReply {
    /// Decode a HEAD reply frame.
    ///
    /// The reply is type 0x40 (extend bit ignored) with the old-firmware
    /// field as a little-endian u32 at offset 4 `[static]`.
    pub fn decode(frame: &[u8]) -> Result<HeadReply, DecodeError> {
        const WHAT: &str = "HEAD reply";
        need(WHAT, frame, MIN_LEN)?;
        let ty = frame[0] & 0x7F;
        if ty != HEAD {
            return Err(DecodeError::UnexpectedType {
                what: WHAT,
                expected: HEAD,
                got: ty,
            });
        }
        let field = u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
        Ok(HeadReply { field })
    }

    /// The old-firmware field.
    pub fn old_firmware_field(&self) -> u32 {
        self.field
    }

    /// Whether the meter asks for a firmware update.
    ///
    /// Any non-zero field means "update required" `[static]`.
    pub fn update_required(&self) -> bool {
        self.field != 0
    }
}
