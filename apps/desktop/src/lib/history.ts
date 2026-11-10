/**
 * The session history and statistics the browser-side backends keep: the mock backend
 * (`VITE_BACKEND=mock`) and the web backend (`VITE_BACKEND=web`, FR QFTCX0) share them.
 * They follow `src-tauri/src/history.rs` and `src-tauri/src/stats.rs`.
 */

import type {
  HistoryGap,
  HistorySeries,
  MinMaxAvg,
  RangeStats,
  RunMark,
  RunProgress,
  RunProgressQuery,
  Sample,
  Series,
  SeriesBuckets,
  StatsSnapshot,
} from "./backend";
import { widenToNeighbours } from "./chartData";

/** Running min / max / sum of one quantity. */
class RunningMinMax {
  min = Infinity;
  max = -Infinity;
  sum = 0;

  push(x: number): void {
    this.min = Math.min(this.min, x);
    this.max = Math.max(this.max, x);
    this.sum += x;
  }

  snapshot(n: number): MinMaxAvg | null {
    return n > 0 ? { min: this.min, max: this.max, avg: this.sum / n } : null;
  }
}

/** Statistics over the samples since the last reset, like `src-tauri/src/stats.rs`:
 * charge and energy by the trapezoid rule, never across a reset. */
export class SessionStats {
  #first: number | null = null;
  #last: Sample | null = null;
  #samples = 0;
  #chargeAh = 0;
  #energyWh = 0;
  #vbus = new RunningMinMax();
  #ibus = new RunningMinMax();
  #power = new RunningMinMax();

  push(s: Sample): void {
    const prev = this.#last;
    if (prev) {
      const dtH = (s.t_ms - prev.t_ms) / 3_600_000;
      this.#chargeAh += ((prev.ibus_a + s.ibus_a) / 2) * dtH;
      this.#energyWh += ((prev.vbus_v * prev.ibus_a + s.vbus_v * s.ibus_a) / 2) * dtH;
    }
    this.#first ??= s.t_ms;
    this.#last = s;
    this.#samples += 1;
    this.#vbus.push(s.vbus_v);
    this.#ibus.push(s.ibus_a);
    this.#power.push(s.vbus_v * s.ibus_a);
  }

  snapshot(): StatsSnapshot {
    const n = this.#samples;
    return {
      elapsed_ms: this.#first === null || !this.#last ? 0 : this.#last.t_ms - this.#first,
      samples: n,
      charge_ah: this.#chargeAh,
      energy_wh: this.#energyWh,
      vbus: this.#vbus.snapshot(n),
      ibus: this.#ibus.snapshot(n),
      power: this.#power.snapshot(n),
    };
  }
}

const SERIES_FIELD: Record<Series, keyof Sample> = {
  vbus: "vbus_v",
  ibus: "ibus_a",
  dp: "dp_v",
  dm: "dm_v",
  cc1: "cc1_v",
  cc2: "cc2_v",
};

const FIELDS = Object.keys(SERIES_FIELD) as Series[];

/** Spare room left after a synthetic seed, so live samples append without a copy. */
export const SEED_HEADROOM = 1 << 20;

/** A sample's time (ms), current (A) and power (W), for integrating. */
interface RunPoint {
  t: number;
  i: number;
  p: number;
}

const MS_PER_HOUR = 3_600_000;

/** The point at time `t` on the line through `a` and `b`, as the trapezoid rule assumes. */
function toward(a: RunPoint, b: RunPoint, t: number): RunPoint {
  const w = b.t !== a.t ? (t - a.t) / (b.t - a.t) : 0;
  return { t, i: a.i + (b.i - a.i) * w, p: a.p + (b.p - a.p) * w };
}

/** Charge (Ah) and energy (Wh) from `a` to `b` by the trapezoid rule. */
function trapezoid(a: RunPoint, b: RunPoint): [number, number] {
  const h = Math.max(0, b.t - a.t) / MS_PER_HOUR;
  return [((a.i + b.i) / 2) * h, ((a.p + b.p) / 2) * h];
}

/** The most E.D. points and marks one answer holds, as in the Rust backend. */
const MAX_ED_POINTS = 8192;
const MAX_MARKS = 1000;

/** The percentages to mark at every `stepPct`, 0% and 100% left out. */
function markPercents(stepPct: number): number[] {
  if (!(stepPct > 0 && stepPct < 100)) return [];
  const out: number[] = [];
  for (let k = 1; k <= MAX_MARKS && k * stepPct < 100 - 1e-9; k++) out.push(k * stepPct);
  return out;
}

/** `q.ed_points` evenly spaced times over the E.D. window clamped to `runT0..runT1` (the
 * scope's samples). */
function edTimes(q: RunProgressQuery, runT0: number, runT1: number): number[] {
  const n = Math.min(MAX_ED_POINTS, Math.max(0, Math.floor(q.ed_points)));
  const x = q.ed_t0_ms ?? runT0;
  const y = q.ed_t1_ms ?? runT1;
  const a = Math.max(Math.min(x, y), runT0);
  const b = Math.min(Math.max(x, y), runT1);
  if (!(n > 0) || a > b) return [];
  if (n === 1) return [b];
  const step = (b - a) / (n - 1);
  return Array.from({ length: n }, (_, k) => (k === n - 1 ? b : a + step * k));
}

/** The session history as columns (sample times plus one column per series), so a
 * range is found by binary search and bucketed without building sample objects:
 * a pan over a 10-million-sample history stays well inside a frame (AC-AH2AEM.4). */
export class HistoryColumns {
  length = 0;
  #t = new Float64Array(0);
  /** Float32 values (plenty for a demo double) keep a 10M seed near 320 MB; an opened
   * recording file keeps its readings at full (Float64) precision, as the desktop does. */
  #cols = {} as Record<Series, Float32Array | Float64Array>;
  readonly #Col: Float32ArrayConstructor | Float64ArrayConstructor;

  constructor(capacity = 1024, precision: "f32" | "f64" = "f32") {
    this.#Col = precision === "f64" ? Float64Array : Float32Array;
    this.#alloc(capacity);
  }

  #alloc(capacity: number): void {
    const t = new Float64Array(capacity);
    t.set(this.#t.subarray(0, this.length));
    this.#t = t;
    for (const name of FIELDS) {
      const col = new this.#Col(capacity);
      const old = this.#cols[name] as Float32Array | Float64Array | undefined;
      if (old) col.set(old.subarray(0, this.length));
      this.#cols[name] = col;
    }
  }

  push(s: Sample): void {
    if (this.length === this.#t.length) this.#alloc(Math.max(1024, this.#t.length * 2));
    const i = this.length++;
    this.#t[i] = s.t_ms;
    for (const name of FIELDS) this.#cols[name][i] = s[SERIES_FIELD[name]] as number;
  }

  /** Appends `n` samples, one per millisecond from 0, straight into the columns: `write`
   * fills the reused `out` with the reading at `tMs` (no sample object per point). */
  fillSynthetic(n: number, write: (tMs: number, out: Sample) => Sample): void {
    if (this.length + n > this.#t.length) this.#alloc(this.length + n + SEED_HEADROOM);
    const s: Sample = { t_ms: 0, vbus_v: 0, ibus_a: 0, cc1_v: 0, cc2_v: 0, dp_v: 0, dm_v: 0 };
    for (let t = 0; t < n; t++) {
      write(t, s);
      const i = this.length++;
      this.#t[i] = t;
      for (const name of FIELDS) this.#cols[name][i] = s[SERIES_FIELD[name]] as number;
    }
  }

  /** Sample `j` as a {@link Sample} (column precision). */
  sampleAt(j: number): Sample {
    const s: Sample = {
      t_ms: this.#t[j] ?? 0,
      vbus_v: 0,
      ibus_a: 0,
      cc1_v: 0,
      cc2_v: 0,
      dp_v: 0,
      dm_v: 0,
    };
    for (const name of FIELDS) s[SERIES_FIELD[name]] = this.#cols[name][j] ?? 0;
    return s;
  }

  /** Sample `i`'s time, ms. */
  timeAt(i: number): number {
    return this.#t[i] ?? 0;
  }

  /** The first index whose time is `>= t` (or `> t` when `after`). */
  #search(t: number, after: boolean): number {
    let lo = 0;
    let hi = this.length;
    while (lo < hi) {
      const mid = (lo + hi) >>> 1;
      const v = this.#t[mid] as number;
      if (v < t || (after && v === t)) lo = mid + 1;
      else hi = mid;
    }
    return lo;
  }

  /** Series `name` of sample `j`; Power is VBUS × IBUS of that sample, so it takes no
   * column of its own. */
  #value(name: HistorySeries, j: number): number {
    if (name !== "power") return this.#cols[name][j] as number;
    return (this.#cols.vbus[j] as number) * (this.#cols.ibus[j] as number);
  }

  /** Sample `j`'s time, current and power. */
  #pt(j: number): RunPoint {
    const v = this.#cols.vbus[j] as number;
    const i = this.#cols.ibus[j] as number;
    return { t: this.#t[j] as number, i, p: v * i };
  }

  /** Sample `j` starts a new run: the interval before it is (most of) one of `gaps`. */
  #startsRun(j: number, gaps: readonly HistoryGap[]): boolean {
    if (j <= 0 || j >= this.length || gaps.length === 0) return false;
    const a = this.#t[j - 1] as number;
    const b = this.#t[j] as number;
    return gaps.some(
      (g) => Math.min(b, g.to_ms) - Math.max(a, g.from_ms) >= (g.to_ms - g.from_ms) / 2,
    );
  }

  /** The ruler's stats over the samples with `t0 <= t <= t1` (either order), as the Rust
   * `range_stats` counts them: charge and energy by the trapezoid rule over `t0..t1`
   * itself (the parts from each end to its nearest sample follow the line to the sample
   * just outside), nothing across one of `gaps`; plain averages of the samples in range;
   * `null` with no sample in range (FR AXHK2T). */
  rangeStats(a: number, b: number, gaps: readonly HistoryGap[] = []): RangeStats | null {
    const t0 = Math.min(a, b);
    const t1 = Math.max(a, b);
    const lo = this.#search(t0, false);
    const hi = this.#search(t1, true);
    if (hi <= lo) return null;
    let chargeAh = 0;
    let energyWh = 0;
    let sv = 0;
    let si = 0;
    let sp = 0;
    let vMin = Infinity;
    let vMax = -Infinity;
    let iMin = Infinity;
    let iMax = -Infinity;
    let pMin = Infinity;
    let pMax = -Infinity;
    let prev: RunPoint | null = null;
    for (let j = lo; j < hi; j++) {
      const cur = this.#pt(j);
      const v = this.#cols.vbus[j] as number;
      vMin = Math.min(vMin, v);
      vMax = Math.max(vMax, v);
      iMin = Math.min(iMin, cur.i);
      iMax = Math.max(iMax, cur.i);
      pMin = Math.min(pMin, cur.p);
      pMax = Math.max(pMax, cur.p);
      if (prev !== null && !this.#startsRun(j, gaps)) {
        const [dq, de] = trapezoid(prev, cur);
        chargeAh += dq;
        energyWh += de;
      }
      prev = cur;
      sv += this.#cols.vbus[j] as number;
      si += cur.i;
      sp += cur.p;
    }
    if (lo > 0 && !this.#startsRun(lo, gaps)) {
      const inner = this.#pt(lo);
      const [dq, de] = trapezoid(toward(this.#pt(lo - 1), inner, t0), inner);
      chargeAh += dq;
      energyWh += de;
    }
    if (hi < this.length && !this.#startsRun(hi, gaps)) {
      const inner = this.#pt(hi - 1);
      const [dq, de] = trapezoid(inner, toward(inner, this.#pt(hi), t1));
      chargeAh += dq;
      energyWh += de;
    }
    const n = hi - lo;
    return {
      samples: n,
      charge_ah: chargeAh,
      energy_wh: energyWh,
      vbus_avg_v: sv / n,
      ibus_avg_a: si / n,
      power_avg_w: sp / n,
      vbus_min_v: vMin,
      vbus_max_v: vMax,
      ibus_min_a: iMin,
      ibus_max_a: iMax,
      power_min_w: pMin,
      power_max_w: pMax,
    };
  }

  /** Segment marks of the current run (the samples after the last of `gaps`) and E.D. of
   * the session (or the run, per `ed_scope`), from the raw samples, as the Rust
   * `run_progress` does; `null` with no sample. */
  runProgress(q: RunProgressQuery, gaps: readonly HistoryGap[] = []): RunProgress | null {
    if (this.length === 0) return null;
    let start = 0;
    for (const g of gaps) {
      const j = this.#search((g.from_ms + g.to_ms) / 2, false);
      if (j > start && j < this.length) start = j;
    }
    const from = q.ed_scope === "run" ? start : 0;
    const runT0 = this.#t[start] as number;
    const runT1 = this.#t[this.length - 1] as number;
    const edT = edTimes(q, this.#t[from] as number, runT1);
    const edWh: number[] = [];
    let ah = 0;
    let wh = 0;
    let beforeRun: [number, number] = [0, 0];
    let prev: RunPoint | null = null;
    for (let j = from; j < this.length; j++) {
      if (j === start) beforeRun = [ah, wh];
      if (j > from && this.#startsRun(j, gaps)) prev = null;
      const b = this.#pt(j);
      while (edWh.length < edT.length && (edT[edWh.length] as number) <= b.t) {
        const t = edT[edWh.length] as number;
        edWh.push(wh + (prev === null ? 0 : trapezoid(prev, toward(prev, b, t))[1]));
      }
      if (prev !== null) {
        const [dq, de] = trapezoid(prev, b);
        ah += dq;
        wh += de;
      }
      prev = b;
    }
    while (edWh.length < edT.length) edWh.push(wh);
    const edTotalWh = wh;
    const chargeAh = ah - beforeRun[0];
    const energyWh = wh - beforeRun[1];
    const followed = q.kind === "charge" ? chargeAh : energyWh;
    const marksTotalPositive = followed > 0 && Number.isFinite(followed);
    const marks: RunMark[] = [];
    const percents = marksTotalPositive ? markPercents(q.step_pct) : [];
    let last: { t: number; c: number } | null = null;
    let c = 0;
    let before: RunPoint | null = null;
    for (let j = start; j < this.length && marks.length < percents.length; j++) {
      const b = this.#pt(j);
      if (before !== null) c += trapezoid(before, b)[q.kind === "charge" ? 0 : 1];
      before = b;
      while (marks.length < percents.length) {
        const percent = percents[marks.length] as number;
        const target = (followed * percent) / 100;
        if (c < target) break;
        const tMs: number =
          last !== null && last.c < target
            ? last.t + ((target - last.c) / (c - last.c)) * (b.t - last.t)
            : b.t;
        marks.push({ t_ms: tMs, percent });
      }
      last = { t: b.t, c };
    }
    const energyTotalPositive = edTotalWh > 0 && Number.isFinite(edTotalWh);
    return {
      session_t0_ms: this.#t[0] as number,
      run_t0_ms: runT0,
      run_t1_ms: runT1,
      samples: this.length - start,
      charge_ah: chargeAh,
      energy_wh: energyWh,
      marks_total_positive: marksTotalPositive,
      marks,
      ed_total_wh: edTotalWh,
      energy_total_positive: energyTotalPositive,
      ed_t_ms: energyTotalPositive ? edT : [],
      ed_pct: energyTotalPositive ? edWh.map((e) => (100 * e) / edTotalWh) : [],
    };
  }

  /** The first `n` samples as CSV text: time, VBUS and IBUS. */
  csvHead(n: number): string {
    const lines = ["time_ms,vbus_v,ibus_a"];
    for (let j = 0; j < Math.min(n, this.length); j++) {
      lines.push(`${this.#t[j]},${this.#cols.vbus[j]},${this.#cols.ibus[j]}`);
    }
    return lines.join("\n");
  }

  /** Min / max buckets over `t0..=t1`, at most `buckets` per series, plus the sample
   * just outside each edge that falls between samples, so lines run to the view's
   * edges (AC-9CFMD3.4). */
  buckets(
    series: readonly HistorySeries[],
    t0: number,
    t1: number,
    buckets: number,
  ): SeriesBuckets[] {
    const [from, to] = widenToNeighbours(
      this.#search(t0, false),
      this.#search(t1, true),
      this.length,
      (i) => this.#t[i] as number,
      t0,
      t1,
    );
    const n = Math.max(0, to - from);
    const per = buckets > 0 ? Math.max(1, Math.ceil(n / buckets)) : 0;
    return series.map((name) => {
      const out: SeriesBuckets = { series: name, buckets: [] };
      if (per === 0) return out;
      for (let i = from; i < to; i += per) {
        const end = Math.min(to, i + per);
        let min = Infinity;
        let max = -Infinity;
        for (let j = i; j < end; j++) {
          const v = this.#value(name, j);
          if (v < min) min = v;
          if (v > max) max = v;
        }
        out.buckets.push({ t_ms: this.#t[i] as number, min, max });
      }
      return out;
    });
  }
}
