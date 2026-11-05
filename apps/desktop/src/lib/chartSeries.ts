import uPlot from "uplot";
import type { LiveWindowId } from "./chartData";
import { dashAnchorStepMs, traceDashes } from "./dashPath";
import { drawLoopCap } from "./drawCap";
import { hasPlotArea } from "./plotInsets";
import { DASH, MINUS, formatFixed } from "./format";
import {
  DEFAULT_CHART_STYLE,
  axisPresets,
  seriesLooks,
  strokeColor,
  type ChartStyle,
  type RangePreset,
} from "./chartStyle";
import { THEME_COLORS, type ResolvedTheme, type ThemeStorage } from "./theme";

/** One plotted channel. */
export type ChartSeriesKey = "vbus" | "ibus" | "power" | "dp" | "dm" | "cc1" | "cc2";
/** Value axis (and uPlot scale id) a series is drawn against. */
export type ChartAxisId = "volts" | "amps" | "watts";

export interface ChartSeriesDef {
  key: ChartSeriesKey;
  label: string;
  unit: "V" | "A" | "W";
  axis: ChartAxisId;
  /** Key into the theme palette's `chart` tokens. */
  token: string;
}

export interface ChartAxisDef {
  id: ChartAxisId;
  unit: "V" | "A" | "W";
  /** Axis title: a word plus its unit, so it still reads when drawn rotated. */
  title: string;
  side: "left" | "right";
}

/** Plotted series, in uPlot series order (after x). */
export const CHART_SERIES: readonly ChartSeriesDef[] = [
  { key: "vbus", label: "VBUS", unit: "V", axis: "volts", token: "voltage" },
  { key: "ibus", label: "IBUS", unit: "A", axis: "amps", token: "current" },
  { key: "power", label: "Power", unit: "W", axis: "watts", token: "power" },
  { key: "dp", label: "D+", unit: "V", axis: "volts", token: "dPlus" },
  { key: "dm", label: "D−", unit: "V", axis: "volts", token: "dMinus" },
  { key: "cc1", label: "CC1", unit: "V", axis: "volts", token: "cc1" },
  { key: "cc2", label: "CC2", unit: "V", axis: "volts", token: "cc2" },
];

/** Volts on the left, amps on the right; watts (power only) also on the right. */
export const CHART_AXES: readonly ChartAxisDef[] = [
  { id: "volts", unit: "V", title: "Volts (V)", side: "left" },
  { id: "amps", unit: "A", title: "Amps (A)", side: "right" },
  { id: "watts", unit: "W", title: "Watts (W)", side: "right" },
];

/** The definition of one series. */
export function seriesDef(key: ChartSeriesKey): ChartSeriesDef | undefined {
  return CHART_SERIES.find((s) => s.key === key);
}

/** How a session time is written: seconds, minutes and seconds, or hours too. */
type TimeStyle = "s" | "m" | "h";

/** `|ms|` in seconds, rounded to `decimals` places. */
function roundedSeconds(ms: number, decimals: number): number {
  const scale = 10 ** decimals;
  return Math.round((Math.abs(ms) / 1000) * scale) / scale;
}

/** `ms` (session-relative) in `style` with `decimals` places on the seconds; true minus
 * sign before the time. */
function formatTime(ms: number, decimals: number, style: TimeStyle): string {
  if (!Number.isFinite(ms)) return DASH;
  const total = roundedSeconds(ms, decimals);
  const sign = ms < 0 && total > 0 ? MINUS : "";
  if (style === "s") return `${sign}${total.toFixed(decimals)} s`;
  const h = Math.floor(total / 3600);
  const m = Math.floor((total - h * 3600) / 60);
  const sec = (total - h * 3600 - m * 60)
    .toFixed(decimals)
    .padStart(decimals > 0 ? 3 + decimals : 2, "0");
  return style === "h" ? `${sign}${h}:${String(m).padStart(2, "0")}:${sec}` : `${sign}${m}:${sec}`;
}

/** The style a time of `ms` (at `decimals`) needs: seconds under a minute, m:ss under an hour. */
function styleFor(ms: number, decimals: number): TimeStyle {
  const total = roundedSeconds(ms, decimals);
  return total < 60 ? "s" : total < 3600 ? "m" : "h";
}

/** One session time (ms since the session start) in the unit its size needs:
 * `12.345 s`, `1:02.345`, `1:02:03.456` (AC-272WSN.1). */
export function formatSessionTime(ms: number, decimals: number): string {
  return formatTime(ms, decimals, styleFor(ms, decimals));
}

/** A visible span as the zoom readout writes it, in the time axis's format: `10 s`,
 * `2:30`, `40:23`, `1:02:03`; shorter than 10 s with the decimals it needs (`2.5 s`,
 * `0.25 s`). */
export function formatSpan(ms: number): string {
  if (!Number.isFinite(ms)) return DASH;
  if (ms >= 9_999.5) return formatDuration(ms);
  return formatSessionTime(ms, ms < 1_000 ? 2 : 1);
}

/** A length of time in whole seconds: `45 s`, `40:00`, `1:02:03`. */
export function formatDuration(ms: number): string {
  return formatSessionTime(Math.round(ms / 1000) * 1000, 0);
}

/** Time-axis labels for session-relative `splits` spaced `incrMs`: one style for the
 * whole axis, picked by its largest time (AC-272WSN.1). */
export function timeTicks(splits: readonly number[], incrMs: number): string[] {
  const decimals = tickDecimals(incrMs);
  let widest = 0;
  for (const v of splits) if (Number.isFinite(v)) widest = Math.max(widest, Math.abs(v));
  const style = styleFor(widest, decimals);
  return splits.map((v) => formatTime(v, decimals, style));
}

/** Most ticks one axis draws; a range that needs more gets none rather than a stall. */
const MAX_TICKS = 1_000;

/** Tick positions every `incrMs` counted from `originMs`, inside `[min, max]`. */
export function timeSplits(min: number, max: number, incrMs: number, originMs: number): number[] {
  if (!(incrMs > 0) || !Number.isFinite(min) || !Number.isFinite(max) || !(max >= min)) return [];
  const first = Math.ceil((min - originMs) / incrMs);
  const last = Math.floor((max - originMs) / incrMs);
  if (last - first + 1 > MAX_TICKS) return [];
  const out: number[] = [];
  for (let k = first; k <= last; k++) out.push(originMs + k * incrMs);
  return out;
}

/** Time-axis steps: decimal fractions of a second, then seconds, minutes and hours. */
export const TIME_INCRS: readonly number[] = [
  1, 2, 5, 10, 20, 50, 100, 200, 500, 1_000, 2_000, 5_000, 10_000, 15_000, 30_000, 60_000, 120_000,
  300_000, 600_000, 900_000, 1_800_000, 3_600_000, 7_200_000, 10_800_000, 21_600_000, 43_200_000,
  86_400_000,
];

/** Width of a value axis in CSS px: its widest tick label plus the ticks and their gap,
 * so the rotated title beside it never covers a label (AC-272WSN.6). uPlot measures
 * with its own scaled font; later sizing passes keep the first pass's size. */
export function axisSize(
  self: uPlot,
  values: readonly string[] | null,
  axisIdx: number,
  cycleNum: number,
  pxRatio: number = globalThis.devicePixelRatio || 1,
): number {
  // `_size` and the scaled `font` are uPlot's own resolved fields (uplot 1.6, pinned in
  // package.json); the "axis width" tests drive this function through them.
  const axis = self.axes[axisIdx] as uPlot.Axis & { _size?: number; font?: string[] };
  if (cycleNum > 1 && axis._size !== undefined) return axis._size;
  let size = (axis.ticks?.size ?? 10) + (axis.gap ?? 5);
  let longest = "";
  for (const v of values ?? []) if (v.length > longest.length) longest = v;
  if (longest !== "") {
    const before = self.ctx.font;
    self.ctx.font = axis.font?.[0] ?? before;
    size += self.ctx.measureText(longest).width / pxRatio;
    self.ctx.font = before;
  }
  return Math.ceil(size);
}

/** Decimals a seconds label needs for ticks `incrMs` apart (at most ms precision). */
export function tickDecimals(incrMs: number): number {
  if (!(incrMs > 0)) return 0;
  let d = 0;
  // Fewest decimals that write the step exactly (0.025 s needs 3, 0.5 s needs 1).
  while (d < 3 && Math.abs(Math.round(incrMs / 10 ** (3 - d)) - incrMs / 10 ** (3 - d)) > 1e-9) d++;
  return d;
}

/** Value-axis tick labels: as many decimals as the spacing `incr` needs (at most 6), with
 * the true minus sign for negatives. */
export function axisTicks(splits: readonly number[], incr: number): string[] {
  let d = 0;
  while (d < 6 && incr > 0 && Math.abs(Math.round(incr * 10 ** d) - incr * 10 ** d) > 1e-6) d++;
  return splits.map((v) => formatFixed(v, d));
}

/** Per-series on/off choice. */
export type SeriesVisibility = Record<ChartSeriesKey, boolean>;

/** Every series shown except power, which is hidden by default. */
export function defaultVisibility(): SeriesVisibility {
  const v = {} as SeriesVisibility;
  for (const s of CHART_SERIES) v[s.key] = s.key !== "power";
  return v;
}

/** Copy of `v` with `key` flipped. */
export function toggleSeries(v: SeriesVisibility, key: ChartSeriesKey): SeriesVisibility {
  return { ...v, [key]: !v[key] };
}

/** VBUS, IBUS and power: what a chart of a saved recording starts on. */
export const RECORDING_VISIBILITY = Object.fromEntries(
  CHART_SERIES.map((s) => [s.key, s.key === "vbus" || s.key === "ibus" || s.key === "power"]),
) as SeriesVisibility;

/** The size a plot of a saved recording takes in `host`. */
export function recordingPlotSize(host: HTMLElement): { width: number; height: number } {
  return { width: Math.max(host.clientWidth, 100), height: Math.max(host.clientHeight, 100) };
}

/** Keeps `plot` the size of `host`, at most once a frame; returns what stops it. */
export function followRecordingPlotSize(plot: uPlot, host: HTMLElement): () => void {
  let frame = 0;
  const observer = new ResizeObserver(() => {
    if (frame) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      plot.setSize(recordingPlotSize(host));
    });
  });
  observer.observe(host);
  return () => {
    observer.disconnect();
    if (frame) cancelAnimationFrame(frame);
  };
}

/** Store `value` as JSON under `key`; storage failures are ignored (the in-memory choice
 * stays, it just won't survive a reload). */
function saveJson(key: string, value: unknown, storage: ThemeStorage | undefined): void {
  try {
    storage?.setItem(key, JSON.stringify(value));
  } catch {
    // storage unavailable: keep the in-memory choice
  }
}

/** Storage key for the legend's on/off choice (session storage: it lasts for the session). */
export const SERIES_VISIBILITY_KEY = "open-powerz.chart.visibility";

/** Stored choice merged over the defaults; missing, corrupt or unreadable storage gives the defaults. */
export function loadVisibility(storage: ThemeStorage | undefined): SeriesVisibility {
  const v = defaultVisibility();
  try {
    const raw = storage?.getItem(SERIES_VISIBILITY_KEY);
    if (raw == null) return v;
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== "object" || parsed === null) return v;
    const stored = parsed as Record<string, unknown>;
    for (const s of CHART_SERIES) {
      const value = stored[s.key];
      if (typeof value === "boolean") v[s.key] = value;
    }
    return v;
  } catch {
    return defaultVisibility();
  }
}

/** Persist the choice; storage failures are ignored (the choice just won't survive a reload). */
export function saveVisibility(v: SeriesVisibility, storage: ThemeStorage | undefined): void {
  saveJson(SERIES_VISIBILITY_KEY, v, storage);
}

/** Storage key for the "Zero-based axes" choice (session storage, like the legend). */
export const ZERO_BASED_KEY = "open-powerz.chart.zeroBased";

/** The stored "Zero-based axes" choice; off when missing, corrupt or unreadable. */
export function loadZeroBased(storage: ThemeStorage | undefined): boolean {
  try {
    const raw = storage?.getItem(ZERO_BASED_KEY);
    return raw == null ? false : JSON.parse(raw) === true;
  } catch {
    return false;
  }
}

/** Persist the "Zero-based axes" choice; storage failures are ignored. */
export function saveZeroBased(on: boolean, storage: ThemeStorage | undefined): void {
  saveJson(ZERO_BASED_KEY, on, storage);
}

/** Storage key for the chosen live window (session storage, like the legend) (AC-AKNVND.5). */
export const LIVE_WINDOW_KEY = "open-powerz.chart.liveWindow";

/** Every live window id (kept here: `chartData` imports this module). */
const LIVE_WINDOW_IDS: readonly LiveWindowId[] = ["10s", "1m", "10m", "all"];

/** The stored live window; 10 s when missing, corrupt, unknown or unreadable. */
export function loadLiveWindow(storage: ThemeStorage | undefined): LiveWindowId {
  try {
    const raw = storage?.getItem(LIVE_WINDOW_KEY);
    if (raw == null) return "10s";
    const id: unknown = JSON.parse(raw);
    return LIVE_WINDOW_IDS.find((w) => w === id) ?? "10s";
  } catch {
    return "10s";
  }
}

/** Persist the chosen live window; storage failures are ignored. */
export function saveLiveWindow(id: LiveWindowId, storage: ThemeStorage | undefined): void {
  saveJson(LIVE_WINDOW_KEY, id, storage);
}

/** A value axis range that always includes zero (AC-AH2AEM.9): from min(0, dataMin) to
 * max(0, dataMax), padded the way uPlot pads an auto-scaled axis, so a small ripple is
 * drawn to scale and negative values (reverse current) stay in view. */
export function zeroBasedRange(dataMin: number | null, dataMax: number | null): [number, number] {
  const lo = Math.min(0, Number.isFinite(dataMin) ? (dataMin as number) : 0);
  const hi = Math.max(0, Number.isFinite(dataMax) ? (dataMax as number) : 0);
  const [min, max] = uPlot.rangeNum(lo, hi, 0.1, true);
  // `rangeNum` writes an exact zero as -0.
  // Typed as nullable, but finite for finite input.
  const zero = (v: number | null): number => (v === null || v === 0 ? 0 : v);
  return [zero(min), zero(max)];
}

/** The narrowest span each value axis shows with zero-based axes off (AC-CWWKJ6.2), so
 * noise around a flat signal stays small instead of filling the plot. */
export const MIN_AXIS_SPAN: Readonly<Record<ChartAxisId, number>> = {
  volts: 0.1,
  amps: 0.02,
  watts: 0.1,
};

/** An auto-scaled value axis range at least `minSpan` wide (AC-CWWKJ6.2): data that spans
 * less is centred in a `minSpan` window; wider data autoscales as uPlot does. */
export function minSpanRange(
  dataMin: number | null,
  dataMax: number | null,
  minSpan: number,
): [number, number] {
  // uPlot passes null bounds for an axis with no data yet.
  if (!Number.isFinite(dataMin) || !Number.isFinite(dataMax)) return [-minSpan / 2, minSpan / 2];
  const lo = dataMin as number;
  const hi = dataMax as number;
  const centred = centredMinSpan(lo, hi, minSpan);
  if (centred) return centred;
  const [min, max] = uPlot.rangeNum(lo, hi, 0.1, true);
  return [min ?? lo, max ?? hi];
}

/** The overview strip's per-series y-range (AC-CWWKJ6.3): at least the series' axis
 * minimum span, centred, so an idle output reads flat; wider data keeps its raw extent. */
export function envelopeRange(key: "vbus" | "ibus", min: number, max: number): [number, number] {
  return centredMinSpan(min, max, MIN_AXIS_SPAN[key === "vbus" ? "volts" : "amps"]) ?? [min, max];
}

/** A `minSpan`-wide window centred on finite data spanning less than `minSpan`, else null. */
function centredMinSpan(min: number, max: number, minSpan: number): [number, number] | null {
  if (!(Number.isFinite(min) && Number.isFinite(max) && max - min < minSpan)) return null;
  const mid = (min + max) / 2;
  return [mid - minSpan / 2, mid + minSpan / 2];
}

/** The browser's session storage, if reachable. */
export function sessionStore(): ThemeStorage | undefined {
  try {
    return window.sessionStorage;
  } catch {
    return undefined;
  }
}

/** Axes that carry at least one visible series. */
export function visibleAxes(v: SeriesVisibility): ChartAxisId[] {
  return CHART_AXES.filter((a) => CHART_SERIES.some((s) => s.axis === a.id && v[s.key])).map(
    (a) => a.id,
  );
}

/** One pattern family per series so lines stay apart at 1–2 px without colour. */
const DASHES: Record<ChartSeriesKey, number[]> = {
  vbus: [], // solid
  ibus: [12, 4], // long dash
  power: [2, 3], // dot
  dp: [10, 3, 2, 3], // dash-dot
  dm: [4, 4], // short dash
  cc1: [12, 3, 2, 3, 2, 3], // dash-dot-dot
  cc2: [6, 3, 6, 3, 2, 3], // dash-dash-dot
};

export interface SeriesStyle {
  key: ChartSeriesKey;
  stroke: string;
  dash: number[];
}

/** Line colour (from the theme's chart tokens) and dash for every series, in CHART_SERIES order. */
export function seriesStyles(theme: ResolvedTheme): SeriesStyle[] {
  const palette = THEME_COLORS[theme].chart;
  return CHART_SERIES.map((s) => ({
    key: s.key,
    stroke: palette[s.token] ?? THEME_COLORS[theme].text,
    dash: DASHES[s.key],
  }));
}

/** Line width (CSS px) and dash pattern (CSS px) for one series at device pixel ratio `dpr`. */
export interface SeriesStroke {
  width: number;
  dash: number[];
}

/** Line width every series aims for, in CSS px, before snapping to device pixels. */
export const LINE_WIDTH = 1.5;

/** Width and dash for `key` at `dpr`, snapped to whole device pixels (AC-AKNVND.1): at
 * DPR 1 a line is 2 px wide, so it is never a 1 px blend with the grid line under it, and
 * every dash and gap is a whole pixel, so dashed series keep their colour. */
export function seriesStroke(
  key: ChartSeriesKey,
  dpr: number,
  width: number = LINE_WIDTH,
): SeriesStroke {
  const ratio = Number.isFinite(dpr) && dpr > 0 ? dpr : 1;
  const snap = (cssPx: number): number => Math.max(1, Math.round(cssPx * ratio)) / ratio;
  return { width: snap(width), dash: DASHES[key].map(snap) };
}

/** The earliest data time the dashed series' patterns restart from for a view of
 * `xMin..xMax` drawn over `widthPx` device px at `dpr`: the grid time at or before
 * `xMin` of the coarsest dash grid (FR SX1VTC). A frame that holds the samples from it
 * lays the first cell in view from its own start, as the previous frame did, so the
 * dashes left in view do not jump. `xMin` itself when nothing can be anchored. */
export function dashGridStartMs(xMin: number, xMax: number, widthPx: number, dpr: number): number {
  const ratio = Number.isFinite(dpr) && dpr > 0 ? dpr : 1;
  let from = xMin;
  for (const key of Object.keys(DASHES) as ChartSeriesKey[]) {
    const pattern = seriesStroke(key, ratio).dash.map((d) => d * ratio);
    const step = dashAnchorStepMs(pattern, xMin, xMax, widthPx);
    if (step > 0) from = Math.min(from, Math.floor(xMin / step) * step);
  }
  return from;
}

/** The one dash setting every series hands uPlot: none. Shared, so uPlot sets it once. */
const NO_DASH: number[] = [];

/** Device-pixel positions of the series being stroked, shared by every series. */
const SCRATCH_X: (number | null)[] = [];
const SCRATCH_Y: (number | null)[] = [];
const SCRATCH_T: (number | null)[] = [];

/** Far above float noise in a plot position, far below anything visible: a sample
 * position that is a half pixel give or take noise always rounds up (FR SX1VTC). */
const HALF_PX_NUDGE = 1e-6;

/** A uPlot paths builder that strokes the series itself, straight on the plot's context,
 * in `color` at `deviceWidth` and dashed with `devicePattern` (device px; empty: solid),
 * over the visible window `idx0..idx1`; nulls are gaps. It hands uPlot no path back.
 *
 * WebKit never tells its garbage collector what a `Path2D` holds, so the paths of a live
 * chart (new ones every frame) pile up in native memory, solid or dashed alike (FR 4NSVC9).
 * This builds no `Path2D` and sets no canvas dash: the dashes are solid pieces of one
 * plain context path (AC-4NSVC9.3). */
export function dashedPaths(
  devicePattern: readonly number[],
  color = "",
  deviceWidth = 0,
): uPlot.Series.PathBuilder {
  return (u, seriesIdx, idx0, idx1) => {
    const ctx = u.ctx;
    const xData = u.data[0];
    const yData = u.data[seriesIdx];
    const scale = u.series[seriesIdx]?.scale ?? "y";
    if (!ctx || !xData || !yData || idx1 < idx0 || deviceWidth <= 0 || !color) return null;
    // A plot with no area (hidden at 1 × 1, or padded past its size) has nothing to show,
    // and its points land far off it (FR C6VR9K).
    if (!hasPlotArea(u.bbox)) return null;
    // The pattern restarts at grid times fixed in data time, so the dashes stay on their
    // samples while the view scrolls (FR SX1VTC). The window starts at the sample at or
    // before the grid time left of the view, so the first cell is laid from its own start.
    // A plot without an x scale (laid out by hand) dashes from its first point.
    const xScale = (u.scales as uPlot["scales"] | undefined)?.x;
    const xMin = xScale?.min;
    const stepMs = dashAnchorStepMs(devicePattern, xMin, xScale?.max, u.bbox.width);
    if (stepMs > 0 && xMin != null) {
      const grid = Math.floor(xMin / stepMs) * stepMs;
      // Bounded: the walk back takes at most one cell of samples, and never more than
      // the frame's draw cap, whatever the data hold (AC-SX1VTC.5).
      let back = drawLoopCap(u.bbox.width, u.bbox.height);
      while (idx0 > 0 && back-- > 0) {
        const t = xData[idx0];
        if (t != null && Number.isFinite(t) && t <= grid) break;
        idx0--;
      }
    }
    const count = idx1 - idx0 + 1;
    // Reused from frame to frame: a live chart makes no new arrays per draw.
    const xs = SCRATCH_X;
    const ys = SCRATCH_Y;
    const ts = SCRATCH_T;
    xs.length = count;
    ys.length = count;
    ts.length = count;
    for (let i = 0; i < count; i++) {
      const x = xData[idx0 + i];
      const y = yData[idx0 + i];
      ts[i] = x ?? null;
      const ok = x != null && y != null && Number.isFinite(x) && Number.isFinite(y);
      // Whole device pixels, as uPlot's own linear paths round them. A pixel-snapped
      // scroll can put a sample exactly half way between two pixels, where float noise
      // would round it one way on one frame and the other way on the next; the nudge
      // rounds it the same way every frame, so the dashes do not twitch (AC-SX1VTC.6).
      xs[i] = ok ? Math.round(u.valToPos(x, "x", true) + HALF_PX_NUDGE) : null;
      ys[i] = ok ? Math.round(u.valToPos(y, scale, true)) : null;
    }
    const { left, top, width, height } = u.bbox;
    // An odd width sits on pixel centres, as uPlot aligns its own strokes.
    const offset = (deviceWidth % 2) / 2;
    ctx.save();
    ctx.beginPath();
    const clipLeft = left - deviceWidth / 2;
    const clipTop = top - deviceWidth / 2;
    const clipWidth = width + deviceWidth;
    const clipHeight = height + deviceWidth;
    ctx.rect(clipLeft, clipTop, clipWidth, clipHeight);
    ctx.clip();
    ctx.translate(offset, offset);
    ctx.strokeStyle = color;
    ctx.lineWidth = deviceWidth;
    ctx.lineJoin = "round";
    ctx.lineCap = "butt";
    ctx.setLineDash(NO_DASH);
    ctx.beginPath();
    traceDashes(
      ctx,
      xs,
      ys,
      devicePattern,
      // The box the canvas is clipped to: points far off it cost nothing (AC-C6VR9K.4).
      {
        left: clipLeft,
        top: clipTop,
        right: clipLeft + clipWidth,
        bottom: clipTop + clipHeight,
      },
      // No layout fault makes one frame stroke without end (AC-C6VR9K.5).
      drawLoopCap(width, height),
      ts,
      stepMs,
    );
    ctx.stroke();
    ctx.restore();
    return null;
  };
}

export interface ChartOptionsInput {
  theme: ResolvedTheme;
  visibility: SeriesVisibility;
  width: number;
  height: number;
  /** Every value axis includes zero (AC-AH2AEM.9); off means auto-scale. */
  zeroBased?: boolean;
  /** The session start in x units (ms): the time axis counts from it (AC-272WSN.1). */
  originMs?: () => number;
  /** Device pixel ratio the plot is drawn at; line widths and dashes snap to it. */
  pxRatio?: number;
  /** Draws the current axis upside down; the values stay as measured (AC-AXHK2T.7). */
  reverseCurrent?: boolean;
  /** The chart's appearance settings: the axis range presets (AC-AXHK2V.1). */
  style?: ChartStyle;
}

/** The range of value axis `axis` over the data `dataMin`..`dataMax` under its preset
 * (AC-AXHK2V.1): a fixed preset shows `[0, max]`, Auto from zero includes zero, Auto
 * follows `zeroBased`. Shared by the live chart and the SVG export. */
export function presetRange(
  axis: ChartAxisId,
  preset: RangePreset | undefined,
  dataMin: number | null,
  dataMax: number | null,
  zeroBased = false,
): [number, number] {
  if (preset?.kind === "fixed" && preset.max !== undefined) return [0, preset.max];
  if (zeroBased || preset?.kind === "auto0") return zeroBasedRange(dataMin, dataMax);
  return minSpanRange(dataMin, dataMax, MIN_AXIS_SPAN[axis]);
}

/** Whether two tick-split arrays hold the same values in the same order. */
function sameSplits(a: readonly number[], b: readonly number[]): boolean {
  if (a.length !== b.length) return false;
  for (let k = 0; k < a.length; k++) if (a[k] !== b[k]) return false;
  return true;
}

/**
 * An axis `values` callback that keeps the labels of the last tick set (AC-AXHK2W.5):
 * a redraw with the same splits, step and `origin()` gets the same label array back,
 * so a steady stream makes no new labels per frame.
 */
function labelCache(
  make: (splits: number[], incr: number, origin: number) => string[],
  origin: () => number = () => 0,
): (u: uPlot, splits: number[], axis: number, space: number, incr: number) => string[] {
  let lastSplits: number[] = [];
  let lastIncr = NaN;
  let lastOrigin = NaN;
  let labels: string[] | null = null;
  return (_u, splits, _axis, _space, incr) => {
    const o = origin();
    if (labels !== null && incr === lastIncr && o === lastOrigin && sameSplits(splits, lastSplits))
      return labels;
    lastSplits = splits.slice();
    lastIncr = incr;
    lastOrigin = o;
    labels = make(splits, incr, o);
    return labels;
  };
}

/** uPlot options: x = ms since the session start (labelled in seconds), then one series per CHART_SERIES entry on its axis scale. */
export function chartOptions({
  theme,
  visibility,
  width,
  height,
  zeroBased = false,
  originMs = () => 0,
  pxRatio = globalThis.devicePixelRatio || 1,
  reverseCurrent = false,
  style = DEFAULT_CHART_STYLE,
}: ChartOptionsInput): uPlot.Options {
  const presets = axisPresets(style);
  const p = THEME_COLORS[theme];
  const looks = seriesLooks(theme, style);
  const shown = visibleAxes(visibility);
  const series: uPlot.Series[] = [
    {},
    ...CHART_SERIES.map((s, i) => {
      const look = looks[i];
      const stroke = seriesStroke(s.key, pxRatio, look?.width);
      const color = look ? strokeColor(look.color, look.opacity) : p.text;
      return {
        label: s.label,
        scale: s.axis,
        show: visibility[s.key],
        stroke: color,
        width: stroke.width,
        // Never a canvas dash and never a Path2D: WebKit leaks memory with both
        // (AC-4NSVC9.3). Every series strokes itself, a dashed one as solid pieces.
        dash: NO_DASH,
        spanGaps: false,
        paths: dashedPaths(
          stroke.dash.map((d) => d * pxRatio),
          color,
          stroke.width * pxRatio,
        ),
      };
    }),
  ];
  const axes: uPlot.Axis[] = [
    {
      stroke: p.muted,
      incrs: [...TIME_INCRS],
      splits: (_u, _axis, min, max, incr) => timeSplits(min, max, incr, originMs()),
      values: labelCache(
        (splits, incr, origin) =>
          timeTicks(
            splits.map((v) => v - origin),
            incr,
          ),
        originMs,
      ),
    },
    ...CHART_AXES.map((a) => ({
      scale: a.id,
      side: a.side === "left" ? 3 : 1,
      show: shown.includes(a.id),
      label: a.title,
      // A little air between the rotated title and its tick labels.
      labelGap: 6,
      values: labelCache((splits, incr) => axisTicks(splits, incr)),
      size: (u: uPlot, values: string[] | null, axisIdx: number, cycleNum: number) =>
        axisSize(u, values, axisIdx, cycleNum),
      stroke: p.muted,
      grid: { show: a.id === "volts", stroke: p.border },
    })),
  ];
  return {
    width,
    height,
    series,
    axes,
    scales: {
      x: { time: false },
      ...Object.fromEntries(
        CHART_AXES.map((a): [string, uPlot.Scale] => [
          a.id,
          {
            range: (_u, min, max) => presetRange(a.id, presets[a.id], min, max, zeroBased),
            ...(reverseCurrent && a.id === "amps" ? { dir: -1 as const } : {}),
          },
        ]),
      ),
    },
    // The component draws its own legend (toggle buttons) and hover readout.
    legend: { show: false },
    // An Alt-drag selects an x range (the component zooms to it, paused); a plain drag
    // scrubs the playhead and a double-click fits the session, both handled by the
    // component, so uPlot's own double-click reset is off.
    cursor: {
      drag: { x: true, y: false, setScale: false },
      bind: { dblclick: () => null },
    },
  };
}
