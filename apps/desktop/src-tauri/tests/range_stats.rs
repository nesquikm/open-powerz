//! The ruler's backend calculation (FR AXHK2T, AC-AXHK2T.2): charge, energy and
//! averages between two times, integrated over the raw samples of a session's
//! history or of an opened recording, never over the chart's drawn buckets.
//! Every identifier is synthetic.
//!
//! API pinned here, beyond the FR's Technical Design:
//!
//! - `history::RangeStats { samples: u64, charge_ah: f64, energy_wh: f64,
//!   vbus_avg_v: f64, ibus_avg_a: f64, power_avg_w: f64 }` (`Serialize`, so a
//!   Tauri command can answer it as is);
//! - `History::range_stats(t0_ms, t1_ms) -> Option<RangeStats>` over the records
//!   with `t0_ms <= time <= t1_ms`: charge and energy by the trapezoid rule
//!   between neighbouring records of that range, as `powerz_record::Integrator`
//!   counts them, never across a run break; `None` when no record is in range;
//! - `OpenedFile::range_stats(t0_ms, t1_ms)`: the same over an opened file;
//! - the Tauri commands `range_stats` (with an optional `device_id`, like
//!   `history_range`) and `recording_file_range_stats`, both registered.

use std::path::PathBuf;

use open_powerz_desktop_lib::history::{History, RangeStats, Series};
use open_powerz_desktop_lib::recording_file::{self, FileError, OpenControl, OpenedFile};
use powerz_protocol::decode::{QueueRecord, Rate};
use powerz_record::{CsvRecorder, Integrator};

/// A record at `time_ms` with `vbus_v` volts and `ibus_a` amperes.
fn rec(time_ms: u32, vbus_v: f64, ibus_a: f64) -> QueueRecord {
    QueueRecord::from_raw(
        time_ms,
        (vbus_v * 1e6).round() as i32,
        (ibus_a * 1e6).round() as i32,
        [1680, 4, 600, 600],
        Rate::Sps1000,
    )
}

/// A history of `records`, in order.
fn history_of(records: &[QueueRecord]) -> History {
    let mut h = History::new();
    for r in records {
        h.push(r);
    }
    h
}

/// A constant 5 V / 1 A load, one record a second for an hour (3601 records).
fn constant_hour() -> Vec<QueueRecord> {
    (0..=3600u32).map(|s| rec(s * 1000, 5.0, 1.0)).collect()
}

fn stats(h: &History, t0: u64, t1: u64) -> RangeStats {
    h.range_stats(t0, t1)
        .unwrap_or_else(|| panic!("no stats over {t0}..={t1}"))
}

/// `a` within `rel` (relative) of `b`.
fn close(a: f64, b: f64, rel: f64, what: &str) {
    let scale = b.abs().max(1e-12);
    assert!(
        (a - b).abs() / scale <= rel,
        "{what}: {a} against {b} (more than {rel} apart)"
    );
}

/// Reference charge and energy of `records` with `t0 <= time <= t1`: the
/// trapezoid rule as the integrator counts it, over the raw values.
fn reference(records: &[QueueRecord], t0: u64, t1: u64) -> (f64, f64) {
    let mut i = Integrator::new();
    let mut out = (0.0, 0.0);
    for r in records
        .iter()
        .filter(|r| (t0..=t1).contains(&u64::from(r.time_ms())))
    {
        out = i.push(r.time_ms(), r.vbus_v(), r.ibus_a());
    }
    out
}

#[test]
fn a_constant_5_v_1_a_hour_is_1_ah_and_5_wh() {
    let h = history_of(&constant_hour());
    let s = stats(&h, 0, 3_600_000);
    assert_eq!(s.samples, 3601);
    close(s.charge_ah, 1.0, 1e-9, "charge");
    close(s.energy_wh, 5.0, 1e-9, "energy");
    close(s.vbus_avg_v, 5.0, 1e-9, "average V");
    close(s.ibus_avg_a, 1.0, 1e-9, "average A");
    close(s.power_avg_w, 5.0, 1e-9, "average W");
}

#[test]
fn only_the_records_between_the_two_times_count() {
    let h = history_of(&constant_hour());
    // 10:00 to 30:00: 1200 s of the hour, 1201 records.
    let s = stats(&h, 600_000, 1_800_000);
    assert_eq!(s.samples, 1201);
    close(s.charge_ah, 1200.0 / 3600.0, 1e-9, "charge");
    close(s.energy_wh, 5.0 * 1200.0 / 3600.0, 1e-9, "energy");
}

#[test]
fn the_ends_may_fall_between_records() {
    let h = history_of(&constant_hour());
    // 0.5 s past one record to 0.5 s before another: records 1 s .. 9 s.
    let s = stats(&h, 500, 9_500);
    assert_eq!(s.samples, 9);
    // Superseded: this used to pin 8 s (records 1 s .. 9 s only). The two
    // half-second edge parts now count too, interpolated towards the records
    // just outside, so ΔAh is ΔT (9 s) × the average current.
    close(s.charge_ah, 9.0 / 3600.0, 1e-9, "charge");
    close(s.energy_wh, 5.0 * 9.0 / 3600.0, 1e-9, "energy");
}

#[test]
fn charge_and_energy_agree_with_the_time_between_the_cursors_at_low_rates() {
    // 1 SPS and 10 SPS: the cursors rarely sit on a record, and the parts
    // between each cursor and its nearest record are a large share of ΔT.
    for step_ms in [1_000u32, 100] {
        let records: Vec<QueueRecord> = (0..=200u32).map(|k| rec(k * step_ms, 5.0, 2.0)).collect();
        let h = history_of(&records);
        for (t0, t1) in [(250u64, 7_750u64), (1_050, 2_150), (3_333, 19_999)] {
            let s = stats(&h, t0, t1);
            let hours = (t1 - t0) as f64 / 3_600_000.0;
            close(
                s.charge_ah,
                hours * s.ibus_avg_a,
                1e-9,
                &format!("ΔAh {step_ms} ms {t0}..{t1}"),
            );
            close(
                s.energy_wh,
                hours * s.power_avg_w,
                1e-9,
                &format!("ΔWh {step_ms} ms {t0}..{t1}"),
            );
        }
    }
}

#[test]
fn one_record_between_its_neighbours_has_charge() {
    let h = history_of(&constant_hour());
    // Only the record at 2 s is in range; half a second on each side of it.
    let s = stats(&h, 1_500, 2_500);
    assert_eq!(s.samples, 1);
    close(s.charge_ah, 1.0 / 3600.0, 1e-9, "charge");
    close(s.energy_wh, 5.0 / 3600.0, 1e-9, "energy");
}

#[test]
fn the_edge_parts_follow_the_line_between_the_neighbouring_records() {
    // 1 V and a current of t amperes at second t: the trapezoid rule is exact
    // on a straight line, so 0.5 s .. 2.5 s holds (2.5² − 0.5²) / 2 = 3 A·s.
    let records: Vec<QueueRecord> = (0..=10u32)
        .map(|k| rec(k * 1000, 1.0, f64::from(k)))
        .collect();
    let h = history_of(&records);
    let s = stats(&h, 500, 2_500);
    close(s.charge_ah, 3.0 / 3600.0, 1e-9, "charge");
    close(s.energy_wh, 3.0 / 3600.0, 1e-9, "energy");
}

#[test]
fn neighbouring_ranges_add_up_to_the_whole() {
    let records: Vec<QueueRecord> = (0..=100u32)
        .map(|k| {
            rec(
                k * 1000,
                9.0 + f64::from(k % 3),
                0.5 + f64::from(k % 7) / 4.0,
            )
        })
        .collect();
    let h = history_of(&records);
    let whole = stats(&h, 2_300, 77_700);
    let a = stats(&h, 2_300, 40_400);
    let b = stats(&h, 40_400, 77_700);
    close(a.charge_ah + b.charge_ah, whole.charge_ah, 1e-9, "charge");
    close(a.energy_wh + b.energy_wh, whole.energy_wh, 1e-9, "energy");
}

#[test]
fn the_edge_parts_never_cross_a_run_break() {
    let mut h = History::new();
    for s in 0..=10u32 {
        h.push(&rec(s * 1000, 5.0, 1.0));
    }
    // Nothing was measured from 10 s to 20 s.
    h.break_run();
    for s in 20..=30u32 {
        h.push(&rec(s * 1000, 5.0, 1.0));
    }
    // Starts in the gap: no part before the run's first record.
    close(
        stats(&h, 15_000, 25_000).charge_ah,
        5.0 / 3600.0,
        1e-9,
        "from the gap",
    );
    // Ends in the gap: no part after the earlier run's last record.
    close(
        stats(&h, 5_500, 15_000).charge_ah,
        4.5 / 3600.0,
        1e-9,
        "into the gap",
    );
    // Spans it: both outer edge parts, nothing over the gap itself.
    close(
        stats(&h, 5_500, 20_500).charge_ah,
        5.0 / 3600.0,
        1e-9,
        "across the gap",
    );
}

#[test]
fn the_values_come_from_the_raw_samples_not_the_drawn_buckets() {
    // Mostly 0 A with a 10 A spike every 100th record: each drawn bucket's
    // min/max spans 0..10 A, so anything read from buckets lands near 5 A,
    // while the samples average 0.1 A.
    let records: Vec<QueueRecord> = (0..20_000u32)
        .map(|k| {
            let v = 5.0 + 0.25 * f64::from(k % 7) / 7.0;
            let i = if k % 100 == 0 { 10.0 } else { 0.0 };
            rec(k * 10, v, i)
        })
        .collect();
    let h = history_of(&records);
    let (t0, t1) = (10_000u64, 150_000u64);

    // The vector discriminates: the drawn buckets' midpoints are far off.
    let drawn = h.range(&[Series::Ibus], t0, t1, 64);
    let buckets = &drawn[0].buckets;
    assert!(!buckets.is_empty());
    let bucket_mean = buckets
        .iter()
        .map(|b| (f64::from(b.min) + f64::from(b.max)) / 2.0)
        .sum::<f64>()
        / buckets.len() as f64;
    let (want_ah, want_wh) = reference(&records, t0, t1);
    let hours = (t1 - t0) as f64 / 3_600_000.0;
    assert!(
        (bucket_mean * hours - want_ah).abs() / want_ah > 0.5,
        "the test vector must tell buckets from samples"
    );

    let s = stats(&h, t0, t1);
    close(s.charge_ah, want_ah, 1e-3, "charge (within 0.1%)");
    close(s.energy_wh, want_wh, 1e-3, "energy (within 0.1%)");
    let in_range: Vec<&QueueRecord> = records
        .iter()
        .filter(|r| (t0..=t1).contains(&u64::from(r.time_ms())))
        .collect();
    let n = in_range.len() as f64;
    assert_eq!(s.samples, in_range.len() as u64);
    let mean = |f: &dyn Fn(&QueueRecord) -> f64| in_range.iter().map(|r| f(r)).sum::<f64>() / n;
    close(s.ibus_avg_a, mean(&|r| r.ibus_a()), 1e-3, "average A");
    close(s.vbus_avg_v, mean(&|r| r.vbus_v()), 1e-3, "average V");
    close(
        s.power_avg_w,
        mean(&|r| r.vbus_v() * r.ibus_a()),
        1e-3,
        "average W",
    );
}

#[test]
fn a_ramp_matches_the_trapezoid_reference_within_a_tenth_of_a_percent() {
    let records: Vec<QueueRecord> = (0..=36_000u32)
        .map(|k| {
            let t = f64::from(k) / 10.0;
            rec(
                k * 100,
                9.0 + 0.5 * (t / 50.0).sin(),
                0.2 + t / 3600.0 * 2.8,
            )
        })
        .collect();
    let h = history_of(&records);
    for (t0, t1) in [
        (0u64, 3_600_000u64),
        (123_400, 2_345_600),
        (1_000_000, 1_000_900),
    ] {
        let (want_ah, want_wh) = reference(&records, t0, t1);
        let s = stats(&h, t0, t1);
        close(s.charge_ah, want_ah, 1e-3, &format!("charge {t0}..{t1}"));
        close(s.energy_wh, want_wh, 1e-3, &format!("energy {t0}..{t1}"));
    }
}

#[test]
fn nothing_is_counted_across_a_run_break() {
    let mut h = History::new();
    for s in 0..=1000u32 {
        h.push(&rec(s * 1000, 5.0, 1.0));
    }
    // Nothing was measured from 1000 s to 2000 s.
    h.break_run();
    for s in 2000..=3000u32 {
        h.push(&rec(s * 1000, 5.0, 1.0));
    }
    let s = stats(&h, 0, 3_000_000);
    assert_eq!(s.samples, 2002);
    close(s.charge_ah, 2000.0 / 3600.0, 1e-9, "charge");
    close(s.energy_wh, 5.0 * 2000.0 / 3600.0, 1e-9, "energy");
}

#[test]
fn a_range_with_no_record_has_no_stats() {
    let h = history_of(&constant_hour());
    assert_eq!(h.range_stats(4_000_000, 5_000_000), None);
    assert_eq!(h.range_stats(1_200, 1_800), None);
    assert_eq!(History::new().range_stats(0, 1_000), None);
}

#[test]
fn reversed_times_are_the_same_range() {
    // The ruler's T2 may sit left of T1.
    let h = history_of(&constant_hour());
    assert_eq!(
        h.range_stats(1_800_000, 600_000),
        h.range_stats(600_000, 1_800_000)
    );
}

#[test]
fn one_record_has_its_own_values_and_no_charge() {
    let h = history_of(&constant_hour());
    let s = stats(&h, 2_000, 2_000);
    assert_eq!(s.samples, 1);
    assert_eq!(s.charge_ah, 0.0);
    assert_eq!(s.energy_wh, 0.0);
    close(s.vbus_avg_v, 5.0, 1e-9, "average V");
    close(s.ibus_avg_a, 1.0, 1e-9, "average A");
}

#[test]
fn negative_current_keeps_its_sign() {
    let records: Vec<QueueRecord> = (0..=3600u32).map(|s| rec(s * 1000, 5.0, -0.5)).collect();
    let s = stats(&history_of(&records), 0, 3_600_000);
    close(s.charge_ah, -0.5, 1e-9, "charge");
    close(s.energy_wh, -2.5, 1e-9, "energy");
    close(s.ibus_avg_a, -0.5, 1e-9, "average A");
}

/// A fresh scratch directory for one test.
fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("range-stats")
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
fn an_opened_recording_answers_the_ruler_from_its_rows() {
    let dir = scratch("opened");
    let path = dir.join("hour.csv");
    let mut csv = CsvRecorder::new(Vec::new()).expect("header");
    for r in constant_hour() {
        csv.push_with_temp(&r, 1_790_000_000_000, Some(25.5))
            .expect("row");
    }
    std::fs::write(&path, csv.finish().expect("flushed")).expect("csv");

    let file = opened(recording_file::open_csv(&path, &mut OpenControl::silent()));
    let s = file
        .range_stats(0, 3_600_000)
        .expect("stats over the whole file");
    assert_eq!(s.samples, 3601);
    close(s.charge_ah, 1.0, 1e-6, "charge");
    close(s.energy_wh, 5.0, 1e-6, "energy");
    close(s.power_avg_w, 5.0, 1e-6, "average W");

    let part = file.range_stats(600_000, 1_800_000).expect("stats");
    close(part.charge_ah, 1200.0 / 3600.0, 1e-6, "charge of a part");
    assert_eq!(file.range_stats(4_000_000, 5_000_000), None);
}

#[test]
fn range_stats_serializes_with_its_field_names() {
    let s = stats(&history_of(&constant_hour()), 0, 3_600_000);
    let json = serde_json::to_value(s).expect("json");
    for key in [
        "samples",
        "charge_ah",
        "energy_wh",
        "vbus_avg_v",
        "ibus_avg_a",
        "power_avg_w",
    ] {
        assert!(json.get(key).is_some(), "{key} missing from {json}");
    }
}

#[test]
fn both_ruler_commands_are_registered() {
    let source = include_str!("../src/lib.rs");
    let handlers = source
        .split("generate_handler![")
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .expect("the handler list");
    for name in ["range_stats", "recording_file_range_stats"] {
        assert!(
            handlers.split(',').any(|h| h.trim() == name),
            "{name} is not registered"
        );
    }
}
