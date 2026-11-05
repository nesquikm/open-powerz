//! Folding the old part of a long session's history (FR AXHK2W,
//! AC-AXHK2W.10, .11, .12). Every identifier is synthetic.
//!
//! The history kept every sample at 20 B, so a 30-hour run at 1000 SPS
//! (108 M samples) would take over 2 GB. It now keeps full resolution only
//! for the newest samples and folds older chunks into blocks that keep, per
//! series, min, max, sum and count, plus the block's start and end time and
//! its charge and energy sums. Whole-session totals, the ruler at block
//! edges, segment marks and E.D. stay exact there; only placement inside a
//! block is lost.
//!
//! API pinned here, beyond the FR's Technical Design:
//!
//! - `history::HISTORY_FULL_SAMPLES: usize = 10_000_000`: what
//!   `History::new()` keeps at full resolution;
//! - `History::with_full_limit(n) -> History`: the same history with a
//!   smaller full-resolution limit (a test runs a 30-hour session in
//!   compressed time this way). It keeps at least the newest `n` records at
//!   full resolution and folds whole older chunks;
//! - `History::fold_boundary_ms(&self) -> Option<u64>`: the time where full
//!   resolution starts (data before it is coarser); `None` while nothing is
//!   folded. It is after every folded block and at or before the oldest of
//!   the newest `n` records;
//! - `History::fold_blocks_ms(&self) -> Vec<(u64, u64)>`: each folded block's
//!   first and last record time, oldest first (a block never spans a run
//!   break);
//! - `History::len()` stays the count of every record pushed, folded ones
//!   included, and `range_stats(..).samples` counts folded records too;
//! - `SessionStore::history_fold_boundary(&self) -> Option<u64>` and the
//!   Tauri command `history_fold_boundary` (optional `device_id`), registered,
//!   for the chart's tick (AC-AXHK2W.11).
//!
//! AC-AXHK2W.13 (the app's growth in a 20-minute tester run at 1000 SPS
//! matching the history's rate within 20%) is a tester runbook item, not an
//! automated test.

use open_powerz_desktop_lib::history::{
    EdScope, HISTORY_FULL_SAMPLES, History, MarkKind, RangeStats, RunProgress, RunProgressQuery,
    Series,
};
use open_powerz_desktop_lib::store::SessionStore;
use powerz_protocol::decode::{QueueRecord, Rate};

/// The full-resolution limit of the compressed run: the real limit over the
/// compression factor.
const COMPRESSION: usize = 100;
const FULL: usize = HISTORY_FULL_SAMPLES / COMPRESSION;
/// 1000 SPS compressed 100× is one record per 100 ms.
const STEP_MS: u32 = 100;
/// 30 hours of records at that step.
const SAMPLES: usize = 30 * 3600 * 1000 / STEP_MS as usize;
/// The record that starts a second measuring run, after a gap.
const BREAK_AT: usize = 400_000;
const BREAK_GAP_MS: u32 = 60_000;
/// Samples per history chunk (the unit that folds).
const CHUNK: usize = 65_536;

/// History memory ceiling for the compressed run, bytes.
///
/// - Full-resolution part: at most `FULL + 2 × CHUNK` samples allocated
///   (the kept newest `FULL`, the rest of the oldest kept chunk, and the open
///   chunk's capacity), at 20 B a sample for the columns plus the min/max
///   pyramid (7 × 8 B per 64 samples, × 4/3 for the levels above).
/// - Folded part: 1/4 B per folded sample (a block of a few thousand samples
///   costs well under 1 KB).
/// - 64 KiB for run lists and vector headroom.
///
/// Here: 231 072 × (20 + 1.17) + 980 000 / 4 + 65 536 ≈ 5.2 MB, against about
/// 23 MB unfolded. At full scale (10 M + 131 072 full, 98 M folded):
/// ≈ 214 MB + 24.5 MB ≈ 239 MB for 30 hours, against about 2.3 GB unfolded.
fn ceiling_bytes() -> usize {
    let full = FULL + 2 * CHUNK;
    let pyramid = full / 64 * 7 * 8 * 4 / 3;
    let folded = SAMPLES - FULL;
    full * 20 + pyramid + folded / 4 + 65_536
}

/// One synthetic record: an always-positive load whose voltage steps every
/// 37 000 records and whose current is noisy, so a block's sums matter.
struct Gen {
    seed: u64,
}

impl Gen {
    fn new() -> Gen {
        Gen {
            seed: 0x9e37_79b9_7f4a_7c15,
        }
    }

    fn noise(&mut self) -> f64 {
        self.seed = self
            .seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.seed >> 33) % 10_000) as f64 / 10_000.0
    }

    fn record(&mut self, i: usize) -> QueueRecord {
        let gap = if i >= BREAK_AT { BREAK_GAP_MS } else { 0 };
        let t = (i as u32) * STEP_MS + gap;
        let vbus_v = [5.0, 9.0, 15.0, 20.0, 12.0][(i / 37_000) % 5];
        let ibus_a = 0.3 + 2.7 * self.noise();
        QueueRecord::from_raw(
            t,
            (vbus_v * 1e6) as i32,
            (ibus_a * 1e6).round() as i32,
            [1680, 4, 600, 600],
            Rate::Sps1000,
        )
    }
}

/// The run's records, as both histories saw them, plus the running energy
/// (Wh, trapezoid rule, never across the break) at each record.
struct Run {
    folded: History,
    reference: History,
    t_ms: Vec<u64>,
    running_wh: Vec<f64>,
}

fn run() -> Run {
    let mut folded = History::with_full_limit(FULL);
    let mut reference = History::new();
    let mut g = Gen::new();
    let mut t_ms = Vec::with_capacity(SAMPLES);
    let mut running_wh = Vec::with_capacity(SAMPLES);
    let mut prev: Option<(f64, f64)> = None;
    let mut wh = 0.0;
    for i in 0..SAMPLES {
        if i == BREAK_AT {
            folded.break_run();
            reference.break_run();
            prev = None;
        }
        let r = g.record(i);
        folded.push(&r);
        reference.push(&r);
        let (t, p) = (f64::from(r.time_ms()), r.vbus_v() * r.ibus_a());
        if let Some((t0, p0)) = prev {
            wh += (p0 + p) / 2.0 * (t - t0) / 3_600_000.0;
        }
        prev = Some((t, p));
        t_ms.push(u64::from(r.time_ms()));
        running_wh.push(wh);
    }
    Run {
        folded,
        reference,
        t_ms,
        running_wh,
    }
}

fn close(got: f64, want: f64, what: &str) {
    let tol = 1e-9 * want.abs().max(1e-6);
    assert!(
        (got - want).abs() <= tol,
        "{what}: got {got}, want {want} (diff {})",
        (got - want).abs()
    );
}

fn same_stats(got: Option<RangeStats>, want: Option<RangeStats>, what: &str) {
    let (Some(g), Some(w)) = (got, want) else {
        assert_eq!(got.is_some(), want.is_some(), "{what}: presence");
        return;
    };
    assert_eq!(g.samples, w.samples, "{what}: samples");
    close(g.charge_ah, w.charge_ah, &format!("{what}: charge"));
    close(g.energy_wh, w.energy_wh, &format!("{what}: energy"));
    close(g.vbus_avg_v, w.vbus_avg_v, &format!("{what}: vbus avg"));
    close(g.ibus_avg_a, w.ibus_avg_a, &format!("{what}: ibus avg"));
    close(g.power_avg_w, w.power_avg_w, &format!("{what}: power avg"));
}

/// The folded block holding time `t`, if any.
fn block_of(blocks: &[(u64, u64)], t: f64) -> Option<(u64, u64)> {
    blocks
        .iter()
        .copied()
        .find(|&(s, e)| s as f64 <= t && t <= e as f64)
}

/// Running energy at time `t`, from the test's own sums (linear between
/// records, flat across the break).
fn running_at(run: &Run, t: f64) -> f64 {
    let k = run.t_ms.partition_point(|&x| (x as f64) <= t);
    if k == 0 {
        return 0.0;
    }
    if k >= run.t_ms.len() || k == BREAK_AT {
        return run.running_wh[k - 1];
    }
    let (t0, t1) = (run.t_ms[k - 1] as f64, run.t_ms[k] as f64);
    let (w0, w1) = (run.running_wh[k - 1], run.running_wh[k]);
    w0 + (w1 - w0) * (t - t0) / (t1 - t0)
}

fn query(kind: MarkKind, step_pct: f64) -> RunProgressQuery {
    RunProgressQuery {
        kind,
        step_pct,
        ed_points: 0,
        ed_t0_ms: None,
        ed_t1_ms: None,
        ed_scope: EdScope::Session,
    }
}

fn progress(h: &History, q: &RunProgressQuery) -> RunProgress {
    h.run_progress(q).expect("a progress answer")
}

#[test]
fn the_full_resolution_limit_is_ten_million_samples() {
    assert_eq!(HISTORY_FULL_SAMPLES, 10_000_000);
}

#[test]
fn a_history_under_its_limit_folds_nothing() {
    let mut h = History::with_full_limit(FULL);
    let mut g = Gen::new();
    for i in 0..FULL {
        h.push(&g.record(i));
    }
    assert_eq!(h.fold_boundary_ms(), None);
    assert!(h.fold_blocks_ms().is_empty());
    assert_eq!(h.len(), FULL);
    assert_eq!(SessionStore::new().history_fold_boundary(), None);
}

/// AC-AXHK2W.10, .11, .12: the 30-hour run in compressed time.
#[test]
fn a_thirty_hour_run_folds_under_its_ceiling_and_keeps_its_totals() {
    let run = run();
    let (f, r) = (&run.folded, &run.reference);

    // Every record still counts; the reference never folded.
    assert_eq!(f.len(), SAMPLES);
    assert_eq!(r.fold_boundary_ms(), None, "10 M limit never reached");

    // Memory: under the ceiling, and the ceiling is well below unfolded.
    let ceiling = ceiling_bytes();
    assert!(
        r.memory_bytes() > 3 * ceiling,
        "the unfolded reference ({} B) dwarfs the ceiling ({ceiling} B)",
        r.memory_bytes()
    );
    assert!(
        f.memory_bytes() <= ceiling,
        "folded history takes {} B, ceiling {ceiling} B",
        f.memory_bytes()
    );

    // Folding happened many times; blocks are ordered, never span the break.
    let blocks = f.fold_blocks_ms();
    assert!(blocks.len() >= 10, "{} blocks", blocks.len());
    let break_t = run.t_ms[BREAK_AT];
    for w in blocks.windows(2) {
        assert!(
            w[0].0 <= w[0].1 && w[0].1 < w[1].0,
            "blocks out of order: {w:?}"
        );
    }
    for &(s, e) in &blocks {
        assert!(
            !(s < break_t && e >= break_t),
            "block {s}..{e} spans the break"
        );
    }

    // AC.11: the boundary sits after every block and at or before the oldest
    // of the newest FULL records.
    let boundary = f.fold_boundary_ms().expect("a fold boundary");
    let last_block_end = blocks.last().map(|b| b.1).unwrap_or(0);
    assert!(
        boundary > last_block_end,
        "boundary {boundary} after {last_block_end}"
    );
    assert!(
        boundary <= run.t_ms[SAMPLES - FULL],
        "boundary {boundary} keeps the newest {FULL} records at full resolution"
    );

    // The newest FULL records keep full resolution: one bucket per record.
    let i0 = SAMPLES - FULL;
    let (t0, t1) = (run.t_ms[i0], run.t_ms[i0 + 49]);
    let got = f.range(&[Series::Vbus, Series::Ibus], t0, t1, 50);
    let want = r.range(&[Series::Vbus, Series::Ibus], t0, t1, 50);
    assert_eq!(got, want, "raw buckets in the full-resolution part");
    for sb in &got {
        assert_eq!(sb.buckets.len(), 50);
        assert!(sb.buckets.iter().all(|b| b.min == b.max), "one record each");
    }

    // Per-series min and max survive folding: the envelope over the folded
    // part and over the whole session matches.
    for (a, b) in [(run.t_ms[0], last_block_end), (run.t_ms[0], u64::MAX)] {
        let env = |h: &History| {
            h.range(&[Series::Vbus, Series::Ibus, Series::Power], a, b, 512)
                .into_iter()
                .map(|sb| {
                    assert!(!sb.buckets.is_empty(), "buckets for {:?}", sb.series);
                    sb.buckets.iter().fold((f32::MAX, f32::MIN), |(lo, hi), x| {
                        (lo.min(x.min), hi.max(x.max))
                    })
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(env(f), env(r), "min/max envelope over {a}..{b}");
    }

    // Whole session.
    same_stats(
        f.range_stats(0, u64::MAX),
        r.range_stats(0, u64::MAX),
        "whole session",
    );

    // The ruler at block edges: first record of one block to the last of
    // another (or the newest record), across the break too.
    let n = blocks.len();
    let after_break = blocks
        .iter()
        .position(|&(s, _)| s >= break_t)
        .expect("a block after the break");
    let pairs = [
        (0, n - 1),
        (0, 0),
        (n / 3, n / 2),
        (after_break.saturating_sub(1), after_break),
        (after_break, n - 1),
        (n - 1, n - 1),
    ];
    for (a, b) in pairs {
        let (t0, t1) = (blocks[a].0, blocks[b].1);
        same_stats(
            f.range_stats(t0, t1),
            r.range_stats(t0, t1),
            &format!("blocks {a}..={b} ({t0}..{t1})"),
        );
        // Either order.
        same_stats(
            f.range_stats(t1, t0),
            r.range_stats(t0, t1),
            &format!("blocks {b}..={a} reversed"),
        );
    }
    let newest = *run.t_ms.last().expect("records");
    for t0 in [blocks[0].0, blocks[n / 2].0, blocks[n - 1].0] {
        same_stats(
            f.range_stats(t0, newest),
            r.range_stats(t0, newest),
            &format!("block start {t0} to the newest record"),
        );
    }
    // Anywhere in the full-resolution part, any time.
    for (t0, t1) in [
        (boundary + 1_234, newest - 4_321),
        (boundary + 50, boundary + 77_777),
    ] {
        same_stats(
            f.range_stats(t0, t1),
            r.range_stats(t0, t1),
            &format!("full-resolution {t0}..{t1}"),
        );
    }

    // Segment marks and E.D. totals.
    for q in [
        query(MarkKind::Energy, 10.0),
        query(MarkKind::Charge, 5.0),
        query(MarkKind::Energy, 1.0),
    ] {
        let (g, w) = (progress(f, &q), progress(r, &q));
        let what = format!("{:?} every {}%", q.kind, q.step_pct);
        assert_eq!(g.session_t0_ms, w.session_t0_ms, "{what}: session t0");
        assert_eq!(g.run_t0_ms, w.run_t0_ms, "{what}: run t0");
        assert_eq!(g.run_t1_ms, w.run_t1_ms, "{what}: run t1");
        assert_eq!(g.samples, w.samples, "{what}: samples");
        close(g.charge_ah, w.charge_ah, &format!("{what}: run charge"));
        close(g.energy_wh, w.energy_wh, &format!("{what}: run energy"));
        close(
            g.ed_total_wh,
            w.ed_total_wh,
            &format!("{what}: session energy"),
        );
        assert!(w.marks_total_positive && g.marks_total_positive);
        assert_eq!(g.marks.len(), w.marks.len(), "{what}: mark count");
        for (gm, wm) in g.marks.iter().zip(&w.marks) {
            assert_eq!(gm.percent, wm.percent, "{what}: mark percent");
            match block_of(&blocks, wm.t_ms) {
                // Inside a block only the placement is lost.
                Some((s, e)) => assert!(
                    s as f64 - 1e-6 <= gm.t_ms && gm.t_ms <= e as f64 + 1e-6,
                    "{what}: {}% mark at {} outside its block {s}..{e} (reference {})",
                    wm.percent,
                    gm.t_ms,
                    wm.t_ms
                ),
                None => assert!(
                    (gm.t_ms - wm.t_ms).abs() <= 1e-6 * wm.t_ms.max(1.0),
                    "{what}: {}% mark at {}, reference {}",
                    wm.percent,
                    gm.t_ms,
                    wm.t_ms
                ),
            }
        }
    }

    // E.D. at block edges is exact, in either scope.
    for (scope, a, b) in [
        (EdScope::Session, 0, n - 1),
        (EdScope::Session, n / 4, after_break.saturating_sub(1)),
        (EdScope::Run, after_break, n - 1),
    ] {
        let q = RunProgressQuery {
            ed_points: 2,
            ed_t0_ms: Some(blocks[a].0),
            ed_t1_ms: Some(blocks[b].1),
            ed_scope: scope,
            ..query(MarkKind::Energy, 10.0)
        };
        let (g, w) = (progress(f, &q), progress(r, &q));
        let what = format!("E.D. {scope:?} blocks {a}..={b}");
        close(g.ed_total_wh, w.ed_total_wh, &format!("{what}: total"));
        assert_eq!(g.ed_t_ms, w.ed_t_ms, "{what}: times");
        assert_eq!(g.ed_pct.len(), 2, "{what}: points");
        for (gp, wp) in g.ed_pct.iter().zip(&w.ed_pct) {
            close(*gp, *wp, &format!("{what}: pct"));
        }
    }

    // E.D. over the whole session: exact outside blocks, and inside one
    // between the running energy at its two edges.
    let q = RunProgressQuery {
        ed_points: 500,
        ..query(MarkKind::Energy, 10.0)
    };
    let (g, w) = (progress(f, &q), progress(r, &q));
    assert_eq!(g.ed_t_ms, w.ed_t_ms, "E.D. times");
    assert_eq!(g.ed_pct.len(), 500);
    let total = w.ed_total_wh;
    for ((&t, &gp), &wp) in g.ed_t_ms.iter().zip(&g.ed_pct).zip(&w.ed_pct) {
        match block_of(&blocks, t) {
            Some((s, e)) => {
                let lo = 100.0 * running_at(&run, s as f64) / total;
                let hi = 100.0 * running_at(&run, e as f64) / total;
                assert!(
                    lo - 1e-7 <= gp && gp <= hi + 1e-7,
                    "E.D. at {t}: {gp}% outside its block's {lo}..{hi}%"
                );
            }
            None => close(gp, wp, &format!("E.D. at {t}")),
        }
    }
}

#[test]
fn the_fold_boundary_command_is_registered() {
    let source = include_str!("../src/lib.rs");
    let handlers = source
        .split("generate_handler![")
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .expect("the handler list");
    assert!(
        handlers
            .split(',')
            .any(|h| h.trim() == "history_fold_boundary"),
        "history_fold_boundary is not registered"
    );
}
