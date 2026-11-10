//! The meter's serial port read as the desktop app reads it (FR X3H3JY): the
//! CDC demux tells ADCNEW stream frames, PDM text lines and replies apart, and
//! this reader hands out only the whole binary replies.

use powerz_protocol::adcnew::{CdcDemux, CdcItem};
use wasm_bindgen::prelude::*;

/// Whole binary replies off the CDC byte stream; stream frames, text lines and
/// noise are dropped.
#[wasm_bindgen]
#[derive(Debug, Default)]
pub struct CdcReplyReader {
    demux: CdcDemux,
}

#[wasm_bindgen]
impl CdcReplyReader {
    /// An empty reader.
    #[wasm_bindgen(constructor)]
    pub fn new() -> CdcReplyReader {
        CdcReplyReader::default()
    }

    /// Append bytes read from the port.
    pub fn push(&mut self, bytes: &[u8]) {
        self.demux.push(bytes);
    }

    /// The next whole reply, or `undefined` until more bytes arrive.
    #[wasm_bindgen(js_name = nextReply)]
    pub fn next_reply(&mut self) -> Option<Vec<u8>> {
        loop {
            match self.demux.next_item()? {
                Ok(CdcItem::Reply(bytes)) => return Some(bytes),
                Ok(CdcItem::Frame(_) | CdcItem::Line(_)) | Err(_) => {}
            }
        }
    }
}
