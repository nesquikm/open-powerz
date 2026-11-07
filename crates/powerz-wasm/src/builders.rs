//! Request builders (AC-QFTCWY.1): each returns the frame bytes the native
//! `powerz_protocol::session::Session` sends for the same inputs.

use powerz_protocol::decode::Rate;
use powerz_protocol::frame::{self, Attribute};
use powerz_protocol::session;
use wasm_bindgen::prelude::*;

use crate::BindError;

/// CONNECT: `02 id 00 00`.
#[wasm_bindgen(js_name = connectRequest)]
pub fn connect_request(id: u8) -> Vec<u8> {
    frame::connect(id).to_vec()
}

/// Auth request at `level` (1 = UID, 2 = OTP) over `block`, the UID or the
/// whole OTP block as read; the caller supplies the time and the random value.
#[wasm_bindgen(js_name = authRequest)]
pub fn auth_request(
    id: u8,
    level: u8,
    block: &[u8],
    now_ms: u64,
    random: u64,
) -> Result<Vec<u8>, BindError> {
    let credential = session::auth_credential(level, block)
        .ok_or_else(|| BindError::invalid(format!("unknown auth level {level} (1 or 2)")))?;
    let plaintext = session::auth_plaintext(now_ms, credential, random);
    Ok(session::auth_request(id, &plaintext))
}

/// GET_DATA with the attribute bit `mask`.
#[wasm_bindgen(js_name = getDataRequest)]
pub fn get_data_request(id: u8, mask: u16) -> Vec<u8> {
    frame::get_data(id, Attribute(mask)).to_vec()
}

/// Start the ADC queue at rate code `rate_code` (0..3).
#[wasm_bindgen(js_name = startQueueRequest)]
pub fn start_queue_request(id: u8, rate_code: u8) -> Result<Vec<u8>, BindError> {
    let rate = Rate::from_code(rate_code)
        .ok_or_else(|| BindError::invalid(format!("unknown rate code {rate_code} (0..3)")))?;
    Ok(frame::start_queue(id, rate.code()).to_vec())
}

/// Stop the ADC queue.
#[wasm_bindgen(js_name = stopQueueRequest)]
pub fn stop_queue_request(id: u8) -> Vec<u8> {
    frame::stop_queue(id).to_vec()
}

/// Start the PD sniffer (analyzer mode 1).
#[wasm_bindgen(js_name = startPdSnifferRequest)]
pub fn start_pd_sniffer_request(id: u8) -> Vec<u8> {
    frame::start_pd_sniffer(id).to_vec()
}

/// Stop the PD sniffer.
#[wasm_bindgen(js_name = stopPdSnifferRequest)]
pub fn stop_pd_sniffer_request(id: u8) -> Vec<u8> {
    frame::stop_analyzer(id).to_vec()
}

/// One PD sniffer poll: GET_DATA PD_PACKET, plus ADC when `with_adc`.
#[wasm_bindgen(js_name = pdPollRequest)]
pub fn pd_poll_request(id: u8, with_adc: bool) -> Vec<u8> {
    frame::get_data(id, session::pd_poll_mask(with_adc)).to_vec()
}

/// Memory read of one of the four known blocks; any other `(addr, len)` is
/// a `forbidden` error, as in the native session.
#[wasm_bindgen(js_name = memoryReadRequest)]
pub fn memory_read_request(id: u8, addr: u32, len: u32) -> Result<Vec<u8>, BindError> {
    if !session::is_memory_read_allowed(addr, len) {
        return Err(BindError::new(
            "forbidden",
            format!("memory read of {len} bytes at {addr:#x} is outside the safe blocks"),
        ));
    }
    Ok(session::memory_read_request(id, addr, len))
}
