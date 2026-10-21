import uPlot from "uplot";
import { formatFixed } from "./format";
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

/** A session-relative time (ms since the session start) in seconds, e.g. `12.5 s`. */
export function formatSessionSeconds(ms: number, decimals: number): string {
  return `${(ms / 1000).toFixed(decimals)} s`;
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
  try {
    storage?.setItem(SERIES_VISIBILITY_KEY, JSON.stringify(v));
  } catch {
    // storage unavailable: keep the in-memory choice
  }
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
  try {
    storage?.setItem(ZERO_BASED_KEY, JSON.stringify(on));
  } catch {
    // storage unavailable: keep the in-memory choice
  }
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

export interface ChartOptionsInput {
  theme: ResolvedTheme;
  visibility: SeriesVisibility;
  width: number;
  height: number;
  /** Every value axis includes zero (AC-AH2AEM.9); off means auto-scale. */
  zeroBased?: boolean;
}

/** uPlot options: x = ms since the session start (labelled in seconds), then one series per CHART_SERIES entry on its axis scale. */
export function chartOptions({
  theme,
  visibility,
  width,
  height,
  zeroBased = false,
}: ChartOptionsInput): uPlot.Options {
  const p = THEME_COLORS[theme];
  const styles = seriesStyles(theme);
  const shown = visibleAxes(visibility);
  const series: uPlot.Series[] = [
    {},
    ...CHART_SERIES.map((s, i) => ({
      label: s.label,
      scale: s.axis,
      show: visibility[s.key],
      stroke: styles[i]?.stroke,
      dash: styles[i]?.dash,
      spanGaps: false,
    })),
  ];
  const axes: uPlot.Axis[] = [
    {
      stroke: p.muted,
      values: (_u, splits, _axis, _space, incr) =>
        splits.map((v) => formatSessionSeconds(v, tickDecimals(incr))),
    },
    ...CHART_AXES.map((a) => ({
      scale: a.id,
      side: a.side === "left" ? 3 : 1,
      show: shown.includes(a.id),
      label: a.title,
      values: (_u: uPlot, splits: number[], _axis: number, _space: number, incr: number) =>
        axisTicks(splits, incr),
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
          zeroBased ? { range: (_u, min, max) => zeroBasedRange(min, max) } : {},
        ]),
      ),
    },
    // The component draws its own legend (toggle buttons) and hover readout.
    legend: { show: false },
    // Plain drag selects an x range; the component zooms to it (leaving live mode).
    cursor: { drag: { x: true, y: false, setScale: false } },
  };
}
