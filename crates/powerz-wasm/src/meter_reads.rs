//! Meter reads for the web edition (FR W8SHX8): the settings read and the
//! offline-records book. Each request is the frame the native
//! `powerz_protocol::session::Session` sends; each decoder gives what the
//! desktop worker makes of the same reply.

use powerz_protocol::decode::records::{
    MEMORY_READ_MAX_LEN, RecordEntry, RecordSample, decode_record_list, decode_samples,
};
use powerz_protocol::decode::settings::{MeterSettings, Row};
use powerz_protocol::frame::{self, Attribute};
use powerz_protocol::session;
use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::BindError;
use crate::decoders::{malformed, put_data_sections, to_js};

/// The payload of the `att` section of a PUT_DATA reply named `what`. A
/// reply without that section is `malformed`; a REJECT is `rejected`.
pub(crate) fn section_payload<'a>(
    reply: &'a [u8],
    att: Attribute,
    what: &str,
) -> Result<&'a [u8], BindError> {
    put_data_sections(reply, what)?
        .into_iter()
        .find(|s| s.att == att)
        .map(|s| s.payload)
        .ok_or_else(|| malformed(format!("the reply has no {what} section")))
}

// ---- settings (AC-W8SHX8.1) ----

/// GET_DATA SETTINGS, the only request of a settings read.
#[wasm_bindgen(js_name = settingsRequest)]
pub fn settings_request(id: u8) -> Vec<u8> {
    frame::get_data(id, Attribute::SETTINGS).to_vec()
}

/// The meter's settings as the desktop panel shows them; a plain read has
/// no warnings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SettingsView {
    /// Every decoded field as `{key, label, value, tag}`, in display order.
    pub fields: Vec<Row>,
    /// What a write changed beyond the fields; empty for a read.
    pub warnings: Vec<String>,
}

/// The settings rows of a SETTINGS reply. A reply without a SETTINGS
/// section, a short block or a CRC mismatch is `malformed`; a REJECT is
/// `rejected`.
pub fn decode_settings(reply: &[u8]) -> Result<SettingsView, BindError> {
    let settings = MeterSettings::decode(section_payload(reply, Attribute::SETTINGS, "SETTINGS")?)?;
    Ok(SettingsView {
        fields: settings.rows(),
        warnings: Vec::new(),
    })
}

/// [`decode_settings`] for JS.
#[wasm_bindgen(js_name = decodeSettings)]
pub fn decode_settings_js(reply: &[u8]) -> Result<JsValue, BindError> {
    to_js(&decode_settings(reply)?)
}

// ---- offline records (AC-W8SHX8.2..4) ----

/// One planned memory read of a record window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Chunk {
    /// Start address.
    pub addr: u32,
    /// Length, bytes.
    pub len: u32,
}

/// One listed record as the desktop worker's `RecordEntryView` shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RecordEntryView {
    /// 1-based position in the meter's list.
    pub index: usize,
    /// The record's name.
    pub name: String,
    /// Seconds between samples.
    pub interval_s: u8,
    /// Number of samples.
    pub points: u16,
    /// Recorded time in seconds.
    pub duration_s: u32,
    /// Charge in Ah as stored: signed.
    pub charge_ah: f64,
    /// Energy in Wh as stored: signed like the charge.
    pub energy_wh: f64,
    /// Offset of the samples in the record area.
    pub offset: u32,
}

impl RecordEntryView {
    fn new(index: usize, e: &RecordEntry) -> RecordEntryView {
        RecordEntryView {
            index,
            name: e.name.clone(),
            interval_s: e.interval_s,
            points: e.points,
            duration_s: e.duration_s,
            charge_ah: e.charge_ah(),
            energy_wh: e.energy_wh(),
            offset: e.offset,
        }
    }
}

/// The record list view: `{ records: RecordEntryView[] }`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RecordListView {
    /// Listed records, in the meter's order.
    pub records: Vec<RecordEntryView>,
}

/// The offline records book: the latest list and the record windows it
/// makes readable, as the native session keeps them.
#[wasm_bindgen]
#[derive(Debug, Default)]
pub struct RecordBook {
    entries: Vec<RecordEntry>,
    windows: Vec<(u32, u32)>,
}

/// One sample of a downloaded record, as the desktop worker's
/// `RecordSampleView` shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RecordSampleView {
    /// Seconds from the record start.
    pub t_s: f64,
    /// VBUS in V.
    pub vbus_v: f64,
    /// IBUS in A.
    pub ibus_a: f64,
    /// Charge so far in Ah.
    pub charge_ah: f64,
    /// Energy so far in Wh.
    pub energy_wh: f64,
}

impl RecordSampleView {
    fn new(entry: &RecordEntry, k: usize, s: &RecordSample) -> RecordSampleView {
        RecordSampleView {
            t_s: entry.sample_time_s(k) as f64,
            vbus_v: s.vbus_v(),
            ibus_a: s.ibus_a(),
            charge_ah: s.charge_ah(),
            energy_wh: s.energy_wh(),
        }
    }
}

/// A downloaded record, as the desktop worker's `OfflineRecordView`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OfflineRecordView {
    /// 1-based position in the meter's list.
    pub index: usize,
    /// The record's name.
    pub name: String,
    /// Seconds between samples.
    pub interval_s: u8,
    /// Samples in time order.
    pub samples: Vec<RecordSampleView>,
}

#[wasm_bindgen]
impl RecordBook {
    /// An empty book: nothing listed, nothing readable.
    #[wasm_bindgen(constructor)]
    pub fn new() -> RecordBook {
        RecordBook::default()
    }

    /// GET_DATA RECORD_LIST, the request of a record list.
    #[wasm_bindgen(js_name = listRequest)]
    pub fn list_request(&self, id: u8) -> Vec<u8> {
        frame::get_data(id, Attribute::RECORD_LIST).to_vec()
    }

    /// [`decode_list`](Self::decode_list) for JS.
    #[wasm_bindgen(js_name = decodeList)]
    pub fn decode_list_js(&mut self, reply: &[u8]) -> Result<JsValue, BindError> {
        to_js(&self.decode_list(reply)?)
    }

    /// [`chunks`](Self::chunks) for JS: `{addr, len}[]`.
    #[wasm_bindgen(js_name = chunks)]
    pub fn chunks_js(&self, index: u32) -> Result<JsValue, BindError> {
        to_js(&self.chunks(index)?)
    }

    /// [`decode_record`](Self::decode_record) for JS.
    #[wasm_bindgen(js_name = decodeRecord)]
    pub fn decode_record_js(&self, index: u32, data: &[u8]) -> Result<JsValue, BindError> {
        to_js(&self.decode_record(index, data)?)
    }

    /// [`chunk_request`](Self::chunk_request) for JS.
    #[wasm_bindgen(js_name = chunkRequest)]
    pub fn chunk_request_js(&self, id: u8, addr: u32, len: u32) -> Result<Vec<u8>, BindError> {
        self.chunk_request(id, addr, len)
    }
}

fn not_listed() -> BindError {
    BindError::new("usage", "That record is not on the meter.")
}

fn outside_windows(addr: u32, len: u32) -> BindError {
    BindError::new(
        "forbidden",
        format!("memory read of {len} bytes at {addr:#x} is outside the listed record windows"),
    )
}

impl RecordBook {
    /// Decode a RecordList reply and keep its entries and windows. The
    /// previous windows are dropped first, so a failed list leaves none;
    /// an empty list is not an error.
    pub fn decode_list(&mut self, reply: &[u8]) -> Result<RecordListView, BindError> {
        self.entries.clear();
        self.windows.clear();
        let payload = section_payload(reply, Attribute::RECORD_LIST, "RecordList")?;
        let entries = decode_record_list(payload)?;
        self.windows = entries
            .iter()
            .filter_map(RecordEntry::read_window)
            .collect();
        let records = entries
            .iter()
            .enumerate()
            .map(|(i, e)| RecordEntryView::new(i + 1, e))
            .collect();
        self.entries = entries;
        Ok(RecordListView { records })
    }

    /// The listed entry at 1-based `index`, or the desktop app's refusal.
    fn entry(&self, index: u32) -> Result<&RecordEntry, BindError> {
        let i = usize::try_from(index).map_err(|_| not_listed())?;
        i.checked_sub(1)
            .and_then(|i| self.entries.get(i))
            .ok_or_else(not_listed)
    }

    /// The reads for record `index` (1-based): its own window in chunks of
    /// at most [`MEMORY_READ_MAX_LEN`] bytes. An unlisted record or one with
    /// no samples is a `usage` error with the desktop app's wording; a
    /// window not among the listed ones is `forbidden`.
    pub fn chunks(&self, index: u32) -> Result<Vec<Chunk>, BindError> {
        let entry = self.entry(index)?;
        if entry.points == 0 {
            return Err(BindError::new("usage", "That record has no samples."));
        }
        if !entry
            .read_window()
            .is_some_and(|w| self.windows.contains(&w))
        {
            return Err(BindError::new(
                "forbidden",
                "the record lies outside the listed record windows",
            ));
        }
        Ok(entry
            .read_chunks()
            .into_iter()
            .map(|(addr, len)| Chunk { addr, len })
            .collect())
    }

    /// One record-window memory read: `forbidden` unless `0 < len <=`
    /// [`MEMORY_READ_MAX_LEN`] and `[addr, addr + len)` lies inside a window
    /// of the latest list.
    pub fn chunk_request(&self, id: u8, addr: u32, len: u32) -> Result<Vec<u8>, BindError> {
        let end = addr
            .checked_add(len)
            .ok_or_else(|| outside_windows(addr, len))?;
        let inside = len > 0
            && len <= MEMORY_READ_MAX_LEN
            && self
                .windows
                .iter()
                .any(|&(w, n)| addr >= w && w.checked_add(n).is_some_and(|we| end <= we));
        if !inside {
            return Err(outside_windows(addr, len));
        }
        Ok(session::memory_read_request(id, addr, len))
    }

    /// Decode record `index` from its window's bytes: exactly points × 16
    /// bytes, else `malformed`; an unlisted record is `usage`.
    pub fn decode_record(&self, index: u32, data: &[u8]) -> Result<OfflineRecordView, BindError> {
        let entry = self.entry(index)?;
        let samples = decode_samples(entry, data)?;
        Ok(OfflineRecordView {
            index: index as usize,
            name: entry.name.clone(),
            interval_s: entry.interval_s,
            samples: samples
                .iter()
                .enumerate()
                .map(|(k, s)| RecordSampleView::new(entry, k, s))
                .collect(),
        })
    }
}
