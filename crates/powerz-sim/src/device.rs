//! Request handling of [`SimDevice`]: one request in, zero or more reply
//! transfers queued.

use powerz_protocol::crypto::{self, KEYS};
use powerz_protocol::frame::{self, Attribute, DataHeader, SectionHeader};

use crate::memory;
use crate::queue::RECORD_LEN;
use crate::{Persona, Sample, SimDevice};

/// Message type codes the simulator understands.
pub(crate) mod msg {
    pub const CONNECT: u8 = 0x02;
    pub const DISCONNECT: u8 = 0x03;
    pub const ACCEPT: u8 = 0x05;
    pub const REJECT: u8 = 0x06;
    pub const GET_DATA: u8 = 0x0C;
    pub const START_QUEUE: u8 = 0x0E;
    pub const STOP_QUEUE: u8 = 0x0F;
    pub const START_ANALYZER: u8 = 0x10;
    pub const STOP_ANALYZER: u8 = 0x11;
    pub const HEAD: u8 = 0x40;
    pub const PUT_DATA: u8 = 0x41;
    pub const MEM_READ: u8 = 0x44;
    pub const AUTH: u8 = 0x4C;
}

/// The stale DISCONNECT that waits on the IN pipe right after open.
pub(crate) const STALE_DISCONNECT: [u8; 4] = [msg::DISCONNECT, 0x00, 0x00, 0x00];

/// Analyzer mode of the PD sniffer in the 0x10 start request.
const PD_MODE: u8 = 1;

/// Length of an ADC section payload.
const ADC_LEN: usize = 44;
/// Encrypted payload length of 0x44 and 0x4C requests.
const CRYPT_LEN: usize = 32;

/// Key-3 variant the meter encrypts its auth reply with: byte 1 is `X`.
fn auth_reply_key() -> [u8; 16] {
    let mut k = KEYS[3];
    k[1] = b'X';
    k
}

/// `obj` field of a PUT_DATA header: `floor((total - 12) / 4)` in 10 bits.
pub(crate) fn put_data_obj(total_len: usize) -> u16 {
    ((total_len as i64 - 12).div_euclid(4) as u16) & 0x3FF
}

/// Encode one ADC section payload from `s`.
fn adc_payload(s: &Sample, rate_code: u8) -> Vec<u8> {
    let micro = |v: f64| ((v * 1e6).round() as i32).to_le_bytes();
    let tenth_mv = |v: f64| ((v * 10_000.0).round().clamp(0.0, 65535.0) as u16).to_le_bytes();
    let mut p = vec![0u8; ADC_LEN];
    for (off, v) in [
        (0x00, s.vbus_v),
        (0x04, s.ibus_a),
        (0x08, s.vbus_v),
        (0x0C, s.ibus_a),
        (0x10, s.vbus_v),
        (0x14, s.ibus_a),
    ] {
        p[off..off + 4].copy_from_slice(&micro(v));
    }
    p[0x18..0x1A].copy_from_slice(&((s.temp_c * 128.0).round() as i16).to_le_bytes());
    for (off, v) in [
        (0x1A, s.cc1_v),
        (0x1C, s.cc2_v),
        (0x1E, s.dp_v),
        (0x20, s.dm_v),
        (0x22, 3.3),
    ] {
        p[off..off + 2].copy_from_slice(&tenth_mv(v));
    }
    p[0x26] = rate_code;
    p
}

/// Length of a SETTINGS section payload.
const SETTINGS_LEN: usize = 180;

/// Encode the 180-byte SETTINGS payload: block A (0x00..0x5C) with its
/// CRC-32 at 0x5C, block B (0x60..0xB0: flags with the rate code in bits
/// 2..3, name `POWER-Z` at 0x70) with its CRC-32 at 0xB0.
fn settings_payload(rate_code: u8) -> Vec<u8> {
    let mut p = vec![0u8; SETTINGS_LEN];
    let flags = u32::from(rate_code & 0b11) << 2;
    p[0x60..0x64].copy_from_slice(&flags.to_le_bytes());
    p[0x70..0x70 + 7].copy_from_slice(b"POWER-Z");
    let crc_a = crypto::crc32(&p[0x00..0x5C]);
    p[0x5C..0x60].copy_from_slice(&crc_a.to_le_bytes());
    let crc_b = crypto::crc32(&p[0x60..0xB0]);
    p[0xB0..0xB4].copy_from_slice(&crc_b.to_le_bytes());
    p
}

impl SimDevice {
    /// Main header for a reply of type `ty` with id `id` and the current
    /// auth level in bits 17..18.
    fn header(&self, ty: u8, id: u8, encode: bool, obj: u16) -> [u8; 4] {
        DataHeader {
            ty: ty & 0x7F,
            extend: ty & 0x80 != 0,
            id,
            encode,
            permi: self.auth_level,
            obj,
        }
        .to_header()
        .encode()
    }

    /// Whether memory read and auth are allowed: after CONNECT, or always
    /// on CDC (where 0x02 is ADCNEW, so there is no CONNECT).
    fn link_open(&self) -> bool {
        self.connected || self.persona == Persona::Cdc
    }

    /// Four-byte control reply.
    fn control(&self, ty: u8, id: u8) -> Vec<u8> {
        self.header(ty, id, false, 0).to_vec()
    }

    /// Handle one request; return the reply transfers in order.
    pub(crate) fn handle(&mut self, req: &[u8]) -> Vec<Vec<u8>> {
        let Some(&b0) = req.first() else {
            return Vec::new();
        };
        let id = req.get(1).copied().unwrap_or(0);
        if self.persona == Persona::Cdc && matches!(b0 & 0x7F, 0x01..=0x03) {
            // ADCNEW framing on CDC: out of scope, no reply.
            return Vec::new();
        }
        match b0 & 0x7F {
            msg::CONNECT => {
                self.connected = true;
                vec![self.control(msg::ACCEPT, id)]
            }
            msg::DISCONNECT => {
                self.connected = false;
                vec![self.control(msg::ACCEPT, id)]
            }
            msg::GET_DATA => vec![self.get_data(req, id)],
            ty
            @ (msg::START_QUEUE | msg::STOP_QUEUE | msg::START_ANALYZER | msg::STOP_ANALYZER) => {
                let code = req.get(2).copied().unwrap_or(0) >> 1;
                vec![self.queue_control(ty, id, code)]
            }
            msg::HEAD => vec![self.head(id)],
            msg::MEM_READ => self.memory_read(req, id),
            msg::AUTH => vec![self.auth(req, id)],
            _ => vec![self.control(msg::REJECT, id)],
        }
    }

    /// HEAD: after CONNECT, `40 id 00 01`, the old-firmware u32 at offset
    /// 4, zero padding to 20 bytes; REJECT before CONNECT `[inferred]`.
    fn head(&self, id: u8) -> Vec<u8> {
        if !self.link_open() {
            return self.control(msg::REJECT, id);
        }
        let mut r = vec![msg::HEAD, id, 0x00, 0x01];
        r.extend(self.head_field.to_le_bytes());
        r.extend([0u8; 12]);
        r
    }

    /// GET_DATA: one PUT_DATA frame with a section per known attribute.
    fn get_data(&mut self, req: &[u8], id: u8) -> Vec<u8> {
        let mask = frame::get_data_mask(req).map_or(0, Attribute::bits);
        let now = self.uptime_ms();
        self.queue.fill(self.scenario, now);
        let mut sections: Vec<(Attribute, u8, Vec<u8>)> = Vec::new();
        if mask & Attribute::ADC.bits() != 0 {
            let sample = self.scenario.sample(now);
            sections.push((Attribute::ADC, 0, adc_payload(&sample, self.queue.code())));
        }
        if mask & Attribute::ADC_QUEUE.bits() != 0 {
            let records = self.queue.take();
            let n = records.len() as u8;
            sections.push((Attribute::ADC_QUEUE, n, records.concat()));
        }
        if mask & Attribute::PD_PACKET.bits() != 0 {
            let s = self.scenario.sample(now);
            let mv = |v: f64| (v * 1000.0).round().clamp(0.0, 65535.0) as u16;
            let ma = (s.ibus_a * 1000.0).round().clamp(-32768.0, 32767.0) as i16;
            let fallback = (mv(s.vbus_v), ma, mv(s.cc1_v), mv(s.cc2_v));
            sections.push((Attribute::PD_PACKET, 0, self.pd.chunk(now, fallback)));
        }
        if mask & Attribute::SETTINGS.bits() != 0 {
            sections.push((Attribute::SETTINGS, 0, settings_payload(self.rate_code)));
        }
        let mut body = Vec::new();
        let n = sections.len();
        for (i, (att, chunk, payload)) in sections.into_iter().enumerate() {
            let size = if att == Attribute::ADC_QUEUE {
                RECORD_LEN
            } else {
                payload.len()
            };
            let h = SectionHeader {
                att: att.bits(),
                next: i + 1 < n,
                chunk,
                size: size as u16,
            };
            body.extend(h.encode());
            body.extend(payload);
        }
        let obj = if body.is_empty() {
            0
        } else {
            put_data_obj(body.len() + 4)
        };
        let mut frame = self.header(msg::PUT_DATA, id, false, obj).to_vec();
        frame.extend(body);
        frame
    }

    /// 0x0E..0x11: REJECT below level 1; 0x0E while streaming is REJECTed.
    /// 0x10 mode 1 starts the PD sniffer, 0x11 stops it.
    fn queue_control(&mut self, ty: u8, id: u8, code: u8) -> Vec<u8> {
        if self.auth_level < 1 || (ty == msg::START_QUEUE && self.streaming) {
            return self.control(msg::REJECT, id);
        }
        let now = self.uptime_ms();
        match ty {
            msg::START_QUEUE => {
                self.streaming = true;
                self.queue.set_rate(code, now);
            }
            msg::STOP_QUEUE => {
                self.streaming = false;
                self.queue.set_rate(self.rate_code, now);
            }
            // Analyzer start: mode 1 is the PD sniffer.
            msg::START_ANALYZER if code == PD_MODE => self.pd.start(now),
            msg::START_ANALYZER => self.pd.stop(),
            msg::STOP_ANALYZER => self.pd.stop(),
            _ => {}
        }
        self.control(msg::ACCEPT, id)
    }

    /// 0x44: confirmation, then the block encrypted with key 0.
    fn memory_read(&mut self, req: &[u8], id: u8) -> Vec<Vec<u8>> {
        let reject = || vec![self.control(msg::REJECT, id)];
        if !self.link_open() || req.len() < 4 + CRYPT_LEN {
            return reject();
        }
        let plain = crypto::decrypt(&KEYS[0], &req[4..4 + CRYPT_LEN]);
        let word = |off: usize| u32::from_le_bytes(plain[off..off + 4].try_into().unwrap());
        let (addr, len) = (word(0), word(4));
        if word(12) != crypto::crc32(&plain[..12]) {
            return reject();
        }
        let Some(block) = memory::block(addr, len, &self.app_version) else {
            return reject();
        };
        let mut confirm = self.header(msg::MEM_READ | 0x80, id, true, 4).to_vec();
        confirm.extend_from_slice(&plain[..16]);
        vec![confirm, crypto::encrypt(&KEYS[0], &block)]
    }

    /// 0x4C: grant level 1 for the UID, 2 for the OTP credential, else 0.
    /// The reply always has id 0 and echoes the plaintext.
    fn auth(&mut self, req: &[u8], id: u8) -> Vec<u8> {
        if !self.link_open() || req.len() < 4 + CRYPT_LEN {
            return self.control(msg::REJECT, id);
        }
        let plain = crypto::decrypt(&KEYS[3], &req[4..4 + CRYPT_LEN]);
        let otp = memory::otp_credential();
        let granted = if plain[8..8 + memory::SIM_UID.len()] == memory::SIM_UID {
            1
        } else if plain[8..8 + otp.len()] == otp[..] {
            2
        } else {
            0
        };
        if granted > 0 {
            self.auth_level = granted;
        }
        let mut reply = DataHeader {
            ty: msg::AUTH,
            extend: false,
            id: 0,
            encode: true,
            permi: granted,
            obj: (CRYPT_LEN / 4) as u16,
        }
        .to_header()
        .encode()
        .to_vec();
        reply.extend(crypto::encrypt(&auth_reply_key(), &plain));
        reply
    }
}

/// Whether `req` is a GET_DATA that polls the ADC queue.
pub(crate) fn is_queue_poll(req: &[u8]) -> bool {
    frame::get_data_mask(req).is_some_and(|m| m.bits() & Attribute::ADC_QUEUE.bits() != 0)
}
