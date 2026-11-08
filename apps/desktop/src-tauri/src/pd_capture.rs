//! The PD analyzer view's backend side (FR TGE83C): captured PD records as
//! table rows, the field tree of one record on demand, and the batching of
//! rows to the UI (at most 60 batches a second).

use powerz_protocol::decode::pd_packet::{PdRecord, PdRecordKind, PdStatus};
use powerz_protocol::pd::{self, Decoded, PowerRole};
use powerz_protocol::pd_details::{self, field_nodes};
use powerz_protocol::pdcap;
use powerz_protocol::pdm::message::cable_identity_from_message;
use powerz_protocol::session::CapturedRecord;
use serde::Serialize;
use std::collections::VecDeque;

use crate::trigger::{CableView, cable_view};

/// Least time between two `pd_rows` batches, ms (at most 60 a second).
pub const ROW_BATCH_MS: u64 = 17;

/// Records a capture keeps by default (FR 4PJRVK, AC .3); the UI's store
/// keeps as many rows (`PD_MAX_ROWS`).
pub const MAX_RECORDS: usize = 100_000;

/// Status blocks a capture keeps by default (FR AXHK2W, AC .2), as many as
/// records; the page's status buffer keeps as many samples.
pub const MAX_STATUS_SAMPLES: usize = 100_000;

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

/// What a clear answers: where record numbering goes on, and the latest
/// status time the capture had.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct PdCleared {
    /// Index of the next record; rows numbered below it are stale.
    pub next_index: u64,
    /// Session time (ms) of the latest status sample the capture had at the
    /// clear, 0 if none. Status samples carry no index, so the page drops
    /// those at or below this time that arrive after the clear.
    pub status_floor_ms: u64,
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
pub use powerz_protocol::pd_details::FieldNode as PdFieldNode;

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

/// How many records a capture keeps.
#[derive(Debug, Clone, Copy)]
struct RecordLimit(usize);

impl Default for RecordLimit {
    fn default() -> RecordLimit {
        RecordLimit(MAX_RECORDS)
    }
}

/// How many status blocks a capture keeps.
#[derive(Debug, Clone, Copy)]
struct StatusLimit(usize);

impl Default for StatusLimit {
    fn default() -> StatusLimit {
        StatusLimit(MAX_STATUS_SAMPLES)
    }
}

/// The capture of one PD view: the decoder context, the last `limit` records
/// and the rows waiting to go out.
#[derive(Debug, Default)]
pub struct PdCapture {
    decoder: pd::Capture,
    /// Index of `entries[0]`: indices keep counting across a clear, so rows
    /// sent before it can never be mistaken for rows after it, and across
    /// the records dropped past the limit.
    base: u64,
    entries: VecDeque<Entry>,
    limit: RecordLimit,
    /// Records dropped past the limit since the last clear.
    dropped: u64,
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
    /// Every record kept so far, with its place in capture order (FR
    /// TGE83E); merged with `status_log` into what a pdcap file saves.
    record_log: VecDeque<(u64, pdcap::Entry)>,
    /// The last `status_limit` status blocks, with their place in capture
    /// order (FR AXHK2W, AC .2).
    status_log: VecDeque<(u64, pdcap::Entry)>,
    /// Place in capture order of the next logged entry.
    log_seq: u64,
    status_limit: StatusLimit,
    /// Status blocks dropped past the limit since the last clear.
    status_dropped: u64,
    /// Session time of the latest status sample pushed (0 if none).
    last_status_ms: u64,
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

impl PdCapture {
    /// An empty capture.
    pub fn new() -> PdCapture {
        PdCapture::default()
    }

    /// An empty capture keeping at most `max_records` records.
    pub fn with_limit(max_records: usize) -> PdCapture {
        PdCapture {
            limit: RecordLimit(max_records),
            ..PdCapture::default()
        }
    }

    /// This capture, keeping at most `max_status` status blocks.
    pub fn with_status_limit(mut self, max_status: usize) -> PdCapture {
        self.status_limit = StatusLimit(max_status);
        self.trim_status();
        self
    }

    /// Status blocks dropped past the limit since the last clear.
    pub fn status_dropped(&self) -> u64 {
        self.status_dropped
    }

    /// How many records this capture keeps.
    pub fn limit(&self) -> usize {
        self.limit.0
    }

    /// Index of the oldest record the capture keeps; rows numbered below it
    /// were dropped past the limit.
    pub fn first_index(&self) -> u64 {
        self.base
    }

    /// Records dropped past the limit since the last clear.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Drop the oldest records past the limit, with the status blocks
    /// logged before them. Indices keep counting.
    fn trim(&mut self) {
        while self.entries.len() > self.limit.0 {
            self.entries.pop_front();
            self.base += 1;
            self.dropped += 1;
            let Some((seq, _)) = self.record_log.pop_front() else {
                continue;
            };
            // Status blocks logged before the dropped record go with it and
            // count as dropped too (AC-AXHK2W.2), so the backend's count
            // never says less than it lost.
            while self.status_log.front().is_some_and(|(s, _)| *s < seq) {
                self.status_log.pop_front();
                self.status_dropped += 1;
            }
        }
    }

    /// Drop the oldest status blocks past the status limit and count them;
    /// records stay. Runs as a batch goes out, so the count a batch carries
    /// covers exactly the blocks sent so far.
    fn trim_status(&mut self) {
        while self.status_log.len() > self.status_limit.0 {
            self.status_log.pop_front();
            self.status_dropped += 1;
        }
    }

    /// `batch`, after trimming the status log if it goes out (see
    /// [`Self::trim_status`]).
    fn trimmed_if_sent(
        &mut self,
        batch: Option<Vec<PdStatusSample>>,
    ) -> Option<Vec<PdStatusSample>> {
        if batch.is_some() {
            self.trim_status();
        }
        batch
    }

    /// Log `entry` as the next one in capture order.
    fn log_entry(&mut self, entry: pdcap::Entry) {
        let seq = self.log_seq;
        self.log_seq += 1;
        match entry {
            pdcap::Entry::Status { .. } => self.status_log.push_back((seq, entry)),
            pdcap::Entry::Record { .. } => self.record_log.push_back((seq, entry)),
        }
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
        let raw_status = status;
        let status = status.map(|s| PdStatusSample {
            session_ms: on_session(i64::from(s.time_ms), offset),
            ..PdStatusSample::from(s)
        });
        if let Some(s) = status {
            self.last_status_ms = self.last_status_ms.max(s.session_ms);
            self.status_out.push(s);
        }
        if let (Some(raw), Some(s)) = (raw_status, status) {
            self.log_entry(pdcap::Entry::Status {
                session_ms: s.session_ms,
                status: raw,
            });
        }
        let (vbus_v, ibus_a) = status.map_or((0.0, 0.0), |s| (s.vbus_v, s.ibus_a));
        for r in records {
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
            self.log_entry(pdcap::Entry::Record {
                session_ms,
                elapsed_ms,
                record: r.record.clone(),
            });
            self.push_row(&r.record, elapsed_ms, session_ms, vbus_v, ibus_a);
        }
        self.trim();
    }

    /// Decode `record` into the next row and keep it for its details.
    fn push_row(
        &mut self,
        record: &PdRecord,
        elapsed_ms: u64,
        session_ms: u64,
        vbus_v: f64,
        ibus_a: f64,
    ) {
        let index = self.base + self.entries.len() as u64;
        let decoded = record.pd_input().and_then(|i| self.decoder.feed(i));
        let row = match (&record.kind, &decoded) {
            (PdRecordKind::Message(_) | PdRecordKind::BigMessage(_), Some(d)) => {
                let raw = pd_details::detail_bytes(record);
                let h = &d.header;
                let direction = if h.sop.is_cable() {
                    Direction::Cable
                } else if h.port_power_role == Some(PowerRole::Source) {
                    Direction::SourceToSink
                } else {
                    Direction::SinkToSource
                };
                self.entries.push_back(Entry {
                    raw,
                    decoded: decoded.clone(),
                });
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
                    summary: d.summary(),
                }
            }
            (kind, _) => {
                let name = match kind {
                    PdRecordKind::Event(e) => e.name(),
                    _ => "Message".to_owned(),
                };
                self.entries.push_back(Entry {
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
        };
        self.rows_out.push(row);
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
        let due = self.status_out.take_due(now_ms);
        self.trimmed_if_sent(due)
    }

    /// When the next status batch can go out, if blocks are waiting.
    pub fn next_status_due_ms(&self) -> Option<u64> {
        self.status_out.next_due_ms()
    }

    /// Every waiting status block, now.
    pub fn take_all_status(&mut self) -> Option<Vec<PdStatusSample>> {
        let all = self.status_out.take_all();
        self.trimmed_if_sent(all)
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

    /// The newest cable Discover Identity ACK the capture keeps (FR AXHK2P).
    pub fn latest_cable(&self) -> Option<CableView> {
        self.record_log.iter().rev().find_map(|(_, entry)| {
            let pdcap::Entry::Record { record, .. } = entry else {
                return None;
            };
            let (PdRecordKind::Message(m) | PdRecordKind::BigMessage(m)) = &record.kind else {
                return None;
            };
            cable_identity_from_message(m.sop, m.header, &m.body).map(|c| cable_view(&c))
        })
    }

    /// Every status block and record kept so far, in capture order, on
    /// session time: what a pdcap file saves. Rows already sent to the UI
    /// are included; nothing waiting to go out is taken.
    pub fn to_pdcap(&self) -> Vec<pdcap::Entry> {
        let mut out = Vec::with_capacity(self.record_log.len() + self.status_log.len());
        let (mut records, mut status) = (
            self.record_log.iter().peekable(),
            self.status_log.iter().peekable(),
        );
        loop {
            let next = match (records.peek(), status.peek()) {
                (Some(r), Some(s)) if r.0 < s.0 => records.next(),
                (Some(_), Some(_)) | (None, Some(_)) => status.next(),
                (Some(_), None) => records.next(),
                (None, None) => break,
            };
            if let Some((_, e)) = next {
                out.push(e.clone());
            }
        }
        out
    }

    /// A capture rebuilt from the entries of a pdcap file: every row and
    /// status sample waiting to go out, every record's details answered.
    /// A record takes VBUS and IBUS from the status block before it, as a
    /// poll's records take them from the poll's status block.
    pub fn from_pdcap(entries: &[pdcap::Entry]) -> PdCapture {
        let mut pd = PdCapture::new();
        let (mut vbus_v, mut ibus_a) = (0.0, 0.0);
        for e in entries {
            match e {
                pdcap::Entry::Status { session_ms, status } => {
                    let s = PdStatusSample {
                        session_ms: *session_ms,
                        ..PdStatusSample::from(*status)
                    };
                    (vbus_v, ibus_a) = (s.vbus_v, s.ibus_a);
                    pd.status_out.push(s);
                }
                pdcap::Entry::Record {
                    session_ms,
                    elapsed_ms,
                    record,
                } => {
                    pd.last_elapsed_ms = pd.last_elapsed_ms.max(*elapsed_ms);
                    pd.push_row(record, *elapsed_ms, *session_ms, vbus_v, ibus_a);
                }
            }
        }
        for e in entries {
            pd.log_entry(e.clone());
        }
        pd.trim();
        pd.trim_status();
        pd
    }

    /// Add one imported record (FR TGE83E, AC .6) as the next row, at
    /// `session_ms` from the log's start, with the row's own VBUS and IBUS.
    pub(crate) fn push_imported(
        &mut self,
        record: PdRecord,
        session_ms: u64,
        vbus_v: f64,
        ibus_a: f64,
    ) {
        let elapsed_ms = self.last_elapsed_ms.max(session_ms);
        self.last_elapsed_ms = elapsed_ms;
        self.push_row(&record, elapsed_ms, session_ms, vbus_v, ibus_a);
        self.log_entry(pdcap::Entry::Record {
            session_ms,
            elapsed_ms,
            record,
        });
        self.trim();
    }

    /// Add one imported status sample for the timeline (FR TGE83E, AC .6).
    pub(crate) fn push_imported_status(&mut self, sample: PdStatusSample) {
        self.last_status_ms = self.last_status_ms.max(sample.session_ms);
        self.status_out.push(sample);
    }

    /// [`PdCapture::clear`] with the status floor, as the UI needs it.
    pub fn clear_with_floor(&mut self) -> PdCleared {
        let status_floor_ms = self.last_status_ms;
        PdCleared {
            next_index: self.clear(),
            status_floor_ms,
        }
    }

    /// Drop every record and return the index the next one gets; indices
    /// do not restart. The decoder keeps its context, so a capture that goes
    /// on decodes as before; the next run's time starts at 0 again.
    pub fn clear(&mut self) -> u64 {
        self.last_elapsed_ms = 0;
        self.base += self.entries.len() as u64;
        self.entries.clear();
        self.dropped = 0;
        self.record_log.clear();
        self.status_log.clear();
        self.status_dropped = 0;
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

    // ---- AC-AXHK2P.6: the identity a capture already holds ----

    /// Session 2: the 240 W cable's Discover Identity ACK on SOP'. [device]
    const CABLE_ACK: [u32; 5] = [
        0xFF00_A841,
        0x1860_050D,
        0x0000_404D,
        0x8254_0000,
        0x000A_2640,
    ];
    const ACK_HEADER: u16 = 0x518F;

    /// A message record of `objects` on `sop`.
    fn record(sop: u8, header: u16, objects: &[u32]) -> CapturedRecord {
        CapturedRecord {
            elapsed_ms: 0,
            record: PdRecord {
                time_ms: 0,
                from_meter: false,
                raw: Vec::new(),
                kind: PdRecordKind::Message(PdMessage {
                    sop,
                    header,
                    body: objects.iter().flat_map(|o| o.to_le_bytes()).collect(),
                }),
            },
        }
    }

    fn ack_of(vid: u16) -> [u32; 5] {
        let mut ack = CABLE_ACK;
        ack[1] = 0x1860_0000 | u32::from(vid);
        ack
    }

    #[test]
    fn a_capture_with_no_cable_ack_has_no_cable() {
        let mut pd = PdCapture::new();
        assert_eq!(pd.latest_cable(), None);
        pd.push(&[source_caps(0), request(1)], None);
        assert_eq!(pd.latest_cable(), None);
    }

    #[test]
    fn a_cable_ack_in_the_live_capture_is_found() {
        let mut pd = PdCapture::new();
        pd.push(
            &[
                source_caps(0),
                record(1, ACK_HEADER, &CABLE_ACK),
                request(1),
            ],
            None,
        );
        let cable = pd.latest_cable().expect("the ACK");
        assert_eq!(cable.vid, 0x050D);
        assert_eq!(cable.pid, Some(0x8254));
        assert_eq!(cable.max_voltage_v, Some(50));
    }

    #[test]
    fn the_newest_cable_ack_wins() {
        let mut pd = PdCapture::new();
        pd.push(&[record(1, ACK_HEADER, &ack_of(0x05AC))], None);
        pd.push(&[record(1, ACK_HEADER, &ack_of(0x18D1))], None);
        assert_eq!(pd.latest_cable().map(|c| c.vid), Some(0x18D1));
    }

    #[test]
    fn a_later_record_that_does_not_decode_leaves_the_newest_ack() {
        let mut pd = PdCapture::new();
        pd.push(
            &[
                record(1, ACK_HEADER, &ack_of(0x05AC)),
                // A REQ, the same ACK on SOP, and an ACK that is no cable.
                record(1, 0x108F, &[0xFF00_A801]),
                record(0, ACK_HEADER, &ack_of(0x18D1)),
                record(1, ACK_HEADER, &[CABLE_ACK[0], 0x1060_1234, 0, 0, 0]),
            ],
            None,
        );
        assert_eq!(pd.latest_cable().map(|c| c.vid), Some(0x05AC));
    }

    #[test]
    fn a_far_plugs_ack_counts_too() {
        let mut pd = PdCapture::new();
        pd.push(&[record(2, ACK_HEADER, &CABLE_ACK)], None);
        assert_eq!(pd.latest_cable().map(|c| c.vid), Some(0x050D));
    }

    #[test]
    fn a_nak_or_a_request_is_no_identity() {
        let mut pd = PdCapture::new();
        pd.push(
            &[
                record(1, 0x108F, &[0xFF00_A801]),
                record(1, 0x108F, &[0xFF00_A881]),
            ],
            None,
        );
        assert_eq!(pd.latest_cable(), None);
    }

    #[test]
    fn the_capture_opened_from_a_file_counts_as_well() {
        let mut live = PdCapture::new();
        live.push(&[source_caps(0), record(1, ACK_HEADER, &CABLE_ACK)], None);
        let opened = PdCapture::from_pdcap(&live.to_pdcap());
        assert_eq!(opened.latest_cable().map(|c| c.vid), Some(0x050D));
    }

    #[test]
    fn a_cleared_capture_has_no_cable() {
        let mut pd = PdCapture::new();
        pd.push(&[record(1, ACK_HEADER, &CABLE_ACK)], None);
        pd.clear();
        assert_eq!(pd.latest_cable(), None);
    }

    #[test]
    fn an_ack_dropped_past_the_limit_is_gone() {
        let mut pd = PdCapture::with_limit(2);
        pd.push(
            &[
                record(1, ACK_HEADER, &CABLE_ACK),
                source_caps(1),
                request(2),
            ],
            None,
        );
        assert_eq!(pd.latest_cable(), None);
    }
}
