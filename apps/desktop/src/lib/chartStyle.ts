/**
 * The chart's appearance settings (FR AXHK2V): axis range presets, fed to the uPlot
 * options by `chartOptions`.
 */

import {
  CHART_SERIES,
  LINE_WIDTH,
  seriesStyles,
  type ChartAxisId,
  type ChartSeriesKey,
} from "./chartSeries";
import { THEME_COLORS, contrastRatio, type ResolvedTheme, type ThemeStorage } from "./theme";

/** One axis range preset: `auto` scales to the data, `auto0` scales to the data from
 * zero, `fixed` shows `[0, max]` whatever the data. */
export interface RangePreset {
  id: string;
  label: string;
  kind: "auto" | "auto0" | "fixed";
  /** The top of a fixed range. */
  max?: number;
}

function fixed(max: number, unit: "V" | "A"): RangePreset {
  return { id: `0-${max}`, label: `0–${max} ${unit}`, kind: "fixed", max };
}

const AUTO: RangePreset = { id: "auto", label: "Auto", kind: "auto" };
const AUTO_ZERO: RangePreset = { id: "auto0", label: "Auto from zero", kind: "auto0" };

/** The VBUS (volts axis) presets, in the order the UI offers them (AC-AXHK2V.1). */
export const VBUS_PRESETS: readonly RangePreset[] = [
  AUTO,
  AUTO_ZERO,
  fixed(6, "V"),
  fixed(12, "V"),
  fixed(24, "V"),
];

/** The IBUS (amps axis) presets, in the order the UI offers them (AC-AXHK2V.1). */
export const IBUS_PRESETS: readonly RangePreset[] = [
  AUTO,
  AUTO_ZERO,
  fixed(2, "A"),
  fixed(5, "A"),
  fixed(8, "A"),
];

export interface ChartStyle {
  /** A `VBUS_PRESETS` id. */
  vbusRange: string;
  /** An `IBUS_PRESETS` id. */
  ibusRange: string;
  /** The chart title; empty for none. */
  title: string;
  /** Each series' look where it was set; a series not here keeps its theme look. */
  looks: Partial<Record<ChartSeriesKey, Partial<SeriesLook>>>;
}

/** A series' stroke: `#rrggbb`, width in CSS px, opacity 0–1. */
export interface SeriesLook {
  color: string;
  width: number;
  opacity: number;
}

export const DEFAULT_CHART_STYLE: ChartStyle = {
  vbusRange: AUTO.id,
  ibusRange: AUTO.id,
  title: "",
  looks: {},
};

/** `style` with `patch` set on series `key`'s look. */
export function setSeriesLook(
  style: ChartStyle,
  key: ChartSeriesKey,
  patch: Partial<SeriesLook>,
): ChartStyle {
  return { ...style, looks: { ...style.looks, [key]: { ...style.looks[key], ...patch } } };
}

/** The preset `id` names in `presets`, Auto for an unknown id. */
export function rangePreset(presets: readonly RangePreset[], id: string): RangePreset {
  return presets.find((p) => p.id === id) ?? AUTO;
}

/** Each value axis' range preset under `style`; the watts axis has none (AC-AXHK2V.1).
 * Shared by the live chart and the image export, so both scale alike. */
export function axisPresets(style: ChartStyle): Partial<Record<ChartAxisId, RangePreset>> {
  return {
    volts: rangePreset(VBUS_PRESETS, style.vbusRange),
    amps: rangePreset(IBUS_PRESETS, style.ibusRange),
  };
}

/** A `#rrggbb` colour, the only form a series look takes. */
const HEX_COLOR = /^#[0-9a-f]{6}$/i;

/** `style` with series `key` back on its theme look (AC-AXHK2V.2). */
export function resetSeriesLook(style: ChartStyle, key: ChartSeriesKey): ChartStyle {
  const looks = { ...style.looks };
  delete looks[key];
  return { ...style, looks };
}

/** A series' look as drawn: its set look over the theme's, plus its own dash pattern,
 * which no look changes, so the lines stay apart without colour (AC-AXHK2V.2). */
export interface ResolvedSeriesLook extends SeriesLook {
  key: ChartSeriesKey;
  dash: number[];
}

/** Every series' look under `theme`, in `CHART_SERIES` order (AC-AXHK2V.2). */
export function seriesLooks(theme: ResolvedTheme, style: ChartStyle): ResolvedSeriesLook[] {
  const themed = seriesStyles(theme);
  return CHART_SERIES.map((s, i) => {
    const set = style.looks[s.key] ?? {};
    return {
      key: s.key,
      color: set.color ?? themed[i]?.stroke ?? "#000000",
      width: set.width ?? LINE_WIDTH,
      opacity: set.opacity ?? 1,
      dash: themed[i]?.dash ?? [],
    };
  });
}

/** `#rrggbb` at `opacity` as a canvas colour: the colour itself when opaque. */
export function strokeColor(color: string, opacity: number): string {
  const hex = /^#([0-9a-f]{6})$/i.exec(color);
  if (opacity >= 1 || !hex) return color;
  const n = Number.parseInt(hex[1] ?? "000000", 16);
  const a = Math.max(0, opacity);
  return `rgba(${(n >> 16) & 0xff}, ${(n >> 8) & 0xff}, ${n & 0xff}, ${a})`;
}

/** The contrast a chart line needs against its background (WCAG 1.4.11, non-text). */
const MIN_LINE_CONTRAST = 3;

/** A sentence naming the contrast when `color` is hard to see on `theme`'s chart
 * background, `null` otherwise; the colour stays allowed (AC-AXHK2V.3). */
export function lowContrastWarning(color: string, theme: ResolvedTheme): string | null {
  if (!HEX_COLOR.test(color)) return null;
  const ratio = contrastRatio(color, THEME_COLORS[theme].bg);
  if (ratio >= MIN_LINE_CONTRAST) return null;
  return `Low contrast: ${ratio.toFixed(1)}:1 on the chart background (3:1 or more reads well).`;
}

/** The one app-wide key the chart style is kept under, not one per meter. */
export const CHART_STYLE_KEY = "open-powerz.chart.style";

/** The saved chart style, field by field; anything missing or unreadable is the
 * default. */
export function loadChartStyle(storage: ThemeStorage | undefined): ChartStyle {
  try {
    const raw = storage?.getItem(CHART_STYLE_KEY);
    if (raw == null) return DEFAULT_CHART_STYLE;
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
      return DEFAULT_CHART_STYLE;
    }
    const stored = parsed as Record<string, unknown>;
    const style: ChartStyle = { ...DEFAULT_CHART_STYLE, looks: {} };
    const known = (presets: readonly RangePreset[], id: unknown): id is string =>
      typeof id === "string" && presets.some((p) => p.id === id);
    if (known(VBUS_PRESETS, stored.vbusRange)) style.vbusRange = stored.vbusRange;
    if (known(IBUS_PRESETS, stored.ibusRange)) style.ibusRange = stored.ibusRange;
    if (typeof stored.title === "string") style.title = stored.title;
    const looks = stored.looks;
    if (typeof looks === "object" && looks !== null) {
      for (const s of CHART_SERIES) {
        const l = (looks as Record<string, unknown>)[s.key];
        if (typeof l !== "object" || l === null) continue;
        const { color, width, opacity } = l as Record<string, unknown>;
        const look: Partial<SeriesLook> = {};
        if (typeof color === "string" && HEX_COLOR.test(color)) look.color = color;
        if (typeof width === "number" && Number.isFinite(width) && width > 0) look.width = width;
        if (typeof opacity === "number" && opacity >= 0 && opacity <= 1) look.opacity = opacity;
        if (Object.keys(look).length > 0) style.looks[s.key] = look;
      }
    }
    return style;
  } catch {
    return DEFAULT_CHART_STYLE;
  }
}

/** Keeps `style`; a storage failure is ignored (it just won't survive a restart). */
export function saveChartStyle(style: ChartStyle, storage: ThemeStorage | undefined): void {
  try {
    storage?.setItem(CHART_STYLE_KEY, JSON.stringify(style));
  } catch {
    // Not kept.
  }
}
