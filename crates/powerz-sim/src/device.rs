//! Request handling of [`SimDevice`]: one request in, zero or more reply
//! transfers queued.

use powerz_protocol::crypto::{self, KEYS};
use powerz_protocol::decode::records::MEMORY_READ_MAX_LEN;
use powerz_protocol::decode::settings::apply_settings_write;
use powerz_protocol::frame::{self, Attribute, DataHeader, SectionHeader};

use powerz_protocol::transport::guard::check_settings_write;

use crate::memory;
use crate::queue::RECORD_LEN;
use crate::records;
use crate::{Persona, Sample, SimDevice};

/// Message type codes the simulator understands.
pub(crate) mod msg {
    pub const CONNECT: u8 = 0x02;
    pub const DISCONNECT: u8 = 0x03;
    pub const ACCEPT: u8 = 0x05;
    pub const REJECT: u8 = 0x06;
    pub const FINISHED: u8 = 0x07;
    pub const GET_DATA: u8 = 0x0C;
    pub const START_QUEUE: u8 = 0x0E;
    pub const STOP_QUEUE: u8 = 0x0F;
    pub const START_ANALYZER: u8 = 0x10;
    pub const STOP_ANALYZER: u8 = 0x11;
    pub const HEAD: u8 = 0x40;
    pub const PUT_DATA: u8 = 0x41;
    pub const MEM_READ: u8 = 0x44;
    pub const SETTINGS_WRITE: u8 = 0x0A;
    pub const SET_DATA: u8 = 0x48;
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
pub(crate) fn adc_payload(s: &Sample, rate_code: u8) -> Vec<u8> {
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
pub(crate) const SETTINGS_LEN: usize = 180;

/// The captured FW 2.0.6 SETTINGS block [device] (no serial or UID in it):
/// English, brightness 50 %, rotation 90°, voltage/current/offset
/// calibration words as read. The sim starts from it, applies settings
/// writes and rewrites the rate bits and CRCs.
pub(crate) const SETTINGS_206: [u8; SETTINGS_LEN] = [
    0x91, 0x01, 0x50, 0xF8, 0x00, 0x00, 0x00, 0x00, 0x10, 0x27, 0x41, 0xFF, 0x00, 0x00, 0x00, 0x00,
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x5E, 0x02, 0x00, 0x00,
    0x5E, 0x02, 0x00, 0x00, 0x5E, 0x02, 0x00, 0x00, 0x5E, 0x02, 0x00, 0x00, 0x5E, 0x02, 0x00, 0x00,
    0x3C, 0x35, 0x0F, 0x00, 0x3C, 0x35, 0x0F, 0x00, 0x3C, 0x35, 0x0F, 0x00, 0x3C, 0x35, 0x0F, 0x00,
    0x3C, 0x35, 0x0F, 0x00, 0x3C, 0x35, 0x0F, 0x00, 0x3C, 0x35, 0x0F, 0x00, 0x3C, 0x35, 0x0F, 0x00,
    0x3C, 0x35, 0x0F, 0x00, 0x3C, 0x35, 0x0F, 0x00, 0x50, 0x00, 0x00, 0x00, 0xAE, 0x58, 0x10, 0xB9,
    0x49, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x50, 0x4F, 0x57, 0x45, 0x52, 0x2D, 0x5A, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0xE9, 0x3D, 0x2E, 0x39,
];

/// The FW 2.0.7 SETTINGS block as read after a replug [device] (no serial or
/// UID in it): the meter saved it with an earlier boot time at 0x10, so the
/// CRC at 0x5C matches neither the time now shown there nor 0xFFFFFFFF.
pub(crate) const SETTINGS_207_REBOOTED: [u8; SETTINGS_LEN] = [
    0x91, 0x01, 0x08, 0xF8, 0x00, 0x00, 0x00, 0x00, 0x10, 0x27, 0x41, 0xFF, 0x00, 0x00, 0x00, 0x00,
    0xA6, 0x9B, 0xE5, 0x6A, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x5E, 0x02, 0x00, 0x00,
    0x5E, 0x02, 0x00, 0x00, 0x5E, 0x02, 0x00, 0x00, 0x5E, 0x02, 0x00, 0x00, 0x5E, 0x02, 0x00, 0x00,
    0x3C, 0x35, 0x0F, 0x00, 0x3C, 0x35, 0x0F, 0x00, 0x3C, 0x35, 0x0F, 0x00, 0x3C, 0x35, 0x0F, 0x00,
    0x3C, 0x35, 0x0F, 0x00, 0x3C, 0x35, 0x0F, 0x00, 0x3C, 0x35, 0x0F, 0x00, 0x3C, 0x35, 0x0F, 0x00,
    0x3C, 0x35, 0x0F, 0x00, 0x3C, 0x35, 0x0F, 0x00, 0x50, 0x00, 0x00, 0x00, 0x7F, 0xFB, 0x69, 0x0E,
    0x49, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x50, 0x4F, 0x57, 0x45, 0x52, 0x2D, 0x5A, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0xE9, 0x3D, 0x2E, 0x39,
];

/// Encode the 180-byte SETTINGS payload from `block` with
/// `rate_code` in bits 2..3 of the block B flags word (0x60), then refresh
/// the CRC-32s of block A (0x00..0x5C, at 0x5C) and block B (0x60..0xB0,
/// at 0xB0).
fn settings_payload(block: &[u8; SETTINGS_LEN], rate_code: u8) -> Vec<u8> {
    let mut p = block.to_vec();
    let mut flags = u32::from_le_bytes([p[0x60], p[0x61], p[0x62], p[0x63]]);
    flags = (flags & !(0b11 << 2)) | (u32::from(rate_code & 0b11) << 2);
    p[0x60..0x64].copy_from_slice(&flags.to_le_bytes());
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
    pub(crate) fn control(&self, ty: u8, id: u8) -> Vec<u8> {
        self.header(ty, id, false, 0).to_vec()
    }

    /// Handle one request; return the reply transfers in order.
    pub(crate) fn handle(&mut self, req: &[u8]) -> Vec<Vec<u8>> {
        let Some(&b0) = req.first() else {
            return Vec::new();
        };
        let id = req.get(1).copied().unwrap_or(0);
        if self.mode == powerz_protocol::firmware::DeviceMode::Bootloader {
            return self.bootloader_request(req, id);
        }
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
            msg::SET_DATA => vec![self.set_data(req, id)],
            // Over bulk the KM003C neither answers nor applies a settings
            // write (2026-10-29) `[device]`.
            msg::SETTINGS_WRITE if self.persona != Persona::Cdc => Vec::new(),
            msg::SETTINGS_WRITE => vec![self.settings_write(req, id)],
            _ => vec![self.control(msg::REJECT, id)],
        }
    }

    /// HEAD: after CONNECT, `40 id 00 01`, the old-firmware u32 at offset
    /// 4, zero padding to 20 bytes; REJECT before CONNECT `[inferred]`.
    pub(crate) fn head(&self, id: u8) -> Vec<u8> {
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
        let measured = self.measured();
        self.queue.fill(measured, now);
        let mut sections: Vec<(Attribute, u8, Vec<u8>)> = Vec::new();
        if mask & Attribute::ADC.bits() != 0 {
            let sample = measured.sample(now);
            sections.push((Attribute::ADC, 0, adc_payload(&sample, self.queue.code())));
        }
        if mask & Attribute::ADC_QUEUE.bits() != 0 {
            let records = self.queue.take();
            let n = records.len() as u8;
            sections.push((Attribute::ADC_QUEUE, n, records.concat()));
        }
        if mask & Attribute::PD_PACKET.bits() != 0 {
            let s = measured.sample(now);
            let mv = |v: f64| (v * 1000.0).round().clamp(0.0, 65535.0) as u16;
            let ma = (s.ibus_a * 1000.0).round().clamp(-32768.0, 32767.0) as i16;
            let fallback = (mv(s.vbus_v), ma, mv(s.cc1_v), mv(s.cc2_v));
            sections.push((Attribute::PD_PACKET, 0, self.pd.chunk(now, fallback)));
        }
        if mask & Attribute::SETTINGS.bits() != 0 {
            sections.push((Attribute::SETTINGS, 0, {
                let stored = self.stored();
                if self.faults.settings_saved_before_reboot {
                    SETTINGS_207_REBOOTED.to_vec()
                } else {
                    settings_payload(&stored.settings, stored.rate_code)
                }
            }));
        }
        // RecordList: served from level 1; at level 0 the reply is empty.
        if mask & Attribute::RECORD_LIST.bits() != 0 && self.auth_level >= 1 {
            sections.push((
                Attribute::RECORD_LIST,
                0,
                records::list_payload(&self.record_table),
            ));
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
                let code = self.rate_code();
                self.queue.set_rate(code, now);
            }
            // Analyzer start: mode 1 is the PD sniffer.
            msg::START_ANALYZER if code == PD_MODE => self.pd.start(now),
            msg::START_ANALYZER => self.pd.stop(),
            msg::STOP_ANALYZER => self.pd.stop(),
            _ => {}
        }
        self.control(msg::ACCEPT, id)
    }

    /// 0x44: confirmation, then the block encrypted with key 0 in one
    /// transfer. Like the meter `[device]`, a read longer than
    /// [`MEMORY_READ_MAX_LEN`] bytes gets its confirmation and only the
    /// first 256 bytes, then nothing. Its replies carry level 0 in the
    /// header whatever the session level (`C4 id 01 01`, `06 id 00 00`).
    fn memory_read(&mut self, req: &[u8], id: u8) -> Vec<Vec<u8>> {
        let level0 = |ty: u8, encode: bool, obj: u16| {
            DataHeader {
                ty: ty & 0x7F,
                extend: ty & 0x80 != 0,
                id,
                encode,
                permi: 0,
                obj,
            }
            .to_header()
            .encode()
            .to_vec()
        };
        let reject = || vec![level0(msg::REJECT, false, 0)];
        if !self.link_open() || req.len() < 4 + CRYPT_LEN || self.info_read_fails() {
            return reject();
        }
        let plain = crypto::decrypt(&KEYS[0], &req[4..4 + CRYPT_LEN]);
        let word = |off: usize| u32::from_le_bytes(plain[off..off + 4].try_into().unwrap());
        let (addr, len) = (word(0), word(4));
        if word(12) != crypto::crc32(&plain[..12]) {
            return reject();
        }
        // Record samples: only inside a listed window, from level 1.
        if self.faults.reject_record_reads && records::in_record_area(addr) {
            return reject();
        }
        let record = (self.auth_level >= 1)
            .then(|| records::samples(&self.record_table, addr, len))
            .flatten();
        let Some(block) = memory::block(addr, len, &self.app_version, &self.serial).or(record)
        else {
            return reject();
        };
        let mut confirm = level0(msg::MEM_READ | 0x80, true, 4);
        confirm.extend_from_slice(&plain[..16]);
        let mut data = crypto::encrypt(&KEYS[0], &block);
        data.truncate(MEMORY_READ_MAX_LEN as usize);
        vec![confirm, data]
    }

    /// 0x48: only the record-table write (attribute 0x28) is taken; it
    /// replaces the table (kept as is under the `ignore-table-write`
    /// fault) and is answered with FINISHED, as the KM003C does `[device]`.
    /// Every other SET_DATA is REJECTed and changes
    /// nothing.
    fn set_data(&mut self, req: &[u8], id: u8) -> Vec<u8> {
        if !self.link_open() {
            return self.control(msg::REJECT, id);
        }
        let Some(table) = records::table_write(req) else {
            return self.control(msg::REJECT, id);
        };
        if !self.faults.ignore_table_write {
            self.record_table = table;
        }
        self.control(msg::FINISHED, id)
    }

    /// 0x0A on CDC: a settings write laid out as `check_settings_write` demands
    /// (only the documented keys, in order, values in range) is answered with
    /// FINISHED, as the KM003C does `[device]`, and applied to the SETTINGS
    /// block (kept as is under the `ignore-settings-write` fault) the way the
    /// meter stores it (`apply_settings_write`: key 0x15 sets the rate code,
    /// 0x14 the bits 0..1 of 0x60 `[device]`). Every other 0x0A frame, such as
    /// one carrying the factory-reset key, is REJECTed and changes nothing.
    fn settings_write(&mut self, req: &[u8], id: u8) -> Vec<u8> {
        if !self.link_open() || check_settings_write(req).is_err() {
            return self.control(msg::REJECT, id);
        }
        if self.faults.ignore_settings_write {
            return self.control(msg::FINISHED, id);
        }
        let mut stored = self.stored();
        let old_rate = stored.rate_code;
        apply_settings_write(&mut stored.settings, req);
        if self.faults.stir_undecoded_settings {
            stored.settings[0x08] ^= 0x01;
        }
        let w60 = u32::from_le_bytes([
            stored.settings[0x60],
            stored.settings[0x61],
            stored.settings[0x62],
            stored.settings[0x63],
        ]);
        stored.rate_code = ((w60 >> 2) & 0b11) as u8;
        let rate = (stored.rate_code != old_rate).then_some(stored.rate_code);
        drop(stored);
        if let Some(code) = rate
            && !self.streaming
        {
            let now = self.uptime_ms();
            self.queue.set_rate(code, now);
        }
        self.control(msg::FINISHED, id)
    }

    /// 0x4C: grant level 1 for the UID, 2 for the OTP credential, else 0.
    /// The reply always has id 0 and echoes the plaintext.
    fn auth(&mut self, req: &[u8], id: u8) -> Vec<u8> {
        if !self.link_open() || req.len() < 4 + CRYPT_LEN {
            return self.control(msg::REJECT, id);
        }
        let plain = crypto::decrypt(&KEYS[3], &req[4..4 + CRYPT_LEN]);
        let otp = memory::otp_credential(&self.serial);
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
