/**
 * Timeline navigation (FR FENCD9): pure helpers, no DOM. Collapsed long gaps, the
 * J/K/L shuttle, playhead steps, the zoom slider scale, presets and the overview strip.
 */

import type { GapSpan } from "./chartData";
import type { SessionExtent, TimeRange } from "./chartView";

export type { SessionExtent, TimeRange };

/** Gaps longer than this collapse to a break on screen. */
export const COLLAPSE_MIN_GAP_MS = 60_000;
/** A collapsed gap's width on screen. */
export const BREAK_PX = 24;
/** A collapsed gap is never wider than this. */
export const MAX_BREAK_PX = 32;
/** The narrowest view (the zoom slider's bottom), in ms. */
export const MIN_SPAN_MS = 10;
/** The shuttle's speeds, as multiples of real time. */
export const SHUTTLE_SPEEDS = [1, 2, 4, 8] as const;

/** Session time <-> display coordinate for one view, with long gaps squeezed. */
export interface TimeWarp {
  /** Monotonic non-decreasing. */
  fwd(ms: number): number;
  /** Inverse of `fwd` outside collapsed gaps. */
  bwd(d: number): number;
  /** The collapsed gaps in view. */
  breaks: { fromMs: number; toMs: number }[];
}

const IDENTITY: TimeWarp = { fwd: (ms) => ms, bwd: (d) => d, breaks: [] };

/** The display warp for `view` drawn across `widthPx`: every gap longer than
 * COLLAPSE_MIN_GAP_MS that overlaps the view is squeezed by one factor over its whole
 * length, chosen so a gap fully in view spans BREAK_PX (never more than MAX_BREAK_PX)
 * and a gap partly in view spans its share of that; elsewhere one ms of data is one
 * ms of display (AC-FENCD9.11). A long gap narrower than its break stays as it is. The
 * identity when there is nothing to collapse. */
export function buildWarp(gaps: readonly GapSpan[], view: TimeRange, widthPx: number): TimeWarp {
  const { t0Ms, t1Ms } = view;
  if (!(t1Ms > t0Ms) || !(widthPx > 0)) return IDENTITY;
  const long = gaps
    .filter((g) => g.to_ms - g.from_ms > COLLAPSE_MIN_GAP_MS && g.to_ms > t0Ms && g.from_ms < t1Ms)
    .map((g) => ({ fromMs: g.from_ms, toMs: g.to_ms }))
    .sort((a, b) => a.fromMs - b.fromMs);
  // Overlapping reports of one stretch merge into one break.
  const merged: { fromMs: number; toMs: number }[] = [];
  for (const g of long) {
    const prev = merged[merged.length - 1];
    if (prev !== undefined && g.fromMs <= prev.toMs) prev.toMs = Math.max(prev.toMs, g.toMs);
    else merged.push({ ...g });
  }
  // A long gap no longer than one break would not shrink: it keeps its true width and
  // is neither marked nor given a share of the break budget. Dropping one leaves more
  // display time per break, so repeat until none is left to drop.
  let collapsed = merged;
  let breakMs = 0;
  for (;;) {
    if (collapsed.length === 0) return IDENTITY;
    // Visible fraction of each gap, and the data time left on screen.
    let fractions = 0;
    let visibleGapMs = 0;
    for (const g of collapsed) {
      const visible = Math.min(g.toMs, t1Ms) - Math.max(g.fromMs, t0Ms);
      visibleGapMs += visible;
      fractions += visible / (g.toMs - g.fromMs);
    }
    const dataMs = t1Ms - t0Ms - visibleGapMs;
    if (!(dataMs > 0)) return IDENTITY;
    // Breaks never take more than half the plot between them.
    const breakPx = Math.min(BREAK_PX, MAX_BREAK_PX, widthPx / (2 * fractions));
    // Display time of one whole break: breakPx / widthPx of the warped view.
    breakMs = (breakPx * dataMs) / (widthPx - breakPx * fractions);
    const kept = collapsed.filter((g) => g.toMs - g.fromMs > breakMs);
    if (kept.length === collapsed.length) break;
    collapsed = kept;
  }
  // Per gap, in time order: its ends, its squeeze, the time cut before it and its
  // display start (display origin: real time 0, no gap before), so fwd / bwd find
  // their gap by binary search (uPlot calls them once per point).
  const n = collapsed.length;
  const from = new Float64Array(n);
  const to = new Float64Array(n);
  const scale = new Float64Array(n);
  const cutBefore = new Float64Array(n);
  const starts = new Float64Array(n);
  let removed = 0;
  collapsed.forEach((g, i) => {
    const len = g.toMs - g.fromMs;
    from[i] = g.fromMs;
    to[i] = g.toMs;
    scale[i] = breakMs / len;
    cutBefore[i] = removed;
    starts[i] = g.fromMs - removed;
    removed += (1 - scale[i]!) * len;
  });
  const fwd = (ms: number): number => {
    const i = lastBelow(from, ms);
    if (i < 0) return ms;
    const inside = Math.min(ms, to[i]!) - from[i]!;
    return ms - cutBefore[i]! - (1 - scale[i]!) * inside;
  };
  const bwd = (d: number): number => {
    const i = lastBelow(starts, d);
    if (i < 0) return d;
    const len = to[i]! - from[i]!;
    const displayLen = scale[i]! * len;
    if (d < starts[i]! + displayLen) return from[i]! + (d - starts[i]!) / scale[i]!;
    return d + cutBefore[i]! + len - displayLen;
  };
  return { fwd, bwd, breaks: collapsed };
}

/** The last index whose value is below `v` in ascending `values`, or -1. */
function lastBelow(values: Float64Array, v: number): number {
  let lo = 0;
  let hi = values.length;
  while (lo < hi) {
    const mid = (lo + hi) >>> 1;
    if (values[mid]! < v) lo = mid + 1;
    else hi = mid;
  }
  return lo - 1;
}

/** J/K/L shuttle: the speed after pressing `key`. L from stopped or backwards → 1×,
 * L while playing forwards → the next speed up (8× stays 8×); J mirrors it backwards;
 * K stops (AC-FENCD9.7). Speed is a signed multiple of real time. */
export function shuttlePress(speed: number, key: "J" | "K" | "L"): number {
  if (key === "K") return 0;
  const sign = key === "L" ? 1 : -1;
  const current = speed * sign;
  if (!(current > 0)) return sign * SHUTTLE_SPEEDS[0];
  const top = SHUTTLE_SPEEDS[SHUTTLE_SPEEDS.length - 1] ?? 1;
  return sign * (SHUTTLE_SPEEDS.find((s) => s > current) ?? top);
}

/** Moves the playhead by speed × dtMs, clamped to the session; `reachedEnd` when moving
 * forwards hits the newest data, `reachedStart` when moving backwards hits the session
 * start (AC-FENCD9.7). */
export function shuttleStep(
  playheadMs: number,
  speed: number,
  dtMs: number,
  session: SessionExtent,
): { playheadMs: number; reachedEnd: boolean; reachedStart: boolean } {
  const moved = playheadMs + speed * dtMs;
  const clamped = Math.min(session.lastMs, Math.max(session.firstMs, moved));
  return {
    playheadMs: clamped,
    reachedEnd: speed > 0 && clamped >= session.lastMs,
    reachedStart: speed < 0 && clamped <= session.firstMs,
  };
}

/** ← / →: the playhead moved one pixel's worth of time (view span / `widthPx`), or a
 * tenth of the view with `coarse` (Shift). A null playhead starts at the view centre.
 * Clamped to the session (AC-FENCD9.6). */
export function stepPlayhead(
  playheadMs: number | null,
  direction: -1 | 1,
  view: TimeRange,
  widthPx: number,
  coarse: boolean,
  session: SessionExtent,
): number {
  const span = view.t1Ms - view.t0Ms;
  const from = playheadMs ?? (view.t0Ms + view.t1Ms) / 2;
  const step = coarse ? span / 10 : span / Math.max(1, widthPx);
  return Math.min(session.lastMs, Math.max(session.firstMs, from + direction * step));
}

/** The view moved by the least amount that puts `playheadMs` inside it, kept within the
 * session; the same view when nothing moves (AC-FENCD9.6). */
export function followPlayhead(
  view: TimeRange,
  playheadMs: number,
  session: SessionExtent,
): TimeRange {
  const span = view.t1Ms - view.t0Ms;
  let t0 = view.t0Ms;
  if (playheadMs < t0) t0 = playheadMs;
  else if (playheadMs > view.t1Ms) t0 = playheadMs - span;
  if (t0 + span > session.lastMs) t0 = session.lastMs - span;
  if (t0 < session.firstMs) t0 = session.firstMs;
  return t0 === view.t0Ms ? view : { t0Ms: t0, t1Ms: t0 + span };
}

function clamp01(v: number): number {
  return Math.min(1, Math.max(0, v));
}

/** Zoom slider position (0..1) for a span: logarithmic, 0 ↔ MIN_SPAN_MS, 1 ↔
 * `maxSpanMs` (the whole session); clamped (AC-FENCD9.9). */
export function spanToSlider(spanMs: number, maxSpanMs: number): number {
  if (!(maxSpanMs > MIN_SPAN_MS) || !(spanMs > 0)) return spanMs >= maxSpanMs ? 1 : 0;
  return clamp01(Math.log(spanMs / MIN_SPAN_MS) / Math.log(maxSpanMs / MIN_SPAN_MS));
}

/** The span for a zoom slider position (0..1, clamped) (AC-FENCD9.9). */
export function sliderToSpan(value: number, maxSpanMs: number): number {
  if (!(maxSpanMs > MIN_SPAN_MS)) return Math.max(MIN_SPAN_MS, maxSpanMs);
  const v = Number.isFinite(value) ? clamp01(value) : 0;
  return MIN_SPAN_MS * Math.pow(maxSpanMs / MIN_SPAN_MS, v);
}

/** `span` placed from `t0`, kept inside the session (the whole session when wider). */
function fitSpan(t0: number, span: number, session: SessionExtent): TimeRange {
  if (span >= session.lastMs - session.firstMs)
    return { t0Ms: session.firstMs, t1Ms: session.lastMs };
  const start = Math.min(session.lastMs - span, Math.max(session.firstMs, t0));
  return { t0Ms: start, t1Ms: start + span };
}

/** Paused preset: the preset's span (Whole: the session) centred on `anchorMs`, kept
 * inside the session with its span (AC-FENCD9.10). */
export function presetRange(
  spanMs: number | null,
  anchorMs: number,
  session: SessionExtent,
): TimeRange {
  if (spanMs === null) return { t0Ms: session.firstMs, t1Ms: session.lastMs };
  return fitSpan(anchorMs - spanMs / 2, spanMs, session);
}

/** The overview window for `view` on a strip `widthPx` wide drawn with `warp` built for
 * the whole session: CSS px from the strip's left edge, the view clamped to the session
 * (AC-FENCD9.1). */
export function overviewWindow(
  view: TimeRange,
  session: SessionExtent,
  widthPx: number,
  warp: TimeWarp,
): { leftPx: number; widthPx: number } {
  const d0 = warp.fwd(session.firstMs);
  const d1 = warp.fwd(session.lastMs);
  if (!(d1 > d0) || !(widthPx > 0)) return { leftPx: 0, widthPx: Math.max(0, widthPx) };
  const toPx = (ms: number): number => {
    const t = Math.min(session.lastMs, Math.max(session.firstMs, ms));
    return ((warp.fwd(t) - d0) / (d1 - d0)) * widthPx;
  };
  const left = toPx(view.t0Ms);
  return { leftPx: left, widthPx: Math.max(0, toPx(view.t1Ms) - left) };
}

/** What an overview drag moves: the window, or one of its edges. */
export type OverviewDrag = "move" | "left" | "right";

/** Display units per strip pixel and the session's display start for `warp`. */
function stripScale(
  session: SessionExtent,
  widthPx: number,
  warp: TimeWarp,
): { d0: number; k: number } {
  const d0 = warp.fwd(session.firstMs);
  const d1 = warp.fwd(session.lastMs);
  return { d0, k: widthPx > 0 && d1 > d0 ? (d1 - d0) / widthPx : 0 };
}

/** Range after dragging the overview window (`move`, keeping the span) or one of its
 * edges (`left` / `right`) by `deltaPx`, kept in the session and never narrower than
 * MIN_SPAN_MS (AC-FENCD9.2). */
export function dragOverview(
  start: TimeRange,
  kind: OverviewDrag,
  deltaPx: number,
  session: SessionExtent,
  widthPx: number,
  warp: TimeWarp,
): TimeRange {
  const { k } = stripScale(session, widthPx, warp);
  const shift = (ms: number): number => warp.bwd(warp.fwd(ms) + deltaPx * k);
  if (kind === "move") return fitSpan(shift(start.t0Ms), start.t1Ms - start.t0Ms, session);
  if (kind === "left") {
    const t0 = Math.max(session.firstMs, Math.min(start.t1Ms - MIN_SPAN_MS, shift(start.t0Ms)));
    return { t0Ms: t0, t1Ms: start.t1Ms };
  }
  const t1 = Math.min(session.lastMs, Math.max(start.t0Ms + MIN_SPAN_MS, shift(start.t1Ms)));
  return { t0Ms: start.t0Ms, t1Ms: t1 };
}

/** A click on the strip outside the window: the same span centred on the clicked time,
 * kept in the session (AC-FENCD9.2). */
export function overviewClick(
  xPx: number,
  view: TimeRange,
  session: SessionExtent,
  widthPx: number,
  warp: TimeWarp,
): TimeRange {
  const { d0, k } = stripScale(session, widthPx, warp);
  const at = warp.bwd(d0 + xPx * k);
  const span = view.t1Ms - view.t0Ms;
  return fitSpan(at - span / 2, span, session);
}
