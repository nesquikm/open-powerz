//! The first rows of a desktop session have no TEMP (FR 3B70S7, AC-3B70S7.2).
//!
//! The worker appends the records of a queue poll to the session store before it
//! reads the ADC snapshot that carries the temperature, so the records of the first
//! poll, appended before the first snapshot, have no TEMP; the records after it carry
//! the reading. This pins that rule over the simulator on its fake clock, in the
//! recording and in the export.

mod common;

use std::sync::mpsc;

use common::*;
use open_powerz_desktop_lib::store::SessionStore;
use open_powerz_desktop_lib::worker::{Worker, WorkerConfig};
use powerz_protocol::decode::Rate;
use powerz_record::CSV_HEADER;

struct ChannelWriter(mpsc::Sender<Vec<u8>>);

impl std::io::Write for ChannelWriter {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        let _ = self.0.send(data.to_vec());
        Ok(data.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The data rows of a CSV written through a [`ChannelWriter`] (header checked).
fn rows(rx: &mpsc::Receiver<Vec<u8>>) -> Vec<String> {
    let bytes: Vec<u8> = rx.try_iter().flatten().collect();
    let text = String::from_utf8(bytes).expect("utf-8 CSV");
    let mut lines = text.lines().map(str::to_owned);
    assert_eq!(lines.next().as_deref(), Some(CSV_HEADER), "the CSV header");
    lines.collect()
}

/// The TEMP column of each row.
fn temps(rows: &[String]) -> Vec<String> {
    let at = CSV_HEADER
        .split(',')
        .position(|c| c == "TEMP")
        .expect("a TEMP column");
    rows.iter()
        .map(|r| r.split(',').nth(at).unwrap_or("").to_owned())
        .collect()
}

/// Checks that `t` is a non-empty run of empty TEMP cells followed only by filled
/// ones, and returns the run's length.
fn leading_empty_then_filled(t: &[String], what: &str) -> usize {
    let empty = t.iter().take_while(|c| c.is_empty()).count();
    assert!(
        empty > 0,
        "{what}: the first poll's rows carry a TEMP before the first snapshot"
    );
    assert!(empty < t.len(), "{what}: no row ever carries a TEMP");
    assert!(
        t[empty..].iter().all(|c| !c.is_empty()),
        "{what}: a row after the first snapshot has no TEMP"
    );
    let first: f64 = t[empty].parse().expect("a temperature");
    // The simulator's chip temperature is 30 ± 2 °C.
    assert!(
        (27.0..=33.0).contains(&first),
        "{what}: temperature {first}"
    );
    empty
}

#[test]
fn the_first_polls_records_have_no_temp_and_the_rows_after_the_first_snapshot_carry_it() {
    for rate in [Rate::Sps50, Rate::Sps1000] {
        let clock = clock();
        let pipe = Pipe::sim(
            Scenario::Load5v1a,
            FaultPlan::new(),
            &clock,
            PipeOpts::default(),
        );
        let store = SessionStore::new();
        let (_events, sink) = capture(&clock);
        let mut worker = Worker::start_with_store(
            pipe,
            clock.clone(),
            WorkerConfig::new(rate, 1),
            store.clone(),
            sink,
        )
        .unwrap_or_else(|e| panic!("worker did not start: {e:?}"));
        assert_eq!(store.history_len(), 0, "precondition: no record yet");

        let (rec_tx, rec_rx) = mpsc::channel();
        worker
            .start_recording(Box::new(ChannelWriter(rec_tx)))
            .expect("recording starts");
        run_for(&mut worker, &clock, 2_000);
        worker.stop_recording().expect("recording stops");
        let recorded = rows(&rec_rx);
        let rec_empty = leading_empty_then_filled(&temps(&recorded), "recording");

        let (tx, rx) = mpsc::channel();
        worker
            .export(Box::new(ChannelWriter(tx)), |_| {})
            .join()
            .expect("export thread")
            .expect("export");
        let exported = rows(&rx);
        assert_eq!(
            exported.len(),
            recorded.len(),
            "{rate:?}: export vs recording"
        );
        let ex_empty = leading_empty_then_filled(&temps(&exported), "export");
        assert_eq!(
            ex_empty, rec_empty,
            "{rate:?}: the export and the recording disagree on the first poll's rows"
        );
    }
}
