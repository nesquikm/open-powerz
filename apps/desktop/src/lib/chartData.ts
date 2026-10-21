import type uPlot from "uplot";
import type { Gap, HistoryGap, Sample, SeriesBuckets } from "./backend";
import {
  CHART_SERIES,
  formatDuration,
  type ChartSeriesKey,
  type SeriesVisibility,
} from "./chartSeries";

/** A stretch without samples: a stream gap, an outage, or a break between runs the
 * history reports. */
export interface GapSpan extends HistoryGap {
  /** What a stream event says caused it; the history does not know. */
  kind?: Gap["kind"];
}

/** The index range `[lo, hi)` of the samples in `[t0Ms, t1Ms]`, widened by the sample
 * just outside each end that falls between samples, so lines run to the view's edges at
 * any zoom (AC-9CFMD3.4); an end on a sample, or past the first or last, stays put.
 * `timeAt(i)` is sample `i`'s time, for `0 <= i < len`. */
export function widenToNeighbours(
  lo: number,
  hi: number,
  len: number,
  timeAt: (i: number) => number,
  t0Ms: number,
  t1Ms: number,
): [number, number] {
  if (t1Ms < t0Ms) return [lo, hi];
  if (lo > 0 && (lo === len || timeAt(lo) > t0Ms)) lo--;
  if (hi < len && (hi === 0 || timeAt(hi - 1) < t1Ms)) hi++;
  return [lo, hi];
}

/** Live ring capacity: 10 minutes at 1000 SPS. */
export const LIVE_CAPACITY = 600_000;

export type LiveWindowId = "10s" | "1m" | "10m" | "all";

export interface LiveWindow {
  id: LiveWindowId;
  label: string;
  /** Span shown, `null` for the whole session. */
  ms: number | null;
}

/** The live windows the selector offers. */
export const LIVE_WINDOWS: readonly LiveWindow[] = [
  { id: "10s", label: "10 s", ms: 10_000 },
  { id: "1m", label: "1 min", ms: 60_000 },
  { id: "10m", label: "10 min", ms: 600_000 },
  { id: "all", label: "Whole session", ms: null },
];

/** What the chart draws: shared x (ms) and one y array per series (`null` = no point). */
export interface ChartFrame {
  x: number[];
  y: Record<ChartSeriesKey, (number | null)[]>;
  /** Points stand for min/max of a time bucket, not raw samples. */
  decimated: boolean;
}

/** Stored channels, power is derived (VBUS × IBUS). */
type StoredKey = "vbus" | "ibus" | "dp" | "dm" | "cc1" | "cc2";

function emptyY(): Record<ChartSeriesKey, (number | null)[]> {
  const y = {} as Record<ChartSeriesKey, (number | null)[]>;
  for (const s of CHART_SERIES) y[s.key] = [];
  return y;
}

/** Most gaps a {@link GapList} keeps; older ones go first. */
export const MAX_GAPS = 10_000;

/** Share of each gap two reports of one stretch overlap by, at least. */
const SAME_STRETCH = 0.9;

/** `a` and `b` describe the same stretch without data: identical, or overlapping by at
 * least {@link SAME_STRETCH} of each one's length. */
function sameStretch(a: GapSpan, b: GapSpan): boolean {
  if (a.from_ms === b.from_ms && a.to_ms === b.to_ms) return true;
  const overlap = Math.min(a.to_ms, b.to_ms) - Math.max(a.from_ms, b.from_ms);
  if (!(overlap > 0)) return false;
  return (
    overlap >= SAME_STRETCH * (a.to_ms - a.from_ms) &&
    overlap >= SAME_STRETCH * (b.to_ms - b.from_ms)
  );
}

/** A session's gaps sorted by start, capped at the newest `cap`; ranges are found by
 * binary search, so drawing never rescans the whole list. */
export class GapList {
  readonly cap: number;
  #gaps: GapSpan[] = [];
  /** The longest gap ever added, so a range query knows how far back one can start. */
  #longestMs = 0;
  #version = 0;

  constructor(cap: number = MAX_GAPS) {
    this.cap = Math.max(1, Math.floor(cap));
  }

  get length(): number {
    return this.#gaps.length;
  }

  /** Changes whenever the gaps do, so a caller can cache what it built from them. */
  get version(): number {
    return this.#version;
  }

  /** Adds `gap`, unless the list already has the same stretch: the stream and the
   * history can both report one outage, a millisecond apart, and it gets one band. */
  push(gap: GapSpan): void {
    // A gap without finite ends has no place in a sorted list.
    if (!Number.isFinite(gap.from_ms) || !Number.isFinite(gap.to_ms)) return;
    const gaps = this.#gaps;
    const at = this.#firstFrom(gap.from_ms);
    for (const i of [at - 1, at]) {
      const g = gaps[i];
      if (g && sameStretch(g, gap)) {
        // A report the list already covers changes nothing.
        if (g.from_ms <= gap.from_ms && g.to_ms >= gap.to_ms && (g.kind ?? gap.kind) === g.kind)
          return;
        // The widened gap may sort earlier or reach another report: take it out and
        // add it again, so the list stays sorted and merges what it now covers.
        gaps.splice(i, 1);
        this.push({
          ...g,
          kind: g.kind ?? gap.kind,
          from_ms: Math.min(g.from_ms, gap.from_ms),
          to_ms: Math.max(g.to_ms, gap.to_ms),
        });
        return;
      }
    }
    this.#longestMs = Math.max(this.#longestMs, gap.to_ms - gap.from_ms);
    this.#version++;
    // Gaps arrive in time order, so this is almost always an append.
    if (gaps.length === 0 || (gaps[gaps.length - 1]?.from_ms ?? -Infinity) <= gap.from_ms) {
      gaps.push(gap);
    } else {
      gaps.splice(this.#firstFrom(gap.from_ms), 0, gap);
    }
    if (gaps.length > this.cap) gaps.splice(0, gaps.length - this.cap);
  }

  clear(): void {
    this.#gaps = [];
    this.#longestMs = 0;
    this.#version++;
  }

  /** Forgets the gaps starting before `ms`. */
  dropBefore(ms: number): void {
    const i = this.#firstFrom(ms);
    if (i > 0) {
      this.#gaps.splice(0, i);
      this.#version++;
    }
  }

  /** The gaps starting in [t0Ms, t1Ms], oldest first. */
  within(t0Ms: number, t1Ms: number): GapSpan[] {
    let i = this.#firstFrom(t0Ms);
    const out: GapSpan[] = [];
    for (; i < this.#gaps.length; i++) {
      const g = this.#gaps[i];
      if (!g || g.from_ms > t1Ms) break;
      out.push(g);
    }
    return out;
  }

  /** The gaps with some of their span inside (t0Ms, t1Ms), oldest first; also one that
   * began before t0Ms. */
  overlapping(t0Ms: number, t1Ms: number): GapSpan[] {
    // A gap overlapping the range starts at most the longest gap's length before it,
    // even when reports of different lengths overlap one another.
    let i = this.#firstFrom(t0Ms - this.#longestMs);
    const out: GapSpan[] = [];
    for (; i < this.#gaps.length; i++) {
      const g = this.#gaps[i];
      if (!g || g.from_ms >= t1Ms) break;
      if (g.to_ms > t0Ms) out.push(g);
    }
    return out;
  }

  /** First index whose gap starts at or after `ms`. */
  #firstFrom(ms: number): number {
    let a = 0;
    let b = this.#gaps.length;
    while (a < b) {
      const m = (a + b) >>> 1;
      if ((this.#gaps[m]?.from_ms ?? 0) < ms) a = m + 1;
      else b = m;
    }
    return a;
  }
}

/** Where a gap's break point sits: between its last sample before (`from_ms`) and the
 * first sample after (`nextMs`), so x stays sorted. */
function breakAt(gap: GapSpan, nextMs: number): number {
  return (gap.from_ms + Math.max(gap.from_ms, Math.min(gap.to_ms, nextMs))) / 2;
}

/** `v`, or `null` when it is not a finite number (no point is drawn). */
function finite(v: number | undefined): number | null {
  return v !== undefined && Number.isFinite(v) ? v : null;
}

/** Number of series a frame carries, and their channels in CHART_SERIES order. */
const SERIES_COUNT = CHART_SERIES.length;

/** Fixed-capacity ring of the newest samples, one typed array per channel. */
export class LiveBuffer {
  readonly capacity: number;
  #t: Float64Array;
  #ch: Record<StoredKey, Float64Array>;
  /** Index of the oldest sample. */
  #head = 0;
  #len = 0;
  /** Gaps between buffered samples, oldest first (AC-AH2AEM.7). */
  #gaps = new GapList();
  /** Per-series min/max scratch for the reduction (reused across frames). */
  #min = new Float64Array(SERIES_COUNT);
  #max = new Float64Array(SERIES_COUNT);
  #minAt = new Int32Array(SERIES_COUNT);
  #maxAt = new Int32Array(SERIES_COUNT);

  constructor(capacity: number = LIVE_CAPACITY) {
    this.capacity = Math.max(1, Math.floor(capacity));
    this.#t = new Float64Array(this.capacity);
    this.#ch = {
      vbus: new Float64Array(this.capacity),
      ibus: new Float64Array(this.capacity),
      dp: new Float64Array(this.capacity),
      dm: new Float64Array(this.capacity),
      cc1: new Float64Array(this.capacity),
      cc2: new Float64Array(this.capacity),
    };
  }

  get length(): number {
    return this.#len;
  }

  get firstMs(): number | null {
    return this.#len === 0 ? null : (this.#t[this.#head] ?? null);
  }

  get lastMs(): number | null {
    return this.#len === 0 ? null : (this.#t[this.#slot(this.#len - 1)] ?? null);
  }

  /** Drops every sample (a new session starts). */
  clear(): void {
    this.#head = 0;
    this.#len = 0;
    this.#gaps.clear();
  }

  /** The stream stopped after the sample at `from_ms` and resumed at `to_ms`: the line
   * breaks there. */
  markGap(gap: GapSpan): void {
    this.#gaps.push(gap);
  }

  /** Appends a batch; the oldest samples go once the ring is full. */
  push(samples: readonly Sample[]): void {
    const cap = this.capacity;
    // Only the newest `cap` samples can survive.
    const from = Math.max(0, samples.length - cap);
    for (let k = from; k < samples.length; k++) {
      const s = samples[k];
      if (!s) continue;
      let slot: number;
      if (this.#len < cap) {
        slot = this.#slot(this.#len);
        this.#len++;
      } else {
        slot = this.#head;
        this.#head = (this.#head + 1) % cap;
      }
      this.#write(slot, s);
    }
    // Gaps that start before the oldest sample no longer sit between two samples.
    const first = this.firstMs;
    if (first !== null) this.#gaps.dropBefore(first);
  }

  /** Puts `samples` (time-ordered) before the oldest buffered sample, keeping only those
   * older than it and only as many as still fit: the buffered samples are newer, so
   * they always stay (FR VEDDFD backfill). */
  prepend(samples: readonly Sample[]): void {
    const first = this.firstMs;
    const older = first === null ? samples : samples.filter((s) => s.t_ms < first);
    const room = this.capacity - this.#len;
    const take = older.slice(Math.max(0, older.length - room));
    if (take.length === 0) return;
    const cap = this.capacity;
    this.#head = (((this.#head - take.length) % cap) + cap) % cap;
    this.#len += take.length;
    for (const [k, s] of take.entries()) this.#write(this.#slot(k), s);
  }

  /** Stores one sample's time and channels in ring slot `slot`. */
  #write(slot: number, s: Sample): void {
    this.#t[slot] = s.t_ms;
    this.#ch.vbus[slot] = s.vbus_v;
    this.#ch.ibus[slot] = s.ibus_a;
    this.#ch.dp[slot] = s.dp_v;
    this.#ch.dm[slot] = s.dm_v;
    this.#ch.cc1[slot] = s.cc1_v;
    this.#ch.cc2[slot] = s.cc2_v;
  }

  /** Samples in [t0Ms, t1Ms], plus the one just outside each edge that falls between
   * samples, so lines run to the view's edges at any zoom (AC-9CFMD3.4); reduced to at
   * most two points per pixel of `widthPx` (each pixel's min and max, in time order)
   * when there are more than that. A gap breaks the line after its `from_ms` sample;
   * non-finite values draw no point. */
  window(t0Ms: number, t1Ms: number, widthPx: number): ChartFrame {
    const t = this.#t;
    const timeAt = (i: number): number => t[this.#slot(i)] ?? 0;
    const [lo, hi] = widenToNeighbours(
      this.#lowerBound(t0Ms),
      this.#upperBound(t1Ms), // exclusive
      this.#len,
      timeAt,
      t0Ms,
      t1Ms,
    );
    const count = Math.max(0, hi - lo);
    const pixels = Number.isFinite(widthPx) ? Math.max(1, Math.floor(widthPx)) : 1;
    const y = emptyY();
    const x: number[] = [];
    const g0 = count > 0 ? Math.min(t0Ms, timeAt(lo)) : t0Ms;
    const g1 = count > 0 ? Math.max(t1Ms, timeAt(hi - 1)) : t1Ms;
    const gaps = this.#gaps.within(g0, g1).filter((gap) => gap.from_ms < g1);
    const cap = this.capacity;
    let g = 0;
    /** Null points for every gap starting before `ms`: uPlot breaks the line there. */
    const breaksBefore = (ms: number, points: number): void => {
      while (g < gaps.length) {
        const gap = gaps[g];
        if (!gap || gap.from_ms >= ms) break;
        pushBreak(x, y, breakAt(gap, ms), points);
        g++;
      }
    };
    if (count <= pixels * 2) {
      const { vbus, ibus, dp, dm, cc1, cc2 } = this.#ch;
      let slot = this.#slot(lo);
      for (let i = lo; i < hi; i++) {
        const ms = t[slot] ?? 0;
        breaksBefore(ms, 1);
        x.push(ms);
        const v = vbus[slot] ?? NaN;
        const a = ibus[slot] ?? NaN;
        y.vbus.push(finite(v));
        y.ibus.push(finite(a));
        y.power.push(finite(v * a));
        y.dp.push(finite(dp[slot]));
        y.dm.push(finite(dm[slot]));
        y.cc1.push(finite(cc1[slot]));
        y.cc2.push(finite(cc2[slot]));
        if (++slot === cap) slot = 0;
      }
      return { x, y, decimated: false };
    }
    const scale = pixels / Math.max(t1Ms - t0Ms, Number.EPSILON);
    const bucketOf = (ms: number): number => Math.min(pixels - 1, Math.floor((ms - t0Ms) * scale));
    let i = lo;
    let slot = this.#slot(lo);
    while (i < hi) {
      const firstMs = t[slot] ?? 0;
      breaksBefore(firstMs, 2);
      const bucket = bucketOf(firstMs);
      // A bucket never spans a gap: samples up to its `from_ms` belong before the break.
      const gapFrom = gaps[g]?.from_ms ?? Infinity;
      this.#resetExtremes();
      let lastMs = firstMs;
      let n = 0;
      while (i < hi) {
        const ms = t[slot] ?? 0;
        if (n > 0 && (ms > gapFrom || bucketOf(ms) !== bucket)) break;
        this.#take(slot, n);
        lastMs = ms;
        n++;
        i++;
        if (++slot === cap) slot = 0;
      }
      x.push(firstMs, lastMs);
      this.#pushExtremes(y);
    }
    return { x, y, decimated: true };
  }

  #resetExtremes(): void {
    this.#min.fill(Infinity);
    this.#max.fill(-Infinity);
    this.#minAt.fill(0);
    this.#maxAt.fill(0);
  }

  /** Folds the sample in `slot` (the bucket's `n`-th) into every series' extremes at once. */
  #take(slot: number, n: number): void {
    const ch = this.#ch;
    const v = ch.vbus[slot] ?? NaN;
    const a = ch.ibus[slot] ?? NaN;
    // CHART_SERIES order: vbus, ibus, power, dp, dm, cc1, cc2.
    this.#fold(0, v, n);
    this.#fold(1, a, n);
    this.#fold(2, v * a, n);
    this.#fold(3, ch.dp[slot] ?? NaN, n);
    this.#fold(4, ch.dm[slot] ?? NaN, n);
    this.#fold(5, ch.cc1[slot] ?? NaN, n);
    this.#fold(6, ch.cc2[slot] ?? NaN, n);
  }

  #fold(k: number, v: number, n: number): void {
    // NaN fails both comparisons; ±Infinity is skipped explicitly.
    if (v === Infinity || v === -Infinity) return;
    if (v < (this.#min[k] ?? Infinity)) {
      this.#min[k] = v;
      this.#minAt[k] = n;
    }
    if (v > (this.#max[k] ?? -Infinity)) {
      this.#max[k] = v;
      this.#maxAt[k] = n;
    }
  }

  /** The bucket's two points per series: its extremes in time order, `null` if none. */
  #pushExtremes(y: ChartFrame["y"]): void {
    for (let k = 0; k < SERIES_COUNT; k++) {
      const key = CHART_SERIES[k]?.key;
      if (!key) continue;
      const min = this.#min[k] ?? Infinity;
      const max = this.#max[k] ?? -Infinity;
      const ys = y[key];
      if (min > max) ys.push(null, null);
      else if ((this.#minAt[k] ?? 0) <= (this.#maxAt[k] ?? 0)) ys.push(min, max);
      else ys.push(max, min);
    }
  }

  /** Ring slot of the logical index `i` (0 = oldest). */
  #slot(i: number): number {
    return (this.#head + i) % this.capacity;
  }

  /** First logical index with t >= ms. */
  #lowerBound(ms: number): number {
    let a = 0;
    let b = this.#len;
    while (a < b) {
      const m = (a + b) >>> 1;
      if ((this.#t[this.#slot(m)] ?? 0) < ms) a = m + 1;
      else b = m;
    }
    return a;
  }

  /** First logical index with t > ms. */
  #upperBound(ms: number): number {
    let a = 0;
    let b = this.#len;
    while (a < b) {
      const m = (a + b) >>> 1;
      if ((this.#t[this.#slot(m)] ?? 0) <= ms) a = m + 1;
      else b = m;
    }
    return a;
  }
}

/** The live window's [t0, t1] in ms: the last `span` up to the newest sample (never
 * before the first one), or the whole buffer; `null` before the first sample. */
export function liveRange(id: LiveWindowId, buf: LiveBuffer): [number, number] | null {
  const first = buf.firstMs;
  const last = buf.lastMs;
  if (first === null || last === null) return null;
  const ms = LIVE_WINDOWS.find((w) => w.id === id)?.ms ?? null;
  return ms === null ? [first, last] : [Math.max(first, last - ms), last];
}

/** Reduced motion: `range` (a `spanMs` live window) advanced in tenth-of-a-span steps
 * instead of scrolling; the end never passes the newest sample, so no blank strip
 * shows, and the start never precedes `firstMs`. A window still shorter than its span
 * (the session is younger) is returned as it is. */
export function steppedRange(
  range: [number, number],
  spanMs: number,
  firstMs: number,
): [number, number] {
  if (range[1] - range[0] < spanMs || !(spanMs > 0)) return range;
  const step = spanMs / 10;
  const end = Math.floor(range[1] / step) * step;
  return [Math.max(firstMs, end - spanMs), end];
}

/** `points` null points at `ms` (two in decimated frames, which pair their points). */
function pushBreak(x: number[], y: ChartFrame["y"], ms: number, points: number): void {
  for (let k = 0; k < points; k++) {
    x.push(ms);
    for (const s of CHART_SERIES) y[s.key].push(null);
  }
}

/** uPlot aligned data: x, then one array per series in CHART_SERIES order. */
export function toAlignedData(frame: ChartFrame): uPlot.AlignedData {
  return [frame.x, ...CHART_SERIES.map((s) => frame.y[s.key])];
}

/** A `history_range` answer as a chart frame: two points per bucket (its min, then its
 * max) at the bucket's time; series the answer does not carry, and non-finite values,
 * stay `null`. Each gap inside the answer's span adds a null pair after the bucket at
 * its `from_ms`, so the line breaks there. */
export function bucketsToFrame(
  result: readonly SeriesBuckets[],
  gaps: readonly GapSpan[] = [],
): ChartFrame {
  const times = new Set<number>();
  for (const s of result) for (const b of s.buckets) times.add(b.t_ms);
  const sorted = [...times].sort((a, b) => a - b);
  const firstT = sorted[0] ?? 0;
  const lastT = sorted[sorted.length - 1] ?? 0;
  const breaks = gaps
    .filter((gap) => sorted.length > 0 && gap.from_ms >= firstT && gap.from_ms < lastT)
    .sort((a, b) => a.from_ms - b.from_ms);
  const x: number[] = [];
  /** Frame index of each bucket time's min point. */
  const index = new Map<number, number>();
  let g = 0;
  for (const t of sorted) {
    while (g < breaks.length) {
      const gap = breaks[g];
      if (!gap || gap.from_ms >= t) break;
      const ms = breakAt(gap, t);
      x.push(ms, ms);
      g++;
    }
    index.set(t, x.length);
    x.push(t, t);
  }
  const y = emptyY();
  for (const s of CHART_SERIES) y[s.key] = new Array<number | null>(x.length).fill(null);
  for (const s of result) {
    const ys = y[s.series];
    for (const b of s.buckets) {
      const i = index.get(b.t_ms);
      if (i === undefined) continue;
      ys[i] = finite(b.min);
      ys[i + 1] = finite(b.max);
    }
  }
  return { x, y, decimated: true };
}

/** A gap the chart shades: its span clipped to the view, and its whole length. */
export interface GapBand {
  fromMs: number;
  toMs: number;
  label: string;
}

/** The gaps of `gaps` visible in [minMs, maxMs], as shaded bands (AC-272WSN.5). */
export function gapBands(gaps: GapList, minMs: number, maxMs: number): GapBand[] {
  return gaps.overlapping(minMs, maxMs).map((g) => ({
    fromMs: Math.max(g.from_ms, minMs),
    toMs: Math.min(g.to_ms, maxMs),
    label: gapLabel(g),
  }));
}

/** Labels already written, per gap: the chart asks again on every redraw. */
const GAP_LABELS = new WeakMap<GapSpan, string>();

/** "No data · <length>" for `g`, written once. */
function gapLabel(g: GapSpan): string {
  let label = GAP_LABELS.get(g);
  if (label === undefined) {
    label = `No data · ${formatDuration(g.to_ms - g.from_ms)}`;
    GAP_LABELS.set(g, label);
  }
  return label;
}

/** A band's label is drawn only when it fits inside the band with 8 CSS px to spare
 * (`textPx` and `bandPx` in device pixels, `pxRatio` device pixels per CSS pixel). */
export function gapLabelFits(textPx: number, bandPx: number, pxRatio: number): boolean {
  return textPx + 8 * pxRatio < bandPx;
}

/** One series' value under the cursor: a sample's value, or a bucket's min–max range. */
export type HoverValue =
  | { key: ChartSeriesKey; kind: "value"; value: number }
  | { key: ChartSeriesKey; kind: "range"; min: number; max: number };

/** What the hover readout shows: the point's time and each visible series' value. */
export interface HoverReadout {
  t_ms: number;
  values: HoverValue[];
}

/** No series has a value at index `i` (a gap break, or an all-bad bucket). */
function blankAt(frame: ChartFrame, i: number): boolean {
  for (const s of CHART_SERIES) {
    const v = frame.y[s.key][i];
    if (v !== null && v !== undefined) return false;
  }
  return true;
}

/** Index of the x nearest `atMs` (x is sorted) that is not blank; -1 when there is none. */
function nearestIndex(frame: ChartFrame, atMs: number): number {
  const x = frame.x;
  let a = 0;
  let b = x.length;
  while (a < b) {
    const m = (a + b) >>> 1;
    if ((x[m] ?? 0) < atMs) a = m + 1;
    else b = m;
  }
  let left = a - 1;
  let right = a;
  while (left >= 0 && blankAt(frame, left)) left--;
  while (right < x.length && blankAt(frame, right)) right++;
  if (left < 0) return right < x.length ? right : -1;
  if (right >= x.length) return left;
  return atMs - (x[left] ?? 0) <= (x[right] ?? 0) - atMs ? left : right;
}

/** The readout at `atMs`: the nearest sample's values, or (decimated view) the nearest
 * bucket's min–max per series; hidden and missing series are left out; `null` when empty. */
export function hoverReadout(
  frame: ChartFrame,
  atMs: number,
  visibility: SeriesVisibility,
): HoverReadout | null {
  const i = nearestIndex(frame, atMs);
  if (i < 0) return null;
  // Decimated frames carry two points per bucket: indices 2k and 2k+1.
  const lo = frame.decimated ? i - (i % 2) : i;
  const hi = frame.decimated ? Math.min(lo + 1, frame.x.length - 1) : i;
  const values: HoverValue[] = [];
  for (const s of CHART_SERIES) {
    if (!visibility[s.key]) continue;
    const ys = frame.y[s.key];
    let min = Infinity;
    let max = -Infinity;
    for (let k = lo; k <= hi; k++) {
      const v = ys[k];
      if (v === null || v === undefined || !Number.isFinite(v)) continue;
      if (v < min) min = v;
      if (v > max) max = v;
    }
    if (min > max) continue;
    values.push(
      min === max
        ? { key: s.key, kind: "value", value: min }
        : { key: s.key, kind: "range", min, max },
    );
  }
  return { t_ms: frame.x[lo] ?? atMs, values };
}
