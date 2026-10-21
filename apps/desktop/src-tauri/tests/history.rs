//! Session history, min/max decimation and running statistics in the desktop
//! backend (FR AH2AEJ). Every identifier is synthetic.
//!
//! The release-only tests (10 M samples: memory, append cost, query time) run
//! in CI with `cargo test --release -p open-powerz-desktop --locked --test history`.
//!
//! Bucket values are `f32`: the reference for a sample is its SI value from
//! the [`QueueRecord`] accessor (`f64`) cast to `f32`.

mod common;

use std::sync::mpsc;
use std::time::{Duration, Instant};

use common::*;
use open_powerz_desktop_lib::batch::Sample;
use open_powerz_desktop_lib::devices::{DeviceListing, DeviceManager, Opener};
use open_powerz_desktop_lib::history::{Bucket, History, Series, SeriesBuckets};
use open_powerz_desktop_lib::stats::{MinMaxAvg, Stats, StatsSnapshot};
use open_powerz_desktop_lib::worker::{BackendEvent, Worker, WorkerConfig};
use powerz_protocol::decode::{QueueRecord, Rate};
use powerz_protocol::transport::{self, discovery::DeviceEntry, discovery::Model};
use powerz_record::Integrator;
use proptest::prelude::*;

const ALL_SERIES: [Series; 6] = [
    Series::Vbus,
    Series::Ibus,
    Series::Dp,
    Series::Dm,
    Series::Cc1,
    Series::Cc2,
];

/// A queue record from raw device units.
#[allow(clippy::too_many_arguments)]
fn rec(
    time_ms: u32,
    vbus_uv: i32,
    ibus_ua: i32,
    cc1: u16,
    cc2: u16,
    dp: u16,
    dm: u16,
    rate: Rate,
) -> QueueRecord {
    let mut b = Vec::with_capacity(20);
    b.extend_from_slice(&time_ms.to_le_bytes());
    b.extend_from_slice(&vbus_uv.to_le_bytes());
    b.extend_from_slice(&ibus_ua.to_le_bytes());
    for v in [cc1, cc2, dp, dm] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    QueueRecord::decode(&b, rate).unwrap()
}

/// The reference value of `series` for record `r`.
fn value(r: &QueueRecord, series: Series) -> f32 {
    (match series {
        Series::Vbus => r.vbus_v(),
        Series::Ibus => r.ibus_a(),
        Series::Dp => r.dp_v(),
        Series::Dm => r.dm_v(),
        Series::Cc1 => r.cc1_v(),
        Series::Cc2 => r.cc2_v(),
    }) as f32
}

/// Brute-force min and max of `series` over samples with `t0 <= t <= t1`.
fn brute(records: &[QueueRecord], series: Series, t0: u64, t1: u64) -> Option<(f32, f32)> {
    records
        .iter()
        .filter(|r| (t0..=t1).contains(&u64::from(r.time_ms())))
        .map(|r| value(r, series))
        .fold(None, |acc, v| match acc {
            None => Some((v, v)),
            Some((lo, hi)) => Some((lo.min(v), hi.max(v))),
        })
}

/// Check one `range` answer against a brute-force scan.
fn check_range(
    records: &[QueueRecord],
    history: &History,
    series: &[Series],
    t0: u64,
    t1: u64,
    buckets: usize,
) {
    let got = history.range(series, t0, t1, buckets);
    assert_eq!(got.len(), series.len(), "one answer per requested series");
    for (want_series, answer) in series.iter().zip(&got) {
        assert_eq!(answer.series, *want_series, "answers in request order");
        let b = &answer.buckets;
        assert!(
            b.len() <= buckets,
            "{} buckets for {buckets} requested",
            b.len()
        );
        for bucket in b {
            assert!(
                (t0..=t1).contains(&bucket.t_ms),
                "bucket at {} outside {t0}..={t1}",
                bucket.t_ms
            );
            assert!(bucket.min <= bucket.max, "{bucket:?}");
        }
        assert!(
            b.windows(2).all(|w| w[0].t_ms <= w[1].t_ms),
            "buckets out of time order"
        );
        let expected = brute(records, *want_series, t0, t1);
        match expected {
            None => assert!(b.is_empty(), "buckets for an empty range: {b:?}"),
            Some((lo, hi)) => {
                assert!(!b.is_empty(), "no buckets for a range with samples");
                let got_lo = b.iter().map(|x| x.min).fold(f32::INFINITY, f32::min);
                let got_hi = b.iter().map(|x| x.max).fold(f32::NEG_INFINITY, f32::max);
                assert_eq!(
                    (got_lo, got_hi),
                    (lo, hi),
                    "{want_series:?} over {t0}..={t1} in {buckets} buckets"
                );
            }
        }
    }
}

/// Deterministic pseudo-random numbers (splitmix64).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn i32_in(&mut self, lo: i32, hi: i32) -> i32 {
        lo + self.below((hi - lo) as u64 + 1) as i32
    }

    fn u16_below(&mut self, n: u16) -> u16 {
        self.below(u64::from(n)) as u16
    }
}

fn random_records(seed: u64, n: usize, start_ms: u32) -> Vec<QueueRecord> {
    let mut rng = Rng(seed);
    let mut t = start_ms;
    (0..n)
        .map(|_| {
            // Mostly 1 ms steps, some repeats and some jumps (stream gaps).
            t += match rng.below(100) {
                0..=2 => 0,
                3 => 1 + rng.below(5_000) as u32,
                _ => 1,
            };
            rec(
                t,
                rng.i32_in(-1_000_000, 48_000_000),
                rng.i32_in(-6_000_000, 6_000_000),
                rng.u16_below(3_300),
                rng.u16_below(3_300),
                rng.u16_below(3_300),
                rng.u16_below(3_300),
                Rate::Sps1000,
            )
        })
        .collect()
}

fn history_of(records: &[QueueRecord]) -> History {
    let mut h = History::new();
    for r in records {
        h.push(r);
    }
    h
}

// ---- AC-AH2AEJ.1: columnar raw-unit history, memory, amortized append ----

#[test]
fn history_keeps_every_record_exactly_in_raw_units() {
    let mut records = random_records(1, 5_000, 10_000);
    // A rate change mid-session: CC / D± scale depends on the rate, so the
    // store must give back each record with the rate it was captured at.
    let last = records.last().unwrap().time_ms();
    records.extend((1..=300u32).map(|k| {
        rec(
            last + k * 500,
            4_021 + k as i32,
            -59 - k as i32,
            82,
            29,
            69,
            68,
            Rate::Sps2,
        )
    }));
    let h = history_of(&records);
    assert_eq!(h.len(), records.len());
    assert!(!h.is_empty());
    for (i, r) in records.iter().enumerate() {
        assert_eq!(h.get(i), Some(*r), "record {i} changed in the store");
    }
    assert_eq!(h.get(records.len()), None);
}

#[test]
fn an_empty_history_has_no_records_and_no_buckets() {
    let h = History::new();
    assert_eq!(h.len(), 0);
    assert!(h.is_empty());
    assert_eq!(h.get(0), None);
    let got = h.range(&ALL_SERIES, 0, u64::MAX, 100);
    assert_eq!(got.len(), 6);
    assert!(got.iter().all(|s| s.buckets.is_empty()));
}

#[test]
fn memory_accounting_covers_at_least_the_raw_columns() {
    let mut h = History::new();
    let empty = h.memory_bytes();
    for r in random_records(2, 100_000, 0) {
        h.push(&r);
    }
    // 20 bytes per sample is the raw columns alone; the accounting must
    // include them (it reports capacity, not just length).
    assert!(
        h.memory_bytes() >= empty + 20 * 100_000,
        "{} bytes for 100 000 samples",
        h.memory_bytes()
    );
}

#[test]
fn the_worker_appends_every_received_record_to_the_history() {
    let clock = clock();
    let pipe = Pipe::sim(
        Scenario::Load5v1a,
        FaultPlan::new(),
        &clock,
        PipeOpts::default(),
    );
    let log = std::sync::Arc::clone(&pipe.log);
    let (_events, sink) = capture(&clock);
    let mut worker = Worker::start(pipe, clock.clone(), config(Rate::Sps1000), sink).unwrap();
    run_for(&mut worker, &clock, 2_000);
    let delivered = log.lock().unwrap().records_delivered;
    assert!(delivered > 1_000, "{delivered} records");
    assert_eq!(worker.history_len(), delivered);
}

// ---- AC-AH2AEJ.2: history_range buckets carry the exact min / max ----

#[test]
fn range_matches_a_brute_force_scan_across_pyramid_levels() {
    // Enough samples for blocks of 64, 256, 1024, 4096 and 16384.
    let records = random_records(3, 100_000, 1_000);
    let h = history_of(&records);
    let first = u64::from(records[0].time_ms());
    let last = u64::from(records.last().unwrap().time_ms());
    let mut rng = Rng(4);
    // The full range, beyond both ends, and a few tight edges.
    check_range(&records, &h, &ALL_SERIES, 0, u64::MAX, 2_000);
    check_range(&records, &h, &ALL_SERIES, first, last, 1);
    check_range(&records, &h, &[Series::Vbus], first + 1, first + 1, 10);
    check_range(&records, &h, &[Series::Ibus], last + 1, u64::MAX, 10);
    for _ in 0..40 {
        let a = first + rng.below(last - first + 1);
        let b = first + rng.below(last - first + 1);
        let (t0, t1) = (a.min(b), a.max(b));
        let buckets = 1 + rng.below(3_000) as usize;
        let series = ALL_SERIES[rng.below(6) as usize];
        check_range(&records, &h, &[series], t0, t1, buckets);
    }
}

#[test]
fn zero_buckets_returns_no_buckets() {
    let records = random_records(5, 1_000, 0);
    let h = history_of(&records);
    let got = h.range(&[Series::Vbus], 0, u64::MAX, 0);
    assert_eq!(got.len(), 1);
    assert!(got[0].buckets.is_empty());
}

#[test]
fn a_full_range_query_is_sized_to_the_requested_buckets() {
    let records = random_records(6, 200_000, 0);
    let h = history_of(&records);
    let got = h.range(&[Series::Vbus], 0, u64::MAX, 2_000);
    let n = got[0].buckets.len();
    // A chart asks for its width in buckets: it must get close to that, not
    // a handful of coarse blocks.
    assert!((1_000..=2_000).contains(&n), "{n} buckets for 2000");
}

fn record_strategy() -> impl Strategy<Value = Vec<(u32, i32, i32, u16, u16, u16, u16)>> {
    prop::collection::vec(
        (
            prop_oneof![8 => Just(1u32), 1 => Just(0u32), 1 => 2u32..2_000],
            -30_000_000i32..30_000_000,
            -6_000_000i32..6_000_000,
            0u16..4_000,
            0u16..4_000,
            0u16..4_000,
            0u16..4_000,
        ),
        0..3_000,
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    #[test]
    fn range_min_max_equals_a_brute_force_scan(
        raw in record_strategy(),
        start in 0u32..1_000_000,
        a in 0u64..4_000_000,
        b in 0u64..4_000_000,
        buckets in 1usize..400,
        pick in 0usize..6,
    ) {
        let mut t = start;
        let records: Vec<QueueRecord> = raw
            .iter()
            .map(|&(dt, v, i, c1, c2, dp, dm)| {
                t += dt;
                rec(t, v, i, c1, c2, dp, dm, Rate::Sps1000)
            })
            .collect();
        let h = history_of(&records);
        let span = u64::from(t.saturating_sub(start)) + 20;
        let t0 = u64::from(start) + a % span;
        let t1 = u64::from(start) + b % span;
        let (t0, t1) = (t0.min(t1).saturating_sub(10), t0.max(t1));
        check_range(&records, &h, &[ALL_SERIES[pick]], t0, t1, buckets);
        check_range(&records, &h, &ALL_SERIES, 0, u64::MAX, buckets);
    }
}

#[test]
fn range_types_serialize_for_the_ui() {
    for (name, series) in [
        ("vbus", Series::Vbus),
        ("ibus", Series::Ibus),
        ("dp", Series::Dp),
        ("dm", Series::Dm),
        ("cc1", Series::Cc1),
        ("cc2", Series::Cc2),
    ] {
        let parsed: Series = serde_json::from_str(&format!("\"{name}\"")).unwrap();
        assert_eq!(parsed, series);
    }
    let answer = SeriesBuckets {
        series: Series::Ibus,
        buckets: vec![Bucket {
            t_ms: 12,
            min: -0.5,
            max: 1.25,
        }],
    };
    assert_eq!(
        serde_json::to_value(&answer).unwrap(),
        serde_json::json!({
            "series": "ibus",
            "buckets": [{"t_ms": 12, "min": -0.5, "max": 1.25}],
        })
    );
}

// ---- AC-AH2AEJ.1 / .3: 10 million samples (release builds only) ----

/// Release builds only: time and memory bounds mean nothing in a debug
/// build. CI runs `cargo test --release -p open-powerz-desktop --locked --test history`.
#[test]
#[cfg_attr(debug_assertions, ignore = "timing bound; run in release mode")]
fn ten_million_samples_fit_in_250_mb_and_a_2000_bucket_query_takes_at_most_50_ms() {
    const N: u32 = 10_000_000;
    let mut rng = Rng(7);
    let mut h = History::new();
    let mut lo = [f32::INFINITY; 6];
    let mut hi = [f32::NEG_INFINITY; 6];
    let mut halves = [Duration::ZERO; 2];
    for half in 0..2u32 {
        let started = Instant::now();
        for k in half * (N / 2)..(half + 1) * (N / 2) {
            let spike = if k == 7_654_321 { 20_000_000 } else { 0 };
            let r = rec(
                k + 1,
                5_000_000 + rng.i32_in(-50_000, 50_000) + spike,
                1_000_000 + rng.i32_in(-1_500_000, 200_000),
                rng.u16_below(3_300),
                rng.u16_below(3_300),
                rng.u16_below(3_300),
                rng.u16_below(3_300),
                Rate::Sps1000,
            );
            for (s, series) in ALL_SERIES.iter().enumerate() {
                let v = value(&r, *series);
                lo[s] = lo[s].min(v);
                hi[s] = hi[s].max(v);
            }
            h.push(&r);
        }
        halves[half as usize] = started.elapsed();
    }
    assert_eq!(h.len(), N as usize);

    let bytes = h.memory_bytes();
    assert!(bytes <= 250_000_000, "{bytes} bytes for 10 M samples");

    // Amortized constant time: the second 5 M appends cost about what the
    // first 5 M did (a per-append cost growing with the length would not).
    assert!(
        halves[1] <= halves[0] * 3 + Duration::from_millis(200),
        "first half {:?}, second half {:?}",
        halves[0],
        halves[1]
    );

    let started = Instant::now();
    let got = h.range(&ALL_SERIES, 0, u64::MAX, 2_000);
    let took = started.elapsed();
    assert!(
        took <= Duration::from_millis(50),
        "full-range query took {took:?}"
    );
    for (s, answer) in got.iter().enumerate() {
        let n = answer.buckets.len();
        assert!((1_000..=2_000).contains(&n), "{n} buckets");
        let got_lo = answer
            .buckets
            .iter()
            .map(|b| b.min)
            .fold(f32::INFINITY, f32::min);
        let got_hi = answer
            .buckets
            .iter()
            .map(|b| b.max)
            .fold(f32::NEG_INFINITY, f32::max);
        assert_eq!((got_lo, got_hi), (lo[s], hi[s]), "{:?}", answer.series);
    }

    // An export snapshot shares the stored samples instead of copying them.
    let started = Instant::now();
    let snapshot = h.snapshot();
    let took = started.elapsed();
    assert!(took <= Duration::from_millis(5), "snapshot took {took:?}");
    assert_eq!(snapshot.len(), N as usize);
    h.push(&rec(N + 1, 0, 0, 0, 0, 0, 0, Rate::Sps1000));
    assert_eq!(snapshot.len(), N as usize, "the snapshot saw a later push");
    assert_eq!(snapshot.iter().count(), N as usize);
}

// ---- AC-AH2AEJ.4: stats from the shared integrator ----

/// Relative closeness for the 1e-9 bound (absolute near zero).
fn close(got: f64, want: f64) -> bool {
    (got - want).abs() <= 1e-9 * got.abs().max(want.abs()) + 1e-12
}

fn assert_close(what: &str, got: f64, want: f64) {
    assert!(close(got, want), "{what}: got {got}, want {want}");
}

/// Stats computed here from the samples the UI received.
struct Reference {
    elapsed_ms: u64,
    samples: u64,
    charge_ah: f64,
    energy_wh: f64,
    vbus: (f64, f64, f64),
    ibus: (f64, f64, f64),
    power: (f64, f64, f64),
}

fn min_max_avg(values: impl Iterator<Item = f64> + Clone) -> (f64, f64, f64) {
    let n = values.clone().count() as f64;
    let lo = values.clone().fold(f64::INFINITY, f64::min);
    let hi = values.clone().fold(f64::NEG_INFINITY, f64::max);
    (lo, hi, values.sum::<f64>() / n)
}

fn reference(samples: &[Sample]) -> Reference {
    assert!(!samples.is_empty());
    // The CLI recorder's integrator, plus an independent trapezoid sum.
    let mut integrator = Integrator::new();
    let (mut charge, mut energy) = (0.0, 0.0);
    let (mut q, mut e) = (0.0f64, 0.0f64);
    for (k, s) in samples.iter().enumerate() {
        (charge, energy) = integrator.push(s.t_ms, s.vbus_v, s.ibus_a);
        if k > 0 {
            let p = samples[k - 1];
            let h = f64::from(s.t_ms - p.t_ms) / 3_600_000.0;
            q += (p.ibus_a + s.ibus_a) / 2.0 * h;
            e += (p.vbus_v * p.ibus_a + s.vbus_v * s.ibus_a) / 2.0 * h;
        }
    }
    assert_close("trapezoid charge", charge, q);
    assert_close("trapezoid energy", energy, e);
    Reference {
        elapsed_ms: u64::from(samples.last().unwrap().t_ms - samples[0].t_ms),
        samples: samples.len() as u64,
        charge_ah: charge,
        energy_wh: energy,
        vbus: min_max_avg(samples.iter().map(|s| s.vbus_v)),
        ibus: min_max_avg(samples.iter().map(|s| s.ibus_a)),
        power: min_max_avg(samples.iter().map(|s| s.vbus_v * s.ibus_a)),
    }
}

fn assert_mma(what: &str, got: &Option<MinMaxAvg>, want: (f64, f64, f64)) {
    let got = got
        .as_ref()
        .unwrap_or_else(|| panic!("{what}: no min/max/avg"));
    assert_close(&format!("{what} min"), got.min, want.0);
    assert_close(&format!("{what} max"), got.max, want.1);
    assert_close(&format!("{what} avg"), got.avg, want.2);
}

fn assert_matches(got: &StatsSnapshot, want: &Reference) {
    assert_eq!(got.samples, want.samples, "sample count");
    assert_eq!(got.elapsed_ms, want.elapsed_ms, "elapsed time");
    assert_close("charge", got.charge_ah, want.charge_ah);
    assert_close("energy", got.energy_wh, want.energy_wh);
    assert_mma("vbus", &got.vbus, want.vbus);
    assert_mma("ibus", &got.ibus, want.ibus);
    assert_mma("power", &got.power, want.power);
}

fn assert_zero(s: &StatsSnapshot) {
    assert_eq!(s.samples, 0);
    assert_eq!(s.elapsed_ms, 0);
    assert_eq!(s.charge_ah, 0.0);
    assert_eq!(s.energy_wh, 0.0);
    assert!(
        s.vbus.is_none() && s.ibus.is_none() && s.power.is_none(),
        "{s:?}"
    );
}

fn as_sample(r: &QueueRecord) -> Sample {
    Sample {
        t_ms: r.time_ms(),
        vbus_v: r.vbus_v(),
        ibus_a: r.ibus_a(),
        cc1_v: r.cc1_v(),
        cc2_v: r.cc2_v(),
        dp_v: r.dp_v(),
        dm_v: r.dm_v(),
    }
}

#[test]
fn stats_follow_the_cli_integrator_with_signed_current() {
    let records = random_records(8, 20_000, 500);
    let mut stats = Stats::new();
    assert_zero(&stats.snapshot());
    for r in &records {
        stats.push(r);
    }
    let samples: Vec<Sample> = records.iter().map(as_sample).collect();
    let want = reference(&samples);
    assert_matches(&stats.snapshot(), &want);
}

#[test]
fn negative_current_gives_negative_charge_and_energy() {
    let mut stats = Stats::new();
    for k in 0..=3_600u32 {
        // 5 V, −0.12 A for one hour at 1 SPS: −0.12 Ah, −0.6 Wh.
        stats.push(&rec(k * 1_000, 5_000_000, -120_000, 0, 0, 0, 0, Rate::Sps2));
    }
    let s = stats.snapshot();
    assert_close("charge", s.charge_ah, -0.12);
    assert_close("energy", s.energy_wh, -0.6);
    assert_eq!(s.elapsed_ms, 3_600_000);
    let ibus = s.ibus.unwrap();
    assert_close("ibus avg", ibus.avg, -0.12);
    let power = s.power.unwrap();
    assert_close("power max", power.max, -0.6);
}

fn config(rate: Rate) -> WorkerConfig {
    WorkerConfig {
        rate,
        session_id: 1,
    }
}

/// Every batch event, in order.
fn batches(events: &Events) -> Vec<(u64, open_powerz_desktop_lib::batch::Batch)> {
    events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|(t, e)| match e {
            BackendEvent::Batch(b) => Some((*t, b.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn every_batch_carries_stats_matching_the_simulator_reference() {
    let clock = clock();
    let pipe = Pipe::sim(
        Scenario::NoisyNegative,
        FaultPlan::new(),
        &clock,
        PipeOpts::default(),
    );
    let (events, sink) = capture(&clock);
    let mut worker = Worker::start(pipe, clock.clone(), config(Rate::Sps1000), sink).unwrap();
    run_for(&mut worker, &clock, 3_000);

    let mut seen: Vec<Sample> = Vec::new();
    let mut checked = 0;
    for (_, batch) in batches(&events) {
        seen.extend_from_slice(&batch.samples);
        if seen.is_empty() {
            assert_zero(&batch.stats);
            continue;
        }
        // A batch's stats cover every sample sent up to and including it.
        assert_matches(&batch.stats, &reference(&seen));
        checked += 1;
    }
    assert!(seen.len() > 2_000, "{} samples", seen.len());
    assert!(checked > 20, "{checked} batches checked");
    assert!(
        seen.iter().any(|s| s.ibus_a < 0.0),
        "the scenario never went negative"
    );
}

// ---- AC-AH2AEJ.6 (backend side): stats reach the UI at least 4 times a second ----

#[test]
fn stats_ride_batches_at_least_four_times_a_second() {
    let clock = clock();
    let pipe = Pipe::sim(
        Scenario::Load5v1a,
        FaultPlan::new(),
        &clock,
        PipeOpts::default(),
    );
    let (events, sink) = capture(&clock);
    let mut worker = Worker::start(pipe, clock.clone(), config(Rate::Sps2), sink).unwrap();
    let t0 = clock.now_ms();
    run_for(&mut worker, &clock, 3_000);
    let times: Vec<u64> = batches(&events).iter().map(|(t, _)| *t).collect();
    assert!(times.len() >= 12, "{} batches in 3 s", times.len());
    let mut prev = t0;
    for t in times {
        assert!(t - prev <= 250, "no stats for {} ms", t - prev);
        prev = t;
    }
    let json = serde_json::to_value(BackendEvent::Batch(batches(&events)[5].1.clone())).unwrap();
    let stats = &json["data"]["stats"];
    for key in [
        "elapsed_ms",
        "samples",
        "charge_ah",
        "energy_wh",
        "vbus",
        "ibus",
        "power",
    ] {
        assert!(stats.get(key).is_some(), "stats.{key} missing: {stats}");
    }
}

// ---- AC-AH2AEJ.5: reset_stats and clear_history ----

#[test]
fn reset_zeroes_the_stats_and_counts_from_the_next_sample() {
    let mut stats = Stats::new();
    for k in 0..100u32 {
        stats.push(&rec(k, 9_000_000, 2_000_000, 0, 0, 0, 0, Rate::Sps1000));
    }
    stats.reset();
    assert_zero(&stats.snapshot());
    // The interval between the last sample before the reset and the first
    // after it does not count.
    let after: Vec<QueueRecord> = (0..50u32)
        .map(|k| {
            rec(
                5_000 + k * 2,
                5_000_000,
                -300_000,
                0,
                0,
                0,
                0,
                Rate::Sps1000,
            )
        })
        .collect();
    stats.push(&after[0]);
    let first = stats.snapshot();
    assert_eq!(first.samples, 1);
    assert_eq!(first.elapsed_ms, 0);
    assert_eq!(first.charge_ah, 0.0);
    assert_eq!(first.energy_wh, 0.0);
    for r in &after[1..] {
        stats.push(r);
    }
    let samples: Vec<Sample> = after.iter().map(as_sample).collect();
    assert_matches(&stats.snapshot(), &reference(&samples));
}

#[test]
fn reset_stats_leaves_the_history_untouched() {
    let clock = clock();
    let pipe = Pipe::sim(
        Scenario::NoisyNegative,
        FaultPlan::new(),
        &clock,
        PipeOpts::default(),
    );
    let (events, sink) = capture(&clock);
    let mut worker = Worker::start(pipe, clock.clone(), config(Rate::Sps1000), sink).unwrap();
    run_for(&mut worker, &clock, 1_500);
    let before = worker.history_len();
    assert!(before > 0);
    let whole_before = worker.history_range(&[Series::Vbus], 0, u64::MAX, 500);

    worker.reset_stats();
    assert_zero(&worker.stats());
    assert_eq!(worker.history_len(), before, "reset touched the history");
    assert_eq!(
        worker.history_range(&[Series::Vbus], 0, u64::MAX, 500),
        whole_before,
        "reset changed the history"
    );

    run_for(&mut worker, &clock, 1_500);
    worker.stop();
    let all = samples_of(&events);
    assert_eq!(all.len(), worker.history_len());
    assert!(worker.history_len() > before);
    // Stats count only the records received after the reset.
    assert_matches(&worker.stats(), &reference(&all[before..]));
}

fn samples_of(events: &Events) -> Vec<Sample> {
    batches(events)
        .into_iter()
        .flat_map(|(_, b)| b.samples)
        .collect()
}

#[test]
fn clear_history_discards_the_history_and_the_stream_goes_on() {
    let clock = clock();
    let pipe = Pipe::sim(
        Scenario::Load5v1a,
        FaultPlan::new(),
        &clock,
        PipeOpts::default(),
    );
    let (_events, sink) = capture(&clock);
    let mut worker = Worker::start(pipe, clock.clone(), config(Rate::Sps1000), sink).unwrap();
    run_for(&mut worker, &clock, 1_000);
    assert!(worker.history_len() > 0);

    worker.clear_history();
    assert_eq!(worker.history_len(), 0);
    let got = worker.history_range(&ALL_SERIES, 0, u64::MAX, 100);
    assert!(got.iter().all(|s| s.buckets.is_empty()), "{got:?}");
    // An export right after the clear holds nothing but the header.
    let (tx, rx) = mpsc::channel();
    let job = worker.export(Box::new(ChannelWriter(tx)), |_| {});
    assert_eq!(job.join().unwrap().unwrap(), 0);
    let text: Vec<u8> = rx.try_iter().flatten().collect();
    assert_eq!(
        String::from_utf8(text).unwrap(),
        format!("{}\n", powerz_record::CSV_HEADER)
    );

    run_for(&mut worker, &clock, 1_000);
    assert!(worker.history_len() > 0, "the history stopped growing");
}

/// A writer that hands every write to a channel.
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

// ---- the commands through the device manager ----

struct SimOpener {
    clock: SimClock,
}

impl Opener for SimOpener {
    type Transport = SimDevice;
    fn open(&self, _device: &DeviceListing) -> Result<SimDevice, transport::Error> {
        Ok(SimDevice::with_clock(
            Persona::Bulk,
            Scenario::Load5v1a,
            self.clock.clone(),
        ))
    }
}

fn manager() -> DeviceManager<SimOpener, SimClock> {
    let clock = clock();
    DeviceManager::new(
        || {
            Ok(vec![DeviceEntry {
                model: Model::Km003c,
                pid: 0x0063,
                serial: Some("SERIAL-0001".to_owned()),
                bus: "1".to_owned(),
                address: 7,
            }])
        },
        SimOpener {
            clock: clock.clone(),
        },
        clock,
    )
}

/// Wait (real time) until `n` more samples have arrived.
fn wait_samples(rx: &mpsc::Receiver<BackendEvent>, n: usize) {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut got = 0;
    while got < n {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(BackendEvent::Batch(b)) => got += b.samples.len(),
            Ok(_) => {}
            Err(e) => panic!("samples never arrived ({e:?}); got {got}"),
        }
    }
}

#[test]
fn the_manager_answers_history_and_stats_commands() {
    let mut m = manager();
    let (tx, rx) = mpsc::channel();
    m.connect("demo:load-5v-1a", 50, move |e| {
        let _ = tx.send(e);
    })
    .expect("connect");
    wait_samples(&rx, 100);

    let stats = m.stats().expect("stats");
    assert!(stats.samples >= 100, "{stats:?}");
    let vbus = stats.vbus.expect("vbus stats");
    assert!((4.5..5.5).contains(&vbus.avg), "{vbus:?}");

    let got = m
        .history_range(&[Series::Vbus, Series::Ibus], 0, u64::MAX, 100)
        .expect("history_range");
    assert_eq!(got.len(), 2);
    assert_eq!(got[0].series, Series::Vbus);
    assert_eq!(got[1].series, Series::Ibus);
    assert!(!got[0].buckets.is_empty() && got[0].buckets.len() <= 100);
    assert!(got[0].buckets.iter().all(|b| (4.5..5.5).contains(&b.min)));

    m.reset_stats().expect("reset_stats");
    m.clear_history().expect("clear_history");
    m.disconnect().unwrap();
}

// ---- history_range input limits ----

#[test]
fn a_huge_bucket_request_is_capped_at_8192_buckets() {
    let records = random_records(9, 20_000, 0);
    let h = history_of(&records);
    let got = h.range(&ALL_SERIES, 0, u64::MAX, usize::MAX);
    for answer in &got {
        assert!(
            !answer.buckets.is_empty() && answer.buckets.len() <= 8_192,
            "{} buckets for {:?}",
            answer.buckets.len(),
            answer.series
        );
    }
    check_range(&records, &h, &ALL_SERIES, 0, u64::MAX, usize::MAX);
}

#[test]
fn a_repeated_series_gets_one_answer_per_request_in_order() {
    let records = random_records(10, 5_000, 0);
    let h = history_of(&records);
    let got = h.range(
        &[Series::Vbus, Series::Ibus, Series::Vbus],
        0,
        u64::MAX,
        300,
    );
    let order: Vec<Series> = got.iter().map(|a| a.series).collect();
    assert_eq!(order, [Series::Vbus, Series::Ibus, Series::Vbus]);
    assert_eq!(got[0], got[2]);
    check_range(
        &records,
        &h,
        &[Series::Vbus, Series::Vbus],
        0,
        u64::MAX,
        300,
    );
}

// ---- a device clock that goes backwards ----

/// The simulator behind a pipe whose device clock jumps back by `shift_ms`
/// from queue poll number `from_poll` on, as a meter that restarts its
/// clock would. Every delivered device time is logged.
struct ClockJump {
    inner: Pipe,
    from_poll: usize,
    shift_ms: u32,
    polls: usize,
    device_times: std::sync::Arc<std::sync::Mutex<Vec<u32>>>,
}

impl transport::Transport for ClockJump {
    fn transact(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, transport::Error> {
        use powerz_protocol::frame::{self, Attribute};
        let queue_poll = frame::get_data_mask(request)
            .is_some_and(|m| m.bits() & Attribute::ADC_QUEUE.bits() != 0);
        let mut reply = self.inner.transact(request, timeout)?;
        if !queue_poll || reply.len() <= 4 {
            return Ok(reply);
        }
        self.polls += 1;
        let base = reply.as_ptr() as usize;
        let spans: Vec<(usize, usize)> = frame::parse_put_data(&reply)
            .map(|sections| {
                sections
                    .iter()
                    .filter(|s| s.att == Attribute::ADC_QUEUE)
                    .map(|s| (s.payload.as_ptr() as usize - base, s.payload.len()))
                    .collect()
            })
            .unwrap_or_default();
        let mut log = self.device_times.lock().unwrap();
        for (start, len) in spans {
            for rec in reply[start..start + len].as_chunks_mut::<20>().0 {
                let mut t = u32::from_le_bytes([rec[0], rec[1], rec[2], rec[3]]);
                if self.polls >= self.from_poll {
                    t = t.saturating_sub(self.shift_ms);
                    rec[..4].copy_from_slice(&t.to_le_bytes());
                }
                log.push(t);
            }
        }
        Ok(reply)
    }

    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, transport::Error> {
        self.inner.read(timeout)
    }
}

/// Brute-force min and max of `series` over UI samples with `t0 <= t <= t1`.
fn brute_samples(samples: &[Sample], series: Series, t0: u64, t1: u64) -> Option<(f32, f32)> {
    samples
        .iter()
        .filter(|s| (t0..=t1).contains(&u64::from(s.t_ms)))
        .map(|s| {
            (match series {
                Series::Vbus => s.vbus_v,
                Series::Ibus => s.ibus_a,
                Series::Dp => s.dp_v,
                Series::Dm => s.dm_v,
                Series::Cc1 => s.cc1_v,
                Series::Cc2 => s.cc2_v,
            }) as f32
        })
        .fold(None, |acc, v| match acc {
            None => Some((v, v)),
            Some((lo, hi)) => Some((lo.min(v), hi.max(v))),
        })
}

/// The overall min / max of one `range` answer.
fn overall(buckets: &[Bucket]) -> Option<(f32, f32)> {
    buckets.iter().fold(None, |acc, b| match acc {
        None => Some((b.min, b.max)),
        Some((lo, hi)) => Some((lo.min(b.min), hi.max(b.max))),
    })
}

/// Session times the UI saw never decrease, and `range` over random spans
/// of them matches a brute-force scan of the samples.
fn assert_timeline(
    samples: &[Sample],
    range: impl Fn(&[Series], u64, u64, usize) -> Vec<SeriesBuckets>,
) {
    assert!(
        samples.windows(2).all(|w| w[0].t_ms <= w[1].t_ms),
        "session time went backwards"
    );
    let first = u64::from(samples[0].t_ms);
    let last = u64::from(samples[samples.len() - 1].t_ms);
    let mut rng = Rng(11);
    let mut spans = vec![(0, u64::MAX), (first, last)];
    for _ in 0..30 {
        let a = first + rng.below(last - first + 1);
        let b = first + rng.below(last - first + 1);
        spans.push((a.min(b), a.max(b)));
    }
    for (t0, t1) in spans {
        for series in ALL_SERIES {
            let got = range(&[series], t0, t1, 1 + rng.below(500) as usize);
            assert_eq!(
                overall(&got[0].buckets),
                brute_samples(samples, series, t0, t1),
                "{series:?} over {t0}..={t1}"
            );
        }
    }
}

#[test]
fn a_device_clock_that_jumps_back_keeps_the_session_timeline_and_ranges_exact() {
    let clock = clock();
    let device_times = std::sync::Arc::default();
    let pipe = ClockJump {
        inner: Pipe::sim(
            Scenario::NoisyNegative,
            FaultPlan::new(),
            &clock,
            PipeOpts::default(),
        ),
        from_poll: 30,
        shift_ms: 5_000,
        polls: 0,
        device_times: std::sync::Arc::clone(&device_times),
    };
    let (events, sink) = capture(&clock);
    let mut worker = Worker::start(pipe, clock.clone(), config(Rate::Sps1000), sink).unwrap();
    run_for(&mut worker, &clock, 3_000);
    worker.stop();
    let raw = device_times.lock().unwrap().clone();
    assert!(
        raw.windows(2).any(|w| w[1] < w[0]),
        "the device clock never jumped back"
    );
    let all = samples_of(&events);
    assert_eq!(all.len(), raw.len());
    assert_eq!(worker.history_len(), all.len());
    assert_timeline(&all, |s, t0, t1, n| worker.history_range(s, t0, t1, n));
    // The chart breaks its lines at the jump too (AC-272WSN.3).
    let gaps = worker.history_gaps(0, u64::MAX);
    assert_eq!(gaps.len(), 1, "one break at the clock jump: {gaps:?}");
    assert!(
        gaps[0].to_ms > gaps[0].from_ms,
        "the break spans forward in session time: {gaps:?}"
    );
}

// ---- one session across reconnects ----

#[test]
fn a_reconnect_on_the_same_store_continues_history_and_stats_without_outage_charge() {
    use open_powerz_desktop_lib::store::SessionStore;
    let clock = clock();
    let store = SessionStore::new();
    let connection = |store: &SessionStore| {
        // A fresh simulator: its device clock starts again at zero.
        let pipe = Pipe::sim(
            Scenario::Load5v1a,
            FaultPlan::new(),
            &clock,
            PipeOpts::default(),
        );
        let (events, sink) = capture(&clock);
        let mut worker = Worker::start_with_store(
            pipe,
            clock.clone(),
            config(Rate::Sps1000),
            store.clone(),
            sink,
        )
        .unwrap();
        run_for(&mut worker, &clock, 1_500);
        worker.stop();
        (worker, samples_of(&events))
    };
    let (_, first) = connection(&store);
    // A 30 s outage at 1 A would add about 8.3 mAh if it were integrated.
    clock.advance(Duration::from_secs(30));
    let (second_worker, second) = connection(&store);

    let all: Vec<Sample> = first.iter().chain(&second).copied().collect();
    assert_eq!(store.history_len(), all.len(), "history lost a connection");
    assert!(
        second[0].t_ms > first[first.len() - 1].t_ms,
        "the second connection does not follow the first"
    );
    assert_timeline(&all, |s, t0, t1, n| store.history_range(s, t0, t1, n));

    let stats = store.stats();
    assert_eq!(stats.samples, all.len() as u64);
    let (a, b) = (reference(&first), reference(&second));
    assert_close("charge", stats.charge_ah, a.charge_ah + b.charge_ah);
    assert_close("energy", stats.energy_wh, a.energy_wh + b.energy_wh);
    assert_mma(
        "vbus",
        &stats.vbus,
        min_max_avg(all.iter().map(|s| s.vbus_v)),
    );

    // An export holds both connections.
    let (tx, rx) = mpsc::channel();
    let mut w = second_worker;
    let rows = w
        .export(Box::new(ChannelWriter(tx)), |_| {})
        .join()
        .unwrap()
        .unwrap();
    assert_eq!(rows, all.len() as u64);
    let csv: Vec<u8> = rx.try_iter().flatten().collect();
    assert_eq!(csv.iter().filter(|&&b| b == b'\n').count(), all.len() + 1);
    // The export does not integrate over the outage either.
    assert_csv_totals(&csv, &stats);
}

// ---- the breaks between runs, for the chart (AC-272WSN.3) ----

#[test]
fn history_gaps_name_every_break_between_runs_inside_the_range() {
    use open_powerz_desktop_lib::history::HistoryGap;
    let mut h = History::new();
    for t in 0..100u32 {
        h.push(&rec(t * 10, 5_000_000, 1_000_000, 0, 0, 0, 0, Rate::Sps50));
    }
    // Unplugged for 40 minutes, then the next connection.
    h.break_run();
    for t in 0..100u32 {
        h.push(&rec(
            2_400_990 + t * 10,
            5_000_000,
            1_000_000,
            0,
            0,
            0,
            0,
            Rate::Sps50,
        ));
    }
    let gap = HistoryGap {
        from_ms: 990,
        to_ms: 2_400_990,
    };
    assert_eq!(h.gaps(0, u64::MAX), vec![gap]);
    // A range that starts or ends inside the gap still overlaps it.
    assert_eq!(h.gaps(1_000, 1_100), vec![gap]);
    assert_eq!(h.gaps(500, 2_400_000), vec![gap]);
    // Ranges on one side of it do not.
    assert_eq!(h.gaps(0, 980), vec![]);
    assert_eq!(h.gaps(2_401_000, u64::MAX), vec![]);
    // One run has no gap; an empty history neither.
    assert_eq!(
        history_of(&random_records(7, 500, 0)).gaps(0, u64::MAX),
        vec![]
    );
    assert_eq!(History::new().gaps(0, u64::MAX), vec![]);
}

#[test]
fn a_reconnect_on_the_same_store_leaves_a_gap_the_chart_can_ask_for() {
    use open_powerz_desktop_lib::history::HistoryGap;
    use open_powerz_desktop_lib::store::SessionStore;
    let clock = clock();
    let store = SessionStore::new();
    let connection = |store: &SessionStore| {
        let pipe = Pipe::sim(
            Scenario::Load5v1a,
            FaultPlan::new(),
            &clock,
            PipeOpts::default(),
        );
        let (events, sink) = capture(&clock);
        let mut worker = Worker::start_with_store(
            pipe,
            clock.clone(),
            config(Rate::Sps1000),
            store.clone(),
            sink,
        )
        .unwrap();
        run_for(&mut worker, &clock, 1_500);
        worker.stop();
        samples_of(&events)
    };
    let first = connection(&store);
    assert!(
        store.history_gaps(0, u64::MAX).is_empty(),
        "one run, no gap"
    );
    clock.advance(Duration::from_secs(30));
    let second = connection(&store);
    let want = HistoryGap {
        from_ms: u64::from(first[first.len() - 1].t_ms),
        to_ms: u64::from(second[0].t_ms),
    };
    assert_eq!(store.history_gaps(0, u64::MAX), vec![want]);
    assert!(
        want.to_ms - want.from_ms >= 30_000,
        "the gap spans the outage: {want:?}"
    );
    let json = serde_json::to_value(want).unwrap();
    assert_eq!(
        json,
        serde_json::json!({ "from_ms": want.from_ms, "to_ms": want.to_ms })
    );
}

/// The CHARGE and ENERGY of the last row of `csv` (8 significant digits).
fn assert_csv_totals(csv: &[u8], stats: &StatsSnapshot) {
    let text = String::from_utf8(csv.to_vec()).unwrap();
    let last = text.lines().last().unwrap();
    let cols: Vec<&str> = last.split(',').collect();
    let charge: f64 = cols[9].parse().unwrap();
    let energy: f64 = cols[10].parse().unwrap();
    let near = |got: f64, want: f64| (got - want).abs() <= 1e-7 * want.abs() + 1e-12;
    assert!(
        near(charge, stats.charge_ah),
        "CSV charge {charge}, stats {}",
        stats.charge_ah
    );
    assert!(
        near(energy, stats.energy_wh),
        "CSV energy {energy}, stats {}",
        stats.energy_wh
    );
}

#[test]
fn a_recording_across_a_clock_jump_matches_the_export_and_the_stats() {
    let clock = clock();
    let pipe = ClockJump {
        inner: Pipe::sim(
            Scenario::Load5v1a,
            FaultPlan::new(),
            &clock,
            PipeOpts::default(),
        ),
        from_poll: 30,
        shift_ms: 5_000,
        polls: 0,
        device_times: std::sync::Arc::default(),
    };
    let (_events, sink) = capture(&clock);
    let mut worker = Worker::start(pipe, clock.clone(), config(Rate::Sps1000), sink).unwrap();
    let (tx, rx) = mpsc::channel();
    worker.start_recording(Box::new(ChannelWriter(tx))).unwrap();
    run_for(&mut worker, &clock, 3_000);
    worker.stop_recording().unwrap();
    worker.stop();
    let recorded: Vec<u8> = rx.try_iter().flatten().collect();
    let (tx, rx) = mpsc::channel();
    worker
        .export(Box::new(ChannelWriter(tx)), |_| {})
        .join()
        .unwrap()
        .unwrap();
    let exported: Vec<u8> = rx.try_iter().flatten().collect();
    assert!(recorded == exported, "recording and export differ");
    assert_csv_totals(&recorded, &worker.stats());
}

#[test]
fn a_rate_change_does_not_integrate_across_the_queue_restart() {
    let clock = clock();
    let pipe = Pipe::sim(
        Scenario::Load5v1a,
        FaultPlan::new(),
        &clock,
        PipeOpts::default(),
    );
    let (events, sink) = capture(&clock);
    let mut worker = Worker::start(pipe, clock.clone(), config(Rate::Sps1000), sink).unwrap();
    run_for(&mut worker, &clock, 1_000);
    let split = worker.history_len();
    worker.set_rate(Rate::Sps50).unwrap();
    run_for(&mut worker, &clock, 1_000);
    worker.stop();
    let all = samples_of(&events);
    assert_eq!(all.len(), worker.history_len());
    assert!(split > 0 && split < all.len());
    let (a, b) = (reference(&all[..split]), reference(&all[split..]));
    let stats = worker.stats();
    assert_close("charge", stats.charge_ah, a.charge_ah + b.charge_ah);
    assert_close("energy", stats.energy_wh, a.energy_wh + b.energy_wh);
}
