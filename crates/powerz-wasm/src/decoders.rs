//! Reply decoders (AC-QFTCWY.2): each takes the reply bytes the page read
//! and gives what the native session makes of them. Malformed or short
//! input is a [`BindError`] with a kind, never a panic.
//!
//! The Rust functions return serde values; the `*_js` exports hand the same
//! values to JS through `serde-wasm-bindgen`.

use powerz_protocol::crypto;
use powerz_protocol::decode::pd_packet::{CaptureClock, PdChunk, PdRecord, PdRecordKind, PdStatus};
use powerz_protocol::decode::{AdcSnapshot, DecodeError, QueueRecord, Rate};
use powerz_protocol::frame::{self, Attribute, FrameError, Reply, Section};
use powerz_protocol::pd::{self, Decoded, PowerRole};
use powerz_protocol::pd_details::{FieldNode, detail_bytes, field_nodes};
use powerz_protocol::pdcap;
use powerz_protocol::session::{self, DeviceInfo};
use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::BindError;

/// Length of one ADC queue record.
const QUEUE_RECORD_LEN: usize = 20;
/// Length of the plaintext memory-read confirmation frame.
const MEMORY_CONFIRMATION_LEN: usize = 20;

pub(crate) fn malformed(message: impl Into<String>) -> BindError {
    BindError::new("malformed", message)
}

impl From<FrameError> for BindError {
    fn from(e: FrameError) -> BindError {
        malformed(e.to_string())
    }
}

impl From<DecodeError> for BindError {
    fn from(e: DecodeError) -> BindError {
        malformed(e.to_string())
    }
}

pub(crate) fn to_js<T: Serialize>(value: &T) -> Result<JsValue, BindError> {
    serde_wasm_bindgen::to_value(value).map_err(|e| BindError::new("internal", e.to_string()))
}

/// A REJECT reply as a `rejected` error, other replies as `malformed`.
fn unexpected(reply: Reply, what: &str) -> BindError {
    match reply {
        Reply::Reject(_) => BindError::new("rejected", format!("the meter rejected {what}")),
        other => malformed(format!("expected {what}, got {other:?}")),
    }
}

/// The sections of a PUT_DATA reply; a bare header has none.
pub(crate) fn put_data_sections<'a>(
    reply: &'a [u8],
    what: &str,
) -> Result<Vec<Section<'a>>, BindError> {
    match frame::classify_reply(reply)? {
        Reply::PutData(_) => Ok(frame::parse_put_data(reply)?),
        other => Err(unexpected(other, what)),
    }
}

// ---- control replies ----

/// Type code of a DISCONNECT frame.
const DISCONNECT: u8 = 0x03;

/// `Ok` when `reply` is an ACCEPT; a REJECT is a `rejected` error. Stale
/// 4-byte DISCONNECT frames ahead of the answer (left on the IN pipe after
/// open, skipped by the native session) are skipped.
#[wasm_bindgen(js_name = decodeAccept)]
pub fn decode_accept(reply: &[u8]) -> Result<(), BindError> {
    let mut reply = reply;
    while reply.len() > 4 && reply[0] & 0x7F == DISCONNECT {
        reply = &reply[4..];
    }
    match frame::classify_reply(reply)? {
        Reply::Accept(_) => Ok(()),
        other => Err(unexpected(other, "an ACCEPT")),
    }
}

// ---- auth ----

/// The level an auth reply grants, for a request built from `block` (the
/// UID or OTP block as read), `now_ms` and `random`. A wrong echo or a
/// granted level of 0 is an `auth_failed` error.
#[wasm_bindgen(js_name = decodeAuthReply)]
pub fn decode_auth_reply(
    reply: &[u8],
    block: &[u8],
    now_ms: u64,
    random: u64,
) -> Result<u8, BindError> {
    if let r @ Reply::Reject(_) = frame::classify_reply(reply)? {
        return Err(unexpected(r, "the auth request"));
    }
    let plaintext = session::auth_plaintext(now_ms, block, random);
    session::auth_reply_level(reply, &plaintext, block.len())
        .ok_or_else(|| BindError::new("auth_failed", "the meter did not grant the auth level"))
}

// ---- ADC snapshot ----

/// One ADC snapshot, in volts, amps, watts and °C.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Adc {
    /// VBUS, V.
    pub vbus_v: f64,
    /// IBUS, A, signed.
    pub ibus_a: f64,
    /// Averaged VBUS, V.
    pub vbus_avg_v: f64,
    /// Averaged IBUS, A.
    pub ibus_avg_a: f64,
    /// Power, W.
    pub power_w: f64,
    /// CC1, V.
    pub cc1_v: f64,
    /// CC2, V.
    pub cc2_v: f64,
    /// D+, V.
    pub dp_v: f64,
    /// D−, V.
    pub dm_v: f64,
    /// Temperature, °C, when the meter reports one.
    pub temp_c: Option<f64>,
}

impl From<&AdcSnapshot> for Adc {
    fn from(a: &AdcSnapshot) -> Adc {
        Adc {
            vbus_v: a.vbus_v(),
            ibus_a: a.ibus_a(),
            vbus_avg_v: a.vbus_avg_v(),
            ibus_avg_a: a.ibus_avg_a(),
            power_w: a.power_w(),
            cc1_v: a.cc1_v(),
            cc2_v: a.cc2_v(),
            dp_v: a.dp_v(),
            dm_v: a.dm_v(),
            temp_c: a.temp_c(),
        }
    }
}

fn adc_of(section: &Section<'_>) -> Result<Adc, BindError> {
    Ok(Adc::from(&AdcSnapshot::decode(section.payload)?))
}

/// The ADC snapshot of a PUT_DATA reply (alone or chained with others).
pub fn decode_adc(reply: &[u8]) -> Result<Adc, BindError> {
    put_data_sections(reply, "ADC data")?
        .iter()
        .find(|s| s.att == Attribute::ADC)
        .ok_or_else(|| BindError::new("missing", "the reply has no ADC section"))
        .and_then(adc_of)
}

/// [`decode_adc`] for JS.
#[wasm_bindgen(js_name = decodeAdc)]
pub fn decode_adc_js(reply: &[u8]) -> Result<JsValue, BindError> {
    to_js(&decode_adc(reply)?)
}

// ---- ADC queue ----

/// One ADC queue sample.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct QueueSample {
    /// Meter time, ms.
    pub time_ms: u32,
    /// VBUS, V.
    pub vbus_v: f64,
    /// IBUS, A, signed.
    pub ibus_a: f64,
    /// CC1, V.
    pub cc1_v: f64,
    /// CC2, V.
    pub cc2_v: f64,
    /// D+, V.
    pub dp_v: f64,
    /// D−, V.
    pub dm_v: f64,
}

impl From<&QueueRecord> for QueueSample {
    fn from(r: &QueueRecord) -> QueueSample {
        QueueSample {
            time_ms: r.time_ms(),
            vbus_v: r.vbus_v(),
            ibus_a: r.ibus_a(),
            cc1_v: r.cc1_v(),
            cc2_v: r.cc2_v(),
            dp_v: r.dp_v(),
            dm_v: r.dm_v(),
        }
    }
}

/// The ADC queue records of a PUT_DATA reply captured at rate code
/// `rate_code` (0..3), in the order they came; a partial last record is
/// dropped.
pub fn decode_queue(reply: &[u8], rate_code: u8) -> Result<Vec<QueueSample>, BindError> {
    let rate = Rate::from_code(rate_code)
        .ok_or_else(|| BindError::invalid(format!("unknown rate code {rate_code} (0..3)")))?;
    let mut samples = Vec::new();
    // A bare PUT_DATA header (any auth level) or a DISCONNECT carries no
    // samples, and a last queue section shorter than it declares keeps its
    // whole records and drops the partial one, as the native `get_data` does
    // [device].
    let sections = match frame::classify_reply(reply)? {
        Reply::PutData(_) if reply.len() == frame::HEADER_LEN => Vec::new(),
        Reply::PutData(_) => frame::parse_put_data_cut_queue(reply)?,
        Reply::Disconnect(_) => Vec::new(),
        other => return Err(unexpected(other, "ADC queue data")),
    };
    for section in sections {
        if section.att != Attribute::ADC_QUEUE {
            continue;
        }
        for chunk in section.payload.as_chunks::<QUEUE_RECORD_LEN>().0 {
            samples.push(QueueSample::from(&QueueRecord::decode(chunk, rate)?));
        }
    }
    Ok(samples)
}

/// [`decode_queue`] for JS.
#[wasm_bindgen(js_name = decodeQueue)]
pub fn decode_queue_js(reply: &[u8], rate_code: u8) -> Result<JsValue, BindError> {
    to_js(&decode_queue(reply, rate_code)?)
}

// ---- PD poll ----

/// Kind of a PD table row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RowKind {
    /// A PD message.
    Message,
    /// An event (attach, detach, reset, ...).
    Event,
}

/// Who sent a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// The source port.
    SourceToSink,
    /// The sink port.
    SinkToSource,
    /// Cable traffic (SOP', SOP'').
    Cable,
}

/// One PD status block, as the desktop chart's status sample.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct PdStatusSample {
    /// Meter time, ms.
    pub session_ms: u64,
    /// VBUS, V.
    pub vbus_v: f64,
    /// IBUS, A, signed.
    pub ibus_a: f64,
    /// CC1, V.
    pub cc1_v: f64,
    /// CC2, V.
    pub cc2_v: f64,
}

impl From<PdStatus> for PdStatusSample {
    fn from(s: PdStatus) -> PdStatusSample {
        PdStatusSample {
            session_ms: u64::from(s.time_ms),
            vbus_v: f64::from(s.vbus_mv) / 1000.0,
            ibus_a: f64::from(s.ibus_ma) / 1000.0,
            cc1_v: f64::from(s.cc1_mv) / 1000.0,
            cc2_v: f64::from(s.cc2_mv) / 1000.0,
        }
    }
}

/// One row of the PD packet table, in the desktop table's shape.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PdRow {
    /// Record index from 0, in capture order.
    pub index: u64,
    /// Ms since the capture start.
    pub elapsed_ms: u64,
    /// The record's meter time on the capture's time base, ms.
    pub session_ms: u64,
    /// Message or event.
    pub kind: RowKind,
    /// SOP name, messages only.
    pub sop: Option<String>,
    /// Message or event name.
    pub name: String,
    /// MessageID, messages only.
    pub message_id: Option<u8>,
    /// Who sent it, messages only.
    pub direction: Option<Direction>,
    /// Number of Data Objects, messages only.
    pub objects: Option<u8>,
    /// Specification Revision, messages only.
    pub spec_revision: Option<String>,
    /// VBUS at the record's time, V.
    pub vbus_v: f64,
    /// IBUS at the record's time, A.
    pub ibus_a: f64,
    /// One-line summary.
    pub summary: String,
}

/// What one PD poll reply holds.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PdPollResult {
    /// Status blocks of the PD_PACKET sections.
    pub status: Vec<PdStatusSample>,
    /// Rows of the records, in the order they came.
    pub rows: Vec<PdRow>,
    /// Records skipped because they ran past their chunk.
    pub skipped: usize,
    /// ADC snapshot, when the poll asked for one.
    pub adc: Option<Adc>,
}

/// The details of one record, in the desktop details' shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PdDetails {
    /// Record index.
    pub index: u64,
    /// The PD message bytes (header little-endian, then the body); the
    /// record bytes for an event.
    pub raw: Vec<u8>,
    /// The decoded field tree.
    pub fields: Vec<FieldNode>,
}

/// What a clear answers, in the desktop clear's shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PdCleared {
    /// Index of the next record; rows numbered below it are stale.
    pub next_index: u64,
    /// Time (ms) of the latest status block the capture had, 0 if none.
    pub status_floor_ms: u64,
}

/// A saved capture: the pdcap file's text and its number of records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PdcapFile {
    /// The pdcap file (JSON Lines).
    pub text: String,
    /// Records in the file.
    pub rows: usize,
}

/// One captured record, kept for its details.
#[derive(Debug, Clone)]
struct Entry {
    raw: Vec<u8>,
    decoded: Option<Decoded>,
}

/// A PD sniffer capture: decodes poll replies into table rows, keeping the
/// PD decoder context, the capture clock and the row numbering across polls,
/// and every record since the last clear for its details and the pdcap file.
#[wasm_bindgen]
#[derive(Debug, Default)]
pub struct PdDecoder {
    capture: pd::Capture,
    clock: CaptureClock,
    next_index: u64,
    /// Meter time minus elapsed time of the capture's first record.
    session_base: Option<i64>,
    /// Index of `entries[0]`.
    base: u64,
    entries: Vec<Entry>,
    /// Status blocks and records since the last clear, in capture order.
    log: Vec<pdcap::Entry>,
    /// Time of the latest status block (0 if none).
    last_status_ms: u64,
}

#[wasm_bindgen]
impl PdDecoder {
    /// A fresh capture.
    #[wasm_bindgen(constructor)]
    pub fn new() -> PdDecoder {
        PdDecoder::default()
    }

    /// [`PdDecoder::decode`] for JS.
    #[wasm_bindgen(js_name = decode)]
    pub fn decode_js(&mut self, reply: &[u8]) -> Result<JsValue, BindError> {
        to_js(&self.decode(reply)?)
    }

    /// [`PdDecoder::details`] for JS: `undefined` for a record not kept.
    #[wasm_bindgen(js_name = details)]
    pub fn details_js(&self, index: u64) -> Result<JsValue, BindError> {
        match self.details(index) {
            Some(d) => to_js(&d),
            None => Ok(JsValue::UNDEFINED),
        }
    }

    /// [`PdDecoder::clear`] for JS.
    #[wasm_bindgen(js_name = clear)]
    pub fn clear_js(&mut self) -> Result<JsValue, BindError> {
        to_js(&self.clear())
    }

    /// The newest cable identity the capture holds since the last clear, in
    /// the trigger panel's shape (FR AXHK2P, FR EAKFEC); `null` for none.
    #[wasm_bindgen(js_name = latestCable)]
    pub fn latest_cable_js(&self) -> Result<JsValue, BindError> {
        match self.latest_cable() {
            Some(c) => to_js(&powerz_trigger::cable_view(&c)),
            None => Ok(JsValue::NULL),
        }
    }

    /// [`PdDecoder::pdcap`] for JS.
    #[wasm_bindgen(js_name = pdcap)]
    pub fn pdcap_js(&self, model: String, firmware_version: String) -> Result<JsValue, BindError> {
        to_js(&self.pdcap(model, firmware_version)?)
    }
}

impl PdDecoder {
    /// Decode one PD poll reply (PD_PACKET, optionally with ADC).
    pub fn decode(&mut self, reply: &[u8]) -> Result<PdPollResult, BindError> {
        let mut out = PdPollResult {
            status: Vec::new(),
            rows: Vec::new(),
            skipped: 0,
            adc: None,
        };
        let sections = match frame::classify_reply(reply)? {
            Reply::PutData(_) if reply.len() == frame::HEADER_LEN => Vec::new(),
            Reply::PutData(_) => frame::parse_put_data(reply)?,
            Reply::Disconnect(_) => Vec::new(),
            other => return Err(unexpected(other, "PD data")),
        };
        for section in &sections {
            if section.att == Attribute::PD_PACKET {
                // A section too short for its status block is counted like a
                // bad record, as in the native session.
                let Ok(chunk) = PdChunk::parse(section.payload) else {
                    out.skipped += 1;
                    continue;
                };
                let status = PdStatusSample::from(chunk.status);
                out.status.push(status);
                self.last_status_ms = self.last_status_ms.max(status.session_ms);
                self.log.push(pdcap::Entry::Status {
                    session_ms: status.session_ms,
                    status: chunk.status,
                });
                out.skipped += chunk.skipped;
                for record in &chunk.records {
                    let row = self.row(record, status.vbus_v, status.ibus_a);
                    self.log.push(pdcap::Entry::Record {
                        session_ms: row.session_ms,
                        elapsed_ms: row.elapsed_ms,
                        record: record.clone(),
                    });
                    out.rows.push(row);
                }
            } else if section.att == Attribute::ADC {
                out.adc = Some(adc_of(section)?);
            }
        }
        Ok(out)
    }

    /// The raw bytes and field tree of record `index`, if the capture keeps it.
    pub fn details(&self, index: u64) -> Option<PdDetails> {
        let entry = self
            .entries
            .get(usize::try_from(index.checked_sub(self.base)?).ok()?)?;
        let fields = entry
            .decoded
            .as_ref()
            .map(|d| field_nodes(d, entry.raw.len()))
            .unwrap_or_default();
        Some(PdDetails {
            index,
            raw: entry.raw.clone(),
            fields,
        })
    }

    /// Drop every record and status block; numbering goes on and the PD
    /// decoder keeps its context.
    pub fn clear(&mut self) -> PdCleared {
        let cleared = PdCleared {
            next_index: self.next_index,
            status_floor_ms: self.last_status_ms,
        };
        self.base = self.next_index;
        self.entries.clear();
        self.log.clear();
        cleared
    }

    /// The newest cable Discover Identity ACK since the last clear, as the
    /// desktop capture finds it.
    pub fn latest_cable(&self) -> Option<powerz_protocol::pdm::message::CableIdentity> {
        self.log.iter().rev().find_map(|entry| {
            let pdcap::Entry::Record { record, .. } = entry else {
                return None;
            };
            let (PdRecordKind::Message(m) | PdRecordKind::BigMessage(m)) = &record.kind else {
                return None;
            };
            powerz_protocol::pdm::message::cable_identity_from_message(m.sop, m.header, &m.body)
        })
    }

    /// The capture since the last clear as a pdcap file, with this crate's
    /// version (the app's: one workspace version) and the meter's model and
    /// firmware (no serial number).
    pub fn pdcap(&self, model: String, firmware_version: String) -> Result<PdcapFile, BindError> {
        let io = |e: std::io::Error| BindError::new("internal", e.to_string());
        let header = pdcap::Header::new(env!("CARGO_PKG_VERSION"), model, firmware_version);
        let mut writer = pdcap::Writer::new(Vec::new(), &header).map_err(io)?;
        for entry in &self.log {
            writer.write(entry).map_err(io)?;
        }
        let bytes = writer.finish().map_err(io)?;
        let text =
            String::from_utf8(bytes).map_err(|e| BindError::new("internal", e.to_string()))?;
        let rows = self
            .log
            .iter()
            .filter(|e| matches!(e, pdcap::Entry::Record { .. }))
            .count();
        Ok(PdcapFile { text, rows })
    }

    /// Decode `record` into the next row.
    fn row(&mut self, record: &PdRecord, vbus_v: f64, ibus_a: f64) -> PdRow {
        let index = self.next_index;
        self.next_index += 1;
        let elapsed_ms = self.clock.elapsed_ms(record.time_ms);
        let elapsed = i64::try_from(elapsed_ms).unwrap_or(i64::MAX);
        let base = *self
            .session_base
            .get_or_insert_with(|| i64::from(record.time_ms) - elapsed);
        let session_ms = u64::try_from(base.saturating_add(elapsed)).unwrap_or(0);
        let decoded = record.pd_input().and_then(|i| self.capture.feed(i));
        match (&record.kind, decoded) {
            (PdRecordKind::Message(_) | PdRecordKind::BigMessage(_), Some(d)) => {
                let summary = d.summary();
                let h = d.header;
                self.entries.push(Entry {
                    raw: detail_bytes(record),
                    decoded: Some(d),
                });
                let direction = if h.sop.is_cable() {
                    Direction::Cable
                } else if h.port_power_role == Some(PowerRole::Source) {
                    Direction::SourceToSink
                } else {
                    Direction::SinkToSource
                };
                PdRow {
                    index,
                    elapsed_ms,
                    session_ms,
                    kind: RowKind::Message,
                    sop: Some(h.sop.name()),
                    name: h.message_type.name().to_owned(),
                    message_id: Some(h.message_id),
                    direction: Some(direction),
                    objects: Some(h.num_data_objects),
                    spec_revision: Some(h.spec_revision.short_label().to_owned()),
                    vbus_v,
                    ibus_a,
                    summary,
                }
            }
            (kind, _) => {
                let name = match kind {
                    PdRecordKind::Event(e) => e.name(),
                    _ => "Message".to_owned(),
                };
                self.entries.push(Entry {
                    raw: record.raw.clone(),
                    decoded: None,
                });
                PdRow {
                    index,
                    elapsed_ms,
                    session_ms,
                    kind: RowKind::Event,
                    sop: None,
                    name: name.clone(),
                    message_id: None,
                    direction: None,
                    objects: None,
                    spec_revision: None,
                    vbus_v,
                    ibus_a,
                    summary: name,
                }
            }
        }
    }
}

// ---- memory read and device info ----

/// The plaintext block of a memory-read reply for `(addr, len)`: the
/// confirmation frame followed by the encrypted data, as read.
#[wasm_bindgen(js_name = decodeMemoryRead)]
pub fn decode_memory_read(reply: &[u8], addr: u32, len: u32) -> Result<Vec<u8>, BindError> {
    if reply.first().map(|b| b & 0x7F) == Some(0x06) {
        return Err(BindError::new(
            "rejected",
            "the meter rejected the memory read",
        ));
    }
    let plaintext = session::memory_read_plaintext(addr, len);
    let confirmation = reply
        .get(..MEMORY_CONFIRMATION_LEN)
        .filter(|c| c[0] == 0xC4 && c[4..] == plaintext[..16])
        .ok_or_else(|| {
            malformed(format!(
                "bad memory-read confirmation for {len} bytes at {addr:#x}"
            ))
        })?;
    let data = &reply[confirmation.len()..];
    let want = (len as usize).div_ceil(crypto::BLOCK_LEN) * crypto::BLOCK_LEN;
    if data.len() != want {
        return Err(malformed(format!(
            "memory read returned {} bytes, expected {want}",
            data.len()
        )));
    }
    let mut block = crypto::decrypt(&crypto::KEYS[0], data);
    block.truncate(len as usize);
    Ok(block)
}

/// Model, versions and identity of the meter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeviceInfoView {
    /// Model string (e.g. `KM003C`).
    pub model: String,
    /// Application firmware version.
    pub firmware_version: String,
    /// Hardware / boot-loader version.
    pub hardware_version: String,
    /// Serial number.
    pub serial: String,
    /// MCU unique ID as lowercase hex.
    pub uid: String,
}

/// Device info from the hardware and firmware info blocks, the OTP block
/// and the UID, each as [`decode_memory_read`] gave it.
pub fn decode_device_info(
    hardware: &[u8],
    firmware: &[u8],
    otp: &[u8],
    uid: &[u8],
) -> Result<DeviceInfoView, BindError> {
    let info =
        DeviceInfo::decode(hardware, firmware, otp, uid).map_err(|e| malformed(e.to_string()))?;
    Ok(DeviceInfoView {
        model: info.model,
        firmware_version: info.firmware_version,
        hardware_version: info.hardware_version,
        serial: info.serial,
        uid: info.uid,
    })
}

/// [`decode_device_info`] for JS.
#[wasm_bindgen(js_name = decodeDeviceInfo)]
pub fn decode_device_info_js(
    hardware: &[u8],
    firmware: &[u8],
    otp: &[u8],
    uid: &[u8],
) -> Result<JsValue, BindError> {
    to_js(&decode_device_info(hardware, firmware, otp, uid)?)
}
