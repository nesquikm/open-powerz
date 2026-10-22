//! The PD analyzer view's backend side (FR TGE83C): captured PD records as
//! table rows, the field tree of one record on demand, and the batching of
//! rows to the UI (at most 60 batches a second).

use powerz_protocol::decode::pd_packet::{PdEvent, PdRecordKind, PdStatus};
use powerz_protocol::pd::{self, Decoded, Field, PowerRole, Sop, SpecRevision};
use powerz_protocol::session::CapturedRecord;
use serde::Serialize;

/// Least time between two `pd_rows` batches, ms (at most 60 a second).
pub const ROW_BATCH_MS: u64 = 17;

/// Kind of a table row.
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

/// One PD status block for the chart (FR TGE83D), on the measurement
/// samples' session time base (like a sample's `t_ms`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct PdStatusSample {
    /// Session time, ms (meter time until the capture moves it).
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

/// One row of the packet table.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PdRow {
    /// Record index from 0, in capture order.
    pub index: u64,
    /// Ms since the capture start.
    pub elapsed_ms: u64,
    /// The record's time on the measurement samples' session time base
    /// (like a sample's `t_ms`), for the chart.
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

/// One node of a record's field tree, with the bytes of `raw` it covers.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PdFieldNode {
    /// Field name.
    pub name: String,
    /// Human-readable value.
    pub meaning: String,
    /// The field's bits, shifted down to bit 0.
    pub raw: u64,
    /// `[start, end)` byte range in the record's raw bytes.
    pub bytes: [usize; 2],
    /// Sub-fields.
    pub children: Vec<PdFieldNode>,
}

/// The details of one record.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PdDetails {
    /// Record index.
    pub index: u64,
    /// The PD message bytes (header little-endian, then the body); the
    /// record bytes for an event.
    pub raw: Vec<u8>,
    /// The decoded field tree.
    pub fields: Vec<PdFieldNode>,
}

/// One captured record, kept for its details.
#[derive(Debug, Clone)]
struct Entry {
    raw: Vec<u8>,
    decoded: Option<Decoded>,
}

/// The capture of one PD view: the decoder context, every record so far
/// and the rows waiting to go out.
#[derive(Debug, Default)]
pub struct PdCapture {
    decoder: pd::Capture,
    /// Index of `entries[0]`: indices keep counting across a clear, so rows
    /// sent before it can never be mistaken for rows after it.
    base: u64,
    entries: Vec<Entry>,
    rows_out: Outbox<PdRow>,
    /// Added to the elapsed time of the running sniffer run's records.
    run_offset_ms: u64,
    /// Elapsed time of the table's last record; 0 after a clear.
    last_elapsed_ms: u64,
    /// Meter time at the running sniffer run's start (its elapsed time 0),
    /// taken from its first record; `None` before that record.
    run_session_base: Option<i64>,
    /// Status blocks waiting to go out (FR TGE83D).
    status_out: Outbox<PdStatusSample>,
    /// Session time minus meter time: the samples' timeline offset, so PD
    /// times land on the samples they belong to after a reconnect or a
    /// device clock jump.
    session_offset_ms: i64,
}

/// Items waiting to go out to the UI, sent in batches at most every
/// `ROW_BATCH_MS` (about 60 a second).
#[derive(Debug)]
struct Outbox<T> {
    pending: Vec<T>,
    last_flush_ms: Option<u64>,
}

impl<T> Default for Outbox<T> {
    fn default() -> Outbox<T> {
        Outbox {
            pending: Vec::new(),
            last_flush_ms: None,
        }
    }
}

impl<T> Outbox<T> {
    fn push(&mut self, item: T) {
        self.pending.push(item);
    }

    /// The waiting items, when a batch is due at `now_ms`.
    fn take_due(&mut self, now_ms: u64) -> Option<Vec<T>> {
        if self.next_due_ms().is_none_or(|due| now_ms < due) {
            return None;
        }
        self.last_flush_ms = Some(now_ms);
        Some(std::mem::take(&mut self.pending))
    }

    /// When the next batch can go out, if items are waiting.
    fn next_due_ms(&self) -> Option<u64> {
        (!self.pending.is_empty()).then(|| {
            self.last_flush_ms
                .map_or(0, |l| l.saturating_add(ROW_BATCH_MS))
        })
    }

    /// Every waiting item, now.
    fn take_all(&mut self) -> Option<Vec<T>> {
        (!self.pending.is_empty()).then(|| std::mem::take(&mut self.pending))
    }

    fn clear(&mut self) {
        self.pending.clear();
    }
}

/// Meter time `device_ms` moved by `offset_ms` onto session time.
fn on_session(device_ms: i64, offset_ms: i64) -> u64 {
    u64::try_from(device_ms.saturating_add(offset_ms)).unwrap_or(0)
}

fn sop_name(sop: Sop) -> String {
    match sop {
        Sop::Sop => "SOP".to_owned(),
        Sop::SopPrime => "SOP'".to_owned(),
        Sop::SopDoublePrime => "SOP''".to_owned(),
        Sop::SopPrimeDebug => "SOP'_Debug".to_owned(),
        Sop::SopDoublePrimeDebug => "SOP''_Debug".to_owned(),
        Sop::Other(n) => format!("SOP {n}"),
    }
}

fn revision(rev: SpecRevision) -> String {
    match rev {
        SpecRevision::Rev1 => "1.0",
        SpecRevision::Rev2 => "2.0",
        SpecRevision::Rev3 => "3.x",
        SpecRevision::Reserved => "Reserved",
    }
    .to_owned()
}

fn event_name(event: &PdEvent) -> String {
    match event {
        PdEvent::Attach(_) => "Attach".to_owned(),
        PdEvent::Detach(_) => "Detach".to_owned(),
        PdEvent::BadMessageId => "Bad Message ID".to_owned(),
        PdEvent::HardReset => "Hard Reset".to_owned(),
        PdEvent::CableReset => "Cable Reset".to_owned(),
        PdEvent::ReceiveError(cause) => format!("Receive Error ({cause:?})"),
        PdEvent::MonitorType(t) => format!("Monitor Type ({t:?})"),
        PdEvent::BigMessage => "Big Message".to_owned(),
        PdEvent::Unknown(b) => format!("Event {b:#04x}"),
    }
}

/// Byte range of bits `hi..=lo` of the word at byte `word`.
fn bit_bytes(word: usize, hi: u16, lo: u16) -> [usize; 2] {
    [word + usize::from(lo) / 8, word + usize::from(hi) / 8 + 1]
}

/// `field` with its byte range: the node covers `span`, its children the
/// bits of the word at byte `word`; every range is clamped to `len`.
fn node(field: &Field, span: [usize; 2], word: usize, len: usize) -> PdFieldNode {
    let clamp = |[s, e]: [usize; 2]| [s.min(len), e.min(len)];
    let span = clamp(span);
    PdFieldNode {
        name: field.name.clone(),
        meaning: field.meaning.clone(),
        raw: field.raw,
        bytes: span,
        children: field
            .children
            .iter()
            .map(|c| {
                let r = clamp(bit_bytes(word, c.bits.hi, c.bits.lo));
                // A child stays inside its parent.
                let r = [r[0].clamp(span[0], span[1]), r[1].clamp(span[0], span[1])];
                node(c, r, word, len)
            })
            .collect(),
    }
}

/// The field tree of `decoded` mapped onto its `len` raw bytes.
fn field_nodes(decoded: &Decoded, len: usize) -> Vec<PdFieldNode> {
    let mut at = 0;
    decoded
        .fields()
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let size = if i == 0 || (i == 1 && decoded.extended_header.is_some()) {
                2
            } else {
                4
            };
            let n = node(f, [at, at + size], at, len);
            at += size;
            n
        })
        .collect()
}

impl PdCapture {
    /// An empty capture.
    pub fn new() -> PdCapture {
        PdCapture::default()
    }

    /// A new sniffer run starts: decode it without the last run's context
    /// (the bus may have changed in between) and carry on the table's time,
    /// as the run's own clock starts at 0 again.
    pub fn start_run(&mut self) {
        self.decoder = pd::Capture::new();
        self.run_offset_ms = self.last_elapsed_ms;
        self.run_session_base = None;
    }

    /// The session timeline's current offset (session time minus meter
    /// time), applied to the records and status blocks pushed from now on.
    pub fn set_session_offset(&mut self, offset_ms: i64) {
        self.session_offset_ms = offset_ms;
    }

    /// Decode `records` into rows, with VBUS and IBUS from `status`.
    pub fn push(&mut self, records: &[CapturedRecord], status: Option<PdStatus>) {
        let offset = self.session_offset_ms;
        let status = status.map(|s| PdStatusSample {
            session_ms: on_session(i64::from(s.time_ms), offset),
            ..PdStatusSample::from(s)
        });
        if let Some(s) = status {
            self.status_out.push(s);
        }
        let (vbus_v, ibus_a) = status.map_or((0.0, 0.0), |s| (s.vbus_v, s.ibus_a));
        for r in records {
            let index = self.base + self.entries.len() as u64;
            let elapsed_ms = self.run_offset_ms.saturating_add(r.elapsed_ms);
            // One base per run: the run's elapsed time never goes back, so
            // session time keeps the meter's spacing at a constant offset.
            let base = *self.run_session_base.get_or_insert_with(|| {
                i64::from(r.record.time_ms) - i64::try_from(r.elapsed_ms).unwrap_or(i64::MAX)
            });
            let session_ms = on_session(
                base.saturating_add(i64::try_from(r.elapsed_ms).unwrap_or(i64::MAX)),
                offset,
            );
            self.last_elapsed_ms = self.last_elapsed_ms.max(elapsed_ms);
            let decoded = r.record.pd_input().and_then(|i| self.decoder.feed(i));
            let row = match (&r.record.kind, &decoded) {
                (PdRecordKind::Message(m) | PdRecordKind::BigMessage(m), Some(d)) => {
                    let mut raw = m.header.to_le_bytes().to_vec();
                    raw.extend_from_slice(&m.body);
                    let h = &d.header;
                    let direction = if h.sop.is_cable() {
                        Direction::Cable
                    } else if h.port_power_role == Some(PowerRole::Source) {
                        Direction::SourceToSink
                    } else {
                        Direction::SinkToSource
                    };
                    self.entries.push(Entry {
                        raw,
                        decoded: decoded.clone(),
                    });
                    PdRow {
                        index,
                        elapsed_ms,
                        session_ms,
                        kind: RowKind::Message,
                        sop: Some(sop_name(h.sop)),
                        name: h.message_type.name().to_owned(),
                        message_id: Some(h.message_id),
                        direction: Some(direction),
                        objects: Some(h.num_data_objects),
                        spec_revision: Some(revision(h.spec_revision)),
                        vbus_v,
                        ibus_a,
                        summary: d.summary(),
                    }
                }
                (kind, _) => {
                    let name = match kind {
                        PdRecordKind::Event(e) => event_name(e),
                        _ => "Message".to_owned(),
                    };
                    self.entries.push(Entry {
                        raw: r.record.raw.clone(),
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
            };
            self.rows_out.push(row);
        }
    }

    /// The rows waiting to go out, when a batch is due at `now_ms`.
    pub fn take_due(&mut self, now_ms: u64) -> Option<Vec<PdRow>> {
        self.rows_out.take_due(now_ms)
    }

    /// When the next batch can go out, if rows are waiting.
    pub fn next_due_ms(&self) -> Option<u64> {
        self.rows_out.next_due_ms()
    }

    /// Every waiting row, now.
    pub fn take_all(&mut self) -> Option<Vec<PdRow>> {
        self.rows_out.take_all()
    }

    /// The status blocks waiting to go out, when a batch is due at `now_ms`
    /// (at most 60 a second, like the rows).
    pub fn take_status_due(&mut self, now_ms: u64) -> Option<Vec<PdStatusSample>> {
        self.status_out.take_due(now_ms)
    }

    /// When the next status batch can go out, if blocks are waiting.
    pub fn next_status_due_ms(&self) -> Option<u64> {
        self.status_out.next_due_ms()
    }

    /// Every waiting status block, now.
    pub fn take_all_status(&mut self) -> Option<Vec<PdStatusSample>> {
        self.status_out.take_all()
    }

    /// The details of record `index`.
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

    /// Drop every record and return the index the next one gets; indices
    /// do not restart. The decoder keeps its context, so a capture that goes
    /// on decodes as before; the next run's time starts at 0 again.
    pub fn clear(&mut self) -> u64 {
        self.last_elapsed_ms = 0;
        self.base += self.entries.len() as u64;
        self.entries.clear();
        self.rows_out.clear();
        self.status_out.clear();
        self.base
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use powerz_protocol::decode::pd_packet::{PdMessage, PdRecord};

    /// A SOP message record at `elapsed_ms`.
    fn message(elapsed_ms: u64, header: u16, object: u32) -> CapturedRecord {
        CapturedRecord {
            elapsed_ms,
            record: PdRecord {
                time_ms: 0,
                from_meter: false,
                raw: Vec::new(),
                kind: PdRecordKind::Message(PdMessage {
                    sop: 0,
                    header,
                    body: object.to_le_bytes().to_vec(),
                }),
            },
        }
    }

    /// Source_Capabilities (rev 3, source) with one 5 V / 3 A fixed PDO.
    fn source_caps(elapsed_ms: u64) -> CapturedRecord {
        message(elapsed_ms, 0x1181, (100 << 10) | 300)
    }

    /// Request (rev 3, sink) for PDO 1 at 3 A.
    fn request(elapsed_ms: u64) -> CapturedRecord {
        message(elapsed_ms, 0x1082, (1 << 28) | (300 << 10) | 300)
    }

    #[test]
    fn a_new_run_decodes_without_the_last_runs_context() {
        let mut fresh = PdCapture::new();
        fresh.push(&[request(0)], None);
        let expected = fresh.take_all().unwrap_or_default();

        let mut pd = PdCapture::new();
        pd.push(&[source_caps(0)], None);
        pd.start_run();
        pd.push(&[request(0)], None);
        let rows = pd.take_all().unwrap_or_default();
        assert_eq!(
            rows.last().map(|r| &r.summary),
            expected.first().map(|r| &r.summary),
            "the Request was decoded against the last run's capabilities"
        );
        assert_eq!(
            pd.details(1).map(|d| d.fields),
            fresh.details(0).map(|d| d.fields)
        );
    }

    #[test]
    fn a_new_run_continues_the_tables_time() {
        let mut pd = PdCapture::new();
        pd.push(&[source_caps(0), request(700)], None);
        pd.start_run();
        pd.push(&[source_caps(0), request(5)], None);
        let times: Vec<u64> = pd
            .take_all()
            .unwrap_or_default()
            .iter()
            .map(|r| r.elapsed_ms)
            .collect();
        assert_eq!(times, vec![0, 700, 700, 705]);
    }

    #[test]
    fn a_new_run_after_a_clear_starts_the_time_again() {
        let mut pd = PdCapture::new();
        pd.push(&[source_caps(0), request(700)], None);
        pd.clear();
        pd.start_run();
        pd.push(&[source_caps(3)], None);
        let times: Vec<u64> = pd
            .take_all()
            .unwrap_or_default()
            .iter()
            .map(|r| r.elapsed_ms)
            .collect();
        assert_eq!(times, vec![3]);
    }

    #[test]
    fn a_clear_drops_status_samples_still_waiting_to_go_out() {
        let mut c = PdCapture::new();
        let status = PdStatus {
            time_ms: 10,
            vbus_mv: 5_000,
            ibus_ma: 100,
            cc1_mv: 1_600,
            cc2_mv: 0,
        };
        c.push(&[source_caps(0)], Some(status));
        c.clear();
        assert_eq!(c.take_all_status(), None);
        assert_eq!(c.take_all(), None);
    }
}
