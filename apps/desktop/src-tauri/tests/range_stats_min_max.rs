//! The range stats' minimum and maximum (FR PBDEQE, AC-PBDEQE.1): VBUS, IBUS
//! and Power over the records in range, beside the averages, so the File tab
//! can show the stats of the range on screen. A folded block overlapping the
//! range counts whole, with its kept minimum and maximum. Every identifier is
//! synthetic.
//!
//! API pinned here, beyond the FR's Technical Design:
//!
//! - `history::RangeStats` gains `vbus_min_v`, `vbus_max_v`, `ibus_min_a`,
//!   `ibus_max_a`, `power_min_w`, `power_max_w` (f64, serialized under those
//!   names).

use open_powerz_desktop_lib::history::{History, RangeStats};
use powerz_protocol::decode::{QueueRecord, Rate};

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

fn history_of(records: &[QueueRecord]) -> History {
    let mut h = History::new();
    for r in records {
        h.push(r);
    }
    h
}

fn stats(h: &History, t0: u64, t1: u64) -> RangeStats {
    h.range_stats(t0, t1)
        .unwrap_or_else(|| panic!("no stats over {t0}..={t1}"))
}

/// `a` within 1e-5 of `b` (a block keeps its min and max as f32).
fn near(a: f64, b: f64, what: &str) {
    assert!((a - b).abs() <= 1e-5, "{what}: {a} against {b}");
}

/// The six extremes of `s`, in field order.
fn extremes(s: &RangeStats) -> [f64; 6] {
    [
        s.vbus_min_v,
        s.vbus_max_v,
        s.ibus_min_a,
        s.ibus_max_a,
        s.power_min_w,
        s.power_max_w,
    ]
}

fn expect_extremes(s: &RangeStats, want: [f64; 6], what: &str) {
    let names = [
        "vbus min",
        "vbus max",
        "ibus min",
        "ibus max",
        "power min",
        "power max",
    ];
    for ((got, want), name) in extremes(s).into_iter().zip(want).zip(names) {
        near(got, want, &format!("{what}: {name}"));
    }
}

/// Eleven seconds at 5 V / 1 A, one record a second, with a VBUS dip to 4.5 V
/// at 3 s, a 2.5 A peak at 7 s and a reverse 0.5 A at 8 s.
fn plain() -> Vec<QueueRecord> {
    (0..=10u32)
        .map(|s| {
            let v = if s == 3 { 4.5 } else { 5.0 };
            let i = match s {
                7 => 2.5,
                8 => -0.5,
                _ => 1.0,
            };
            rec(s * 1000, v, i)
        })
        .collect()
}

#[test]
fn a_plain_range_answers_min_and_max_of_vbus_ibus_and_power() {
    let h = history_of(&plain());
    let s = stats(&h, 0, 10_000);
    // Power min is 5 V × −0.5 A, max 5 V × 2.5 A; the dip's 4.5 W is above −2.5.
    expect_extremes(&s, [4.5, 5.0, -0.5, 2.5, -2.5, 12.5], "whole");
    // The averages are still there, beside the extremes.
    near(s.vbus_avg_v, (10.0 * 5.0 + 4.5) / 11.0, "vbus avg");
}

#[test]
fn a_range_that_leaves_out_the_dip_has_the_range_minimum() {
    let h = history_of(&plain());
    let s = stats(&h, 4_000, 6_000);
    expect_extremes(&s, [5.0, 5.0, 1.0, 1.0, 5.0, 5.0], "4..6 s");
    // Either order.
    assert_eq!(h.range_stats(6_000, 4_000), Some(s));
}

#[test]
fn a_range_of_one_record_has_its_values_as_min_and_max() {
    let h = history_of(&plain());
    let s = stats(&h, 2_500, 3_500);
    assert_eq!(s.samples, 1);
    expect_extremes(&s, [4.5, 4.5, 1.0, 1.0, 4.5, 4.5], "the dip alone");
}

/// Step of the folded history's records, ms.
const STEP_MS: u32 = 100;
/// Records pushed: past one 65 536-record chunk plus the kept full part.
const FOLDED_RECORDS: u32 = 80_000;
/// Full-resolution records the folded history keeps.
const FULL: usize = 1_000;
/// The record with a VBUS dip and an IBUS peak, and the one with reverse current.
const DIP_AT: u32 = 100;
const REVERSE_AT: u32 = 200;

fn folded_records() -> Vec<QueueRecord> {
    (0..FOLDED_RECORDS)
        .map(|k| {
            let (v, i) = match k {
                DIP_AT => (3.5, 3.0),
                REVERSE_AT => (5.0, -1.0),
                _ => (5.0, 1.0),
            };
            rec(k * STEP_MS, v, i)
        })
        .collect()
}

fn folded() -> History {
    let mut h = History::with_full_limit(FULL);
    for r in folded_records() {
        h.push(&r);
    }
    let boundary = h.fold_boundary_ms().expect("the oldest chunk folded");
    assert!(
        boundary > u64::from(REVERSE_AT * STEP_MS),
        "the dip and the reverse record are folded (boundary {boundary})"
    );
    h
}

#[test]
fn a_folded_range_counts_the_blocks_kept_min_and_max() {
    let h = folded();
    let s = stats(&h, 0, u64::MAX);
    assert_eq!(s.samples, u64::from(FOLDED_RECORDS));
    // Power: 3.5 V × 3 A = 10.5 W at the dip, 5 V × −1 A = −5 W at the reverse.
    expect_extremes(
        &s,
        [3.5, 5.0, -1.0, 3.0, -5.0, 10.5],
        "whole folded history",
    );
}

#[test]
fn a_range_overlapping_a_block_counts_that_block_whole() {
    let h = folded();
    let dip_ms = u64::from(DIP_AT * STEP_MS);
    let (b0, b1) = h
        .fold_blocks_ms()
        .into_iter()
        .find(|&(b0, b1)| b0 <= dip_ms && dip_ms <= b1)
        .expect("a block holds the dip");
    // A range at the block's far end, away from the dip, still counts the block whole.
    let t = if b1 > dip_ms { b1 } else { b0 };
    let s = stats(&h, t, t);
    near(s.vbus_min_v, 3.5, "vbus min of the overlapped block");
    near(s.ibus_max_a, 3.0, "ibus max of the overlapped block");
    near(s.power_max_w, 10.5, "power max of the overlapped block");
}

#[test]
fn a_full_resolution_range_after_the_fold_leaves_the_folded_dip_out() {
    let h = folded();
    let boundary = h.fold_boundary_ms().expect("folded");
    let newest = u64::from((FOLDED_RECORDS - 1) * STEP_MS);
    let s = stats(&h, boundary + 1_000, newest);
    expect_extremes(&s, [5.0, 5.0, 1.0, 1.0, 5.0, 5.0], "full-resolution part");
}

#[test]
fn the_extremes_serialize_with_their_field_names() {
    let s = stats(&history_of(&plain()), 0, 10_000);
    let json = serde_json::to_value(s).expect("json");
    for (key, want) in [
        ("vbus_min_v", 4.5),
        ("vbus_max_v", 5.0),
        ("ibus_min_a", -0.5),
        ("ibus_max_a", 2.5),
        ("power_min_w", -2.5),
        ("power_max_w", 12.5),
    ] {
        let got = json
            .get(key)
            .and_then(serde_json::Value::as_f64)
            .unwrap_or_else(|| panic!("{key} missing from {json}"));
        near(got, want, key);
    }
}
