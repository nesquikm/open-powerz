//! Segment marks and E.D. from the raw samples (FR AXHK2T, AC-AXHK2T.4, .5).
//! Every identifier is synthetic.
//!
//! The chart used to derive both from the drawn min/max buckets, whose
//! trapezoid pairs one bucket's max with the next one's min: a bursty load
//! came out several times too high, and the marks moved with the plot width.
//! The backend now integrates the raw samples instead: marks over the current
//! run, E.D. over the whole session (or the run, when asked):
//!
//! - `history::RunProgressQuery { kind: MarkKind, step_pct: f64,
//!   ed_points: usize, ed_t0_ms: Option<u64>, ed_t1_ms: Option<u64>,
//!   ed_scope: EdScope }` (`Deserialize`, snake_case; `MarkKind` is
//!   `"charge" | "energy"`, `EdScope` is `"session" | "run"`, default session);
//! - `History::run_progress(&query) -> Option<RunProgress>` (also on
//!   `Records` and `OpenedFile`); `None` when the history is empty;
//! - `RunProgress { session_t0_ms, run_t0_ms, run_t1_ms, samples, charge_ah,
//!   energy_wh, marks_total_positive, marks: Vec<RunMark { t_ms, percent }>,
//!   ed_total_wh, energy_total_positive, ed_t_ms: Vec<f64>, ed_pct: Vec<f64> }`;
//! - the Tauri commands `run_progress` (optional `device_id`) and
//!   `recording_file_run_progress`, both registered.

use std::path::PathBuf;

use open_powerz_desktop_lib::history::{
    EdScope, History, MarkKind, RunProgress, RunProgressQuery, Series,
};
use open_powerz_desktop_lib::recording_file::{self, FileError, OpenControl, OpenedFile};
use powerz_protocol::decode::{QueueRecord, Rate};
use powerz_record::{CsvRecorder, Integrator};

fn rec(time_ms: u32, vbus_v: f64, ibus_a: f64) -> QueueRecord {
    QueueRecord::from_raw(
        time_ms,
        (vbus_v * 1e6).round() as i32,
        (ibus_a * 1e6).round() as i32,
        [1680, 4, 600, 600],
        Rate::Sps1000,
    )
}

fn history_of(records: &[QueueRecord]) -> History {
    let mut h = History::new();
    for r in records {
        h.push(r);
    }
    h
}

fn query(kind: MarkKind, step_pct: f64, ed_points: usize) -> RunProgressQuery {
    RunProgressQuery {
        kind,
        step_pct,
        ed_points,
        ed_t0_ms: None,
        ed_t1_ms: None,
        ed_scope: EdScope::Session,
    }
}

fn progress(h: &History, q: &RunProgressQuery) -> RunProgress {
    h.run_progress(q).expect("a run")
}

fn close(a: f64, b: f64, abs: f64, what: &str) {
    assert!(
        (a - b).abs() <= abs,
        "{what}: {a} against {b} (more than {abs} apart)"
    );
}

/// A constant 5 V / 1 A load, one record a second, 0 s ..= 1000 s.
fn constant() -> Vec<QueueRecord> {
    (0..=1000u32).map(|s| rec(s * 1000, 5.0, 1.0)).collect()
}

/// Independent reference: where the running charge (or energy) of `records`
/// first reaches each `percent` of its total, linearly between records.
fn reference_marks(records: &[QueueRecord], kind: MarkKind, percents: &[f64]) -> Vec<f64> {
    let mut integ = Integrator::new();
    let run: Vec<(f64, f64)> = records
        .iter()
        .map(|r| {
            let (ah, wh) = integ.push(r.time_ms(), r.vbus_v(), r.ibus_a());
            (
                f64::from(r.time_ms()),
                if kind == MarkKind::Charge { ah } else { wh },
            )
        })
        .collect();
    let total = run.last().map_or(0.0, |&(_, c)| c);
    percents
        .iter()
        .filter_map(|p| {
            let target = total * p / 100.0;
            let k = run.iter().position(|&(_, c)| c >= target)?;
            let (t1, c1) = run[k];
            let Some(&(t0, c0)) = k.checked_sub(1).and_then(|j| run.get(j)) else {
                return Some(t1);
            };
            Some(t0 + (target - c0) / (c1 - c0) * (t1 - t0))
        })
        .collect()
}

#[test]
fn a_constant_load_marks_every_tenth_of_its_charge_and_energy() {
    let h = history_of(&constant());
    for kind in [MarkKind::Charge, MarkKind::Energy] {
        let p = progress(&h, &query(kind, 10.0, 0));
        assert!(p.marks_total_positive);
        let pct: Vec<f64> = p.marks.iter().map(|m| m.percent).collect();
        assert_eq!(
            pct,
            [10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0],
            "0% and 100% are not marked"
        );
        for m in &p.marks {
            close(
                m.t_ms,
                m.percent * 10_000.0,
                1e-6,
                &format!("{kind:?} {}%", m.percent),
            );
        }
    }
    let p = progress(&h, &query(MarkKind::Charge, 25.0, 0));
    let pct: Vec<f64> = p.marks.iter().map(|m| m.percent).collect();
    assert_eq!(pct, [25.0, 50.0, 75.0]);
}

#[test]
fn the_run_totals_come_from_the_raw_samples() {
    let p = progress(&history_of(&constant()), &query(MarkKind::Charge, 10.0, 0));
    assert_eq!(p.samples, 1001);
    assert_eq!((p.run_t0_ms, p.run_t1_ms), (0, 1_000_000));
    close(p.charge_ah, 1000.0 / 3600.0, 1e-12, "charge");
    close(p.energy_wh, 5000.0 / 3600.0, 1e-12, "energy");
}

#[test]
fn energy_share_is_sampled_at_evenly_spaced_times() {
    let h = history_of(&constant());
    let p = progress(&h, &query(MarkKind::Energy, 10.0, 11));
    assert!(p.energy_total_positive);
    assert_eq!(p.ed_t_ms.len(), 11);
    assert_eq!(p.ed_pct.len(), 11);
    for (k, (&t, &pct)) in p.ed_t_ms.iter().zip(&p.ed_pct).enumerate() {
        close(t, k as f64 * 100_000.0, 1e-9, "time");
        close(pct, k as f64 * 10.0, 1e-9, &format!("share at {t} ms"));
    }
    // Between records: halfway through the first second is 0.05%.
    let mut q = query(MarkKind::Energy, 10.0, 2);
    q.ed_t0_ms = Some(500);
    q.ed_t1_ms = Some(500_000);
    let p = progress(&h, &q);
    close(p.ed_t_ms[0], 500.0, 1e-9, "first time");
    close(p.ed_pct[0], 0.05, 1e-9, "share inside the first interval");
    close(p.ed_pct[1], 50.0, 1e-9, "share at the window's end");
}

#[test]
fn the_energy_share_window_is_clamped_to_the_run_and_capped() {
    let h = history_of(&constant());
    let mut q = query(MarkKind::Energy, 10.0, 3);
    // Reversed and past the run's end.
    q.ed_t0_ms = Some(5_000_000);
    q.ed_t1_ms = Some(200_000);
    let p = progress(&h, &q);
    assert_eq!(p.ed_t_ms, [200_000.0, 600_000.0, 1_000_000.0]);
    // A window outside the run has no points.
    q.ed_t0_ms = Some(2_000_000);
    q.ed_t1_ms = Some(3_000_000);
    assert!(progress(&h, &q).ed_t_ms.is_empty());
    // No points asked, none given; an absurd count is capped.
    assert!(
        progress(&h, &query(MarkKind::Energy, 10.0, 0))
            .ed_t_ms
            .is_empty()
    );
    let many = progress(&h, &query(MarkKind::Energy, 10.0, 10_000_000));
    assert_eq!(
        many.ed_t_ms.len(),
        open_powerz_desktop_lib::history::MAX_BUCKETS
    );
    assert_eq!(many.ed_pct.len(), many.ed_t_ms.len());
}

#[test]
fn a_bursty_load_is_marked_from_its_samples_not_its_bucket_envelope() {
    // 10 ms records, 0 A between rare 10 A spikes: sparse in the first half,
    // four times denser in the second.
    let records: Vec<QueueRecord> = (0..40_000u32)
        .map(|k| {
            let every = if k < 20_000 { 200 } else { 50 };
            rec(k * 10, 5.0, if k % every == 0 { 10.0 } else { 0.0 })
        })
        .collect();
    let h = history_of(&records);
    let p = progress(&h, &query(MarkKind::Charge, 10.0, 0));

    // What the chart did: the trapezoid over the min-then-max envelope of a
    // plot-wide bucket frame.
    let drawn = h.range(&[Series::Ibus], 0, 400_000, 600);
    let mut envelope = Vec::new();
    for b in &drawn[0].buckets {
        envelope.push((b.t_ms as f64, f64::from(b.min)));
        envelope.push((b.t_ms as f64, f64::from(b.max)));
    }
    let envelope_ah: f64 = envelope
        .windows(2)
        .map(|w| (w[0].1 + w[1].1) / 2.0 * (w[1].0 - w[0].0) / 3_600_000.0)
        .sum();
    assert!(
        envelope_ah > 3.0 * p.charge_ah,
        "the vector must tell the envelope ({envelope_ah}) from the samples ({})",
        p.charge_ah
    );

    let (want, _) = {
        let mut i = Integrator::new();
        let mut out = (0.0, 0.0);
        for r in &records {
            out = i.push(r.time_ms(), r.vbus_v(), r.ibus_a());
        }
        out
    };
    close(p.charge_ah, want, 1e-12, "charge");
    let percents: Vec<f64> = (1..10).map(|k| f64::from(k) * 10.0).collect();
    let want = reference_marks(&records, MarkKind::Charge, &percents);
    assert_eq!(p.marks.len(), want.len());
    for (m, w) in p.marks.iter().zip(&want) {
        close(m.t_ms, *w, 1e-6, &format!("{}% mark", m.percent));
    }
    // The sparse first half holds only about 20% of the charge: the 20% mark
    // sits near its end (about 200 s), where a time-proportional guess would
    // put it at 80 s.
    assert!(
        p.marks[1].t_ms > 190_000.0,
        "20% mark at {}",
        p.marks[1].t_ms
    );
}

#[test]
fn a_mark_is_where_the_running_total_first_reaches_its_step() {
    // 2 A for 100 s, -1 A for 100 s, then 1 A for 100 s: the running charge
    // reaches its final value at 100 s, falls back, and climbs again.
    let records: Vec<QueueRecord> = (0..=300u32)
        .map(|s| {
            let a = match s {
                0..=100 => 2.0,
                101..=200 => -1.0,
                _ => 1.0,
            };
            rec(s * 1000, 5.0, a)
        })
        .collect();
    let h = history_of(&records);
    let p = progress(&h, &query(MarkKind::Charge, 10.0, 0));
    let percents: Vec<f64> = (1..10).map(|k| f64::from(k) * 10.0).collect();
    let want = reference_marks(&records, MarkKind::Charge, &percents);
    assert_eq!(p.marks.len(), 9);
    for (m, w) in p.marks.iter().zip(&want) {
        close(m.t_ms, *w, 1e-6, &format!("{}% mark", m.percent));
        assert!(
            m.t_ms <= 100_000.0,
            "{}% marked on the second climb",
            m.percent
        );
    }
}

#[test]
fn only_the_current_run_counts() {
    let mut h = History::new();
    for s in 0..=1000u32 {
        h.push(&rec(s * 1000, 5.0, 3.0));
    }
    h.break_run();
    let second: Vec<QueueRecord> = (2000..=3000u32).map(|s| rec(s * 1000, 5.0, 1.0)).collect();
    for r in &second {
        h.push(r);
    }
    let mut q = query(MarkKind::Charge, 10.0, 11);
    q.ed_t0_ms = Some(0);
    q.ed_scope = EdScope::Run;
    let p = progress(&h, &q);
    assert_eq!(p.samples, 1001);
    assert_eq!((p.run_t0_ms, p.run_t1_ms), (2_000_000, 3_000_000));
    close(
        p.charge_ah,
        1000.0 / 3600.0,
        1e-12,
        "charge of the current run",
    );
    for m in &p.marks {
        close(
            m.t_ms,
            2_000_000.0 + m.percent * 10_000.0,
            1e-6,
            &format!("{}%", m.percent),
        );
    }
    // The E.D. window starts at the run, however early it was asked for.
    close(p.ed_t_ms[0], 2_000_000.0, 1e-9, "first E.D. time");
    close(p.ed_pct[5], 50.0, 1e-9, "E.D. halfway");

    // A break with no record after it yet does not empty the run.
    h.break_run();
    assert_eq!(progress(&h, &q).samples, 1001);
}

/// 5 V / 3 A for 1000 s, a run break, then 5 V / 1 A for 1000 s from 2000 s.
fn two_runs(second_a: f64) -> History {
    let mut h = History::new();
    for s in 0..=1000u32 {
        h.push(&rec(s * 1000, 5.0, 3.0));
    }
    h.break_run();
    for s in 2000..=3000u32 {
        h.push(&rec(s * 1000, 5.0, second_a));
    }
    h
}

#[test]
fn energy_share_is_of_the_session_total_by_default() {
    // AC-AXHK2T.5: E.D. is a share of the session's energy (15 kJ + 5 kJ), summed over
    // its runs and flat across the break; the marks still follow the current run (.4).
    let h = two_runs(1.0);
    let p = progress(&h, &query(MarkKind::Charge, 10.0, 7));
    assert!(p.energy_total_positive);
    close(p.ed_total_wh, 20_000.0 / 3600.0, 1e-9, "session energy");
    assert_eq!(
        p.ed_t_ms,
        [
            0.0,
            500_000.0,
            1_000_000.0,
            1_500_000.0,
            2_000_000.0,
            2_500_000.0,
            3_000_000.0
        ]
    );
    for (got, want) in p
        .ed_pct
        .iter()
        .zip([0.0, 37.5, 75.0, 75.0, 75.0, 87.5, 100.0])
    {
        close(*got, want, 1e-9, "session share");
    }
    // The run's own totals and marks are unchanged; the session's first record is
    // reported too.
    assert_eq!(p.session_t0_ms, 0);
    assert_eq!((p.run_t0_ms, p.run_t1_ms), (2_000_000, 3_000_000));
    close(p.energy_wh, 5000.0 / 3600.0, 1e-9, "run energy");
    assert_eq!(p.marks.len(), 9);
    close(p.marks[4].t_ms, 2_500_000.0, 1e-6, "50% of the run");
    // The window is clamped to the session, not the run.
    let mut q = query(MarkKind::Charge, 10.0, 2);
    q.ed_t0_ms = Some(500_000);
    let p = progress(&h, &q);
    assert_eq!(p.ed_t_ms, [500_000.0, 3_000_000.0]);

    // Asked for the run, E.D. is a share of the run's energy.
    let mut q = query(MarkKind::Charge, 10.0, 3);
    q.ed_scope = EdScope::Run;
    let p = progress(&h, &q);
    close(
        p.ed_total_wh,
        5000.0 / 3600.0,
        1e-9,
        "run energy as the E.D. total",
    );
    assert_eq!(p.ed_t_ms, [2_000_000.0, 2_500_000.0, 3_000_000.0]);
    close(p.ed_pct[1], 50.0, 1e-9, "run share");
}

#[test]
fn a_discharging_run_still_has_a_session_energy_share() {
    let h = two_runs(-1.0);
    let p = progress(&h, &query(MarkKind::Charge, 10.0, 3));
    assert!(
        !p.marks_total_positive && p.marks.is_empty(),
        "no run marks"
    );
    assert!(p.energy_total_positive, "the session's energy is positive");
    close(p.ed_total_wh, 10_000.0 / 3600.0, 1e-9, "15 kJ - 5 kJ");
    close(p.ed_pct[2], 100.0, 1e-9, "the end is the whole session");
}

#[test]
fn a_total_that_is_not_positive_is_flagged_and_marks_nothing() {
    let discharge: Vec<QueueRecord> = (0..=100u32).map(|s| rec(s * 1000, 5.0, -1.0)).collect();
    let idle: Vec<QueueRecord> = (0..=100u32).map(|s| rec(s * 1000, 5.0, 0.0)).collect();
    let single = vec![rec(0, 5.0, 1.0)];
    for (name, records) in [
        ("discharge", discharge),
        ("idle", idle),
        ("one record", single),
    ] {
        let p = progress(&history_of(&records), &query(MarkKind::Charge, 10.0, 50));
        assert!(!p.marks_total_positive, "{name}: marks flag");
        assert!(p.marks.is_empty(), "{name}: marks");
        assert!(!p.energy_total_positive, "{name}: energy flag");
        assert!(p.ed_t_ms.is_empty() && p.ed_pct.is_empty(), "{name}: E.D.");
    }
    assert!(
        History::new()
            .run_progress(&query(MarkKind::Charge, 10.0, 50))
            .is_none()
    );
}

#[test]
fn a_step_out_of_range_marks_nothing() {
    let h = history_of(&constant());
    for step in [0.0, -10.0, 100.0, 150.0, f64::NAN] {
        let p = progress(&h, &query(MarkKind::Charge, step, 0));
        assert!(p.marks.is_empty(), "step {step}");
        assert!(p.marks_total_positive, "the total is still positive");
    }
}

#[test]
fn the_marks_do_not_depend_on_the_energy_share_point_count() {
    // The chart's width sets the E.D. point count; the marks must not move.
    let h = history_of(&constant());
    let a = progress(&h, &query(MarkKind::Energy, 5.0, 300));
    let b = progress(&h, &query(MarkKind::Energy, 5.0, 1_700));
    assert_eq!(a.marks, b.marks);
}

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("run-progress")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn opened(result: Result<OpenedFile, FileError>) -> OpenedFile {
    match result {
        Ok(file) => file,
        Err(FileError::Refused(why)) => panic!("refused: {why}"),
        Err(FileError::Cancelled) => panic!("cancelled"),
    }
}

#[test]
fn an_opened_recording_is_marked_from_its_rows() {
    let dir = scratch("opened");
    let path = dir.join("run.csv");
    let mut csv = CsvRecorder::new(Vec::new()).expect("header");
    for r in constant() {
        csv.push_with_temp(&r, 1_790_000_000_000, None)
            .expect("row");
    }
    std::fs::write(&path, csv.finish().expect("flushed")).expect("csv");
    let file = opened(recording_file::open_csv(&path, &mut OpenControl::silent()));
    let p = file
        .run_progress(&query(MarkKind::Charge, 10.0, 11))
        .expect("a run");
    assert_eq!(p.marks.len(), 9);
    close(p.marks[4].t_ms, 500_000.0, 1.0, "50% mark");
    close(p.ed_pct[10], 100.0, 1e-9, "E.D. at the end");
}

#[test]
fn run_progress_serializes_with_its_field_names() {
    let p = progress(&history_of(&constant()), &query(MarkKind::Charge, 10.0, 3));
    let json = serde_json::to_value(&p).expect("json");
    for key in [
        "session_t0_ms",
        "run_t0_ms",
        "run_t1_ms",
        "samples",
        "charge_ah",
        "energy_wh",
        "marks_total_positive",
        "marks",
        "energy_total_positive",
        "ed_total_wh",
        "ed_t_ms",
        "ed_pct",
    ] {
        assert!(json.get(key).is_some(), "{key} missing from {json}");
    }
    assert!(json["marks"][0].get("t_ms").is_some() && json["marks"][0].get("percent").is_some());
    let q: RunProgressQuery = serde_json::from_value(serde_json::json!({
        "kind": "energy", "step_pct": 20, "ed_points": 5, "ed_t0_ms": null
    }))
    .expect("a query from the UI");
    assert_eq!(q.kind, MarkKind::Energy);
    assert_eq!(q.ed_t1_ms, None);
    assert_eq!(q.ed_scope, EdScope::Session, "the session total by default");
    let q: RunProgressQuery = serde_json::from_value(serde_json::json!({
        "kind": "charge", "step_pct": 10, "ed_points": 5, "ed_scope": "run"
    }))
    .expect("a run-scoped query");
    assert_eq!(q.ed_scope, EdScope::Run);
}

#[test]
fn both_run_progress_commands_are_registered() {
    let source = include_str!("../src/lib.rs");
    let handlers = source
        .split("generate_handler![")
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .expect("the handler list");
    for name in ["run_progress", "recording_file_run_progress"] {
        assert!(
            handlers.split(',').any(|h| h.trim() == name),
            "{name} is not registered"
        );
    }
}
