//! Opening a PD capture in the browser (FR EAKFEC): a pdcap file read with
//! the desktop app's loader and rebuilt with its capture
//! ([`powerz_protocol::pd_capture::PdCapture`]), and a vendor PD log whose
//! SQL sql.js runs, turned into rows by the desktop's import rules
//! ([`powerz_protocol::pd_capture::VendorLog`]). Same rows, same counts and
//! the desktop's words when a file is refused.

use powerz_protocol::pd_capture::{
    self, IMPORT_FAILED, ImportLimits, NOT_PD_LOG, OPEN_FAILED, PdCapture, PdDetails, PdRow,
    PdStatusSample, VendorLog, vendor_sql,
};
use powerz_protocol::pdcap;
use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::BindError;

/// `value` for JS with `None` as `null`, as the desktop's JSON answers it, so
/// the page sees the same view from both apps.
fn to_js<T: Serialize>(value: &T) -> Result<JsValue, BindError> {
    value
        .serialize(&serde_wasm_bindgen::Serializer::new().serialize_missing_as_null(true))
        .map_err(|e| BindError::new("internal", e.to_string()))
}

/// A capture opened from a file, in the desktop's `PdOpened` shape less the
/// path (the page names the file).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OpenedPdView {
    /// Every row the capture keeps (at most its record limit).
    pub rows: Vec<PdRow>,
    /// Records dropped past the limit (the oldest ones); not in `rows`.
    pub dropped: u64,
    /// Every status sample of the capture.
    pub status: Vec<PdStatusSample>,
    /// The 1-based line where reading stopped in a damaged pdcap file.
    pub damaged_line: Option<usize>,
    /// Vendor log rows skipped because their record bytes are oversized.
    pub oversized: u64,
    /// Vendor log rows skipped because they hold no whole record.
    pub skipped: u64,
    /// The vendor log has more rows than the import takes.
    pub truncated: bool,
}

/// A capture opened from a file: its view once, then the details of its
/// records and the cable identity it holds, as long as the page shows it.
#[wasm_bindgen]
#[derive(Debug)]
pub struct OpenedPd {
    capture: PdCapture,
    view: Option<OpenedPdView>,
}

/// What the page shows of `capture`, with what the file left out.
fn opened(
    mut capture: PdCapture,
    damaged_line: Option<usize>,
    oversized: u64,
    skipped: u64,
    truncated: bool,
) -> OpenedPd {
    let (rows, dropped, status) = pd_capture::opened_view(&mut capture);
    OpenedPd {
        capture,
        view: Some(OpenedPdView {
            rows,
            dropped,
            status,
            damaged_line,
            oversized,
            skipped,
            truncated,
        }),
    }
}

/// The pdcap file `bytes` as an opened capture. A file damaged after its
/// header opens with what it holds and names the line; a file without a
/// pdcap header is refused in the desktop's words.
pub fn open_pdcap(bytes: &[u8]) -> Result<OpenedPd, BindError> {
    let loaded = pdcap::load(bytes);
    if let (Some(e), None) = (&loaded.error, &loaded.header) {
        return Err(BindError::new("usage", format!("{OPEN_FAILED}: {e}")));
    }
    let capture = PdCapture::from_pdcap(&loaded.entries);
    let damaged_line = loaded.error.as_ref().map(|e| e.line());
    Ok(opened(capture, damaged_line, 0, 0, false))
}

/// [`open_pdcap`] for JS.
#[wasm_bindgen(js_name = openPdcap)]
pub fn open_pdcap_js(bytes: &[u8]) -> Result<OpenedPd, BindError> {
    open_pdcap(bytes)
}

#[wasm_bindgen]
impl OpenedPd {
    /// The rows, status samples and counts the page shows, once: `undefined`
    /// after the first call (the capture keeps only what its details need).
    pub fn take(&mut self) -> Result<JsValue, BindError> {
        match self.take_view() {
            Some(v) => to_js(&v),
            None => Ok(JsValue::UNDEFINED),
        }
    }

    /// The raw bytes and field tree of record `index`; `undefined` for a
    /// record the capture does not keep.
    #[wasm_bindgen(js_name = details)]
    pub fn details_js(&self, index: u64) -> Result<JsValue, BindError> {
        match self.details(index) {
            Some(d) => to_js(&d),
            None => Ok(JsValue::UNDEFINED),
        }
    }

    /// The newest cable identity the capture holds, in the trigger panel's
    /// shape; `null` when it holds none.
    #[wasm_bindgen(js_name = latestCable)]
    pub fn latest_cable_js(&self) -> Result<JsValue, BindError> {
        match self.capture.latest_cable() {
            Some(c) => to_js(&powerz_trigger::cable_view(&c)),
            None => Ok(JsValue::NULL),
        }
    }
}

impl OpenedPd {
    /// The view, once.
    pub fn take_view(&mut self) -> Option<OpenedPdView> {
        self.view.take()
    }

    /// The details of record `index`.
    pub fn details(&self, index: u64) -> Option<PdDetails> {
        self.capture.details(index)
    }
}

// ---- a vendor PD log, its SQL run by sql.js ----

/// The vendor PD log's statements and limits, for sql.js.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VendorPdSql {
    /// One row when the table bound as `?1` exists.
    pub has_table: &'static str,
    /// The count of `pd_table` rows whose `Raw` is over `?1` bytes.
    pub oversized: &'static str,
    /// The `pd_table` rows (`?1` the byte limit, `?2` the fetch limit).
    pub records: &'static str,
    /// The `pd_chart` rows (`?1` the fetch limit).
    pub chart: &'static str,
    /// The byte limit of one `Raw` value.
    pub max_raw_bytes: u64,
    /// The row limit plus one: the `LIMIT` both row queries take.
    pub fetch: u64,
}

/// The statements and limits of the desktop's import.
pub fn vendor_pd_sql() -> VendorPdSql {
    let limits = ImportLimits::DEFAULT;
    VendorPdSql {
        has_table: vendor_sql::HAS_TABLE,
        oversized: vendor_sql::OVERSIZED,
        records: vendor_sql::RECORDS,
        chart: vendor_sql::CHART,
        max_raw_bytes: limits.max_raw_bytes,
        fetch: VendorLog::new(limits, 0).fetch(),
    }
}

/// [`vendor_pd_sql`] for JS.
#[wasm_bindgen(js_name = vendorPdSql)]
pub fn vendor_pd_sql_js() -> Result<JsValue, BindError> {
    to_js(&vendor_pd_sql())
}

/// The refusal of a vendor PD log in the desktop's words: `detail` is why
/// the database could not be read, `None` for one with no `pd_table`.
pub fn vendor_pd_refusal(detail: Option<String>) -> BindError {
    let why = match detail {
        Some(d) => format!("could not read the vendor PD log: {d}"),
        None => NOT_PD_LOG.to_owned(),
    };
    BindError::new("usage", format!("{IMPORT_FAILED}: {why}"))
}

/// [`vendor_pd_refusal`] for JS.
#[wasm_bindgen(js_name = vendorPdRefusal)]
pub fn vendor_pd_refusal_js(detail: Option<String>) -> BindError {
    vendor_pd_refusal(detail)
}

/// A vendor PD log taken in one result row at a time, by the desktop's rules.
#[wasm_bindgen]
#[derive(Debug)]
pub struct VendorPdLog {
    log: VendorLog,
}

/// A number column: NaN stands for anything that is no number.
fn number(n: f64) -> Option<f64> {
    (!n.is_nan()).then_some(n)
}

#[wasm_bindgen]
impl VendorPdLog {
    /// An empty import within the desktop's limits, `oversized` rows left out.
    #[wasm_bindgen(constructor)]
    pub fn new(oversized: f64) -> VendorPdLog {
        // `as` saturates: a NaN or negative count is 0.
        VendorPdLog {
            log: VendorLog::new(ImportLimits::DEFAULT, oversized as u64),
        }
    }

    /// One `pd_table` row: Time, Vbus and Ibus (NaN for no number) and Raw
    /// when it is a blob. False once the cap is passed: stop reading.
    #[wasm_bindgen(js_name = pushRecord)]
    pub fn push_record(
        &mut self,
        time_s: f64,
        vbus_v: f64,
        ibus_a: f64,
        raw: Option<Vec<u8>>,
    ) -> bool {
        self.log.push_record(
            number(time_s),
            number(vbus_v),
            number(ibus_a),
            raw.as_deref(),
        )
    }

    /// One `pd_chart` row (NaN for no number). False once the cap is passed.
    #[wasm_bindgen(js_name = pushChart)]
    pub fn push_chart(&mut self, values: &[f64]) -> bool {
        let at = |i: usize| values.get(i).copied().and_then(number);
        self.log.push_chart([at(0), at(1), at(2), at(3), at(4)])
    }

    /// The opened capture; consumes the import.
    pub fn finish(self) -> OpenedPd {
        let i = self.log.finish();
        opened(i.capture, None, i.oversized, i.skipped, i.truncated)
    }
}

impl VendorPdLog {
    /// One `pd_table` row with typed values (tests and native callers).
    pub fn take_record(
        &mut self,
        time_s: Option<f64>,
        vbus_v: Option<f64>,
        ibus_a: Option<f64>,
        raw: Option<&[u8]>,
    ) -> bool {
        self.log.push_record(time_s, vbus_v, ibus_a, raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_without_a_header_is_refused_in_the_desktops_words() {
        let e = open_pdcap(b"").unwrap_err();
        assert_eq!(e.kind, "usage");
        assert!(
            e.message.starts_with("Could not open the PD capture: "),
            "{}",
            e.message
        );
    }

    #[test]
    fn a_log_with_no_pd_table_is_refused_in_the_desktops_words() {
        assert_eq!(
            vendor_pd_refusal(None).message,
            "Could not import the vendor PD log: the file is not a vendor PD log (it has no pd_table)"
        );
        assert_eq!(
            vendor_pd_refusal(Some("file is not a database".into())).message,
            "Could not import the vendor PD log: could not read the vendor PD log: file is not a database"
        );
    }

    #[test]
    fn the_view_is_taken_once() {
        let mut o = VendorPdLog::new(0.0).finish();
        assert!(o.take_view().is_some());
        assert!(o.take_view().is_none());
    }
}
