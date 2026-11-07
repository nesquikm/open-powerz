//! Pure CDC framing helpers, shared by the CDC transport and the PDM
//! reader; no serial port, so they build without the `cdc` feature.

use super::Error;
use crate::frame::{DeclaredEnd, HEADER_LEN, MainHeader, declared_end, frame_type, msg};

/// Length of the first complete frame at the start of `buf`, walking the
/// section headers of a PUT_DATA reply; `None` while more bytes are needed.
///
/// Control replies are 4 bytes. A PUT_DATA header with `obj == 0` carries no
/// sections. The 0x44 / 0x4C replies are framed by `obj` (payload words). A
/// PD_STATUS section has no size and takes whatever has arrived.
pub fn frame_complete(buf: &[u8]) -> Option<usize> {
    let header = MainHeader::decode(buf).ok()?.data();
    match header.ty {
        msg::PUT_DATA if header.obj == 0 => Some(HEADER_LEN),
        msg::PUT_DATA => match declared_end(buf) {
            DeclaredEnd::Complete(end) => (buf.len() >= end).then_some(end),
            DeclaredEnd::MissingHeader(_) => None,
            DeclaredEnd::PdStatus { .. } => Some(buf.len()),
        },
        msg::MEM_READ_CONFIRM | msg::AUTH_REPLY => {
            let len = HEADER_LEN + 4 * usize::from(header.obj);
            (buf.len() >= len).then_some(len)
        }
        _ => Some(HEADER_LEN),
    }
}

/// Refuse frame types 0x01..0x03 (they control the ADCNEW stream on CDC).
pub fn check_cdc(request: &[u8]) -> Result<(), Error> {
    match frame_type(request) {
        Some(ty) if (0x01..=0x03).contains(&ty) => Err(Error::UnsupportedOnCdc { ty }),
        _ => Ok(()),
    }
}
