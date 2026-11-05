/**
 * What "Save image" asks for (FR AXHK2V): the formats, the size limits and the default
 * file name, plus the title band and ink both image renderers share.
 */

import {
  CHART_AXES,
  CHART_SERIES,
  presetRange,
  type ChartAxisId,
  type SeriesVisibility,
} from "./chartSeries";
import { axisPresets, seriesLooks, type ChartStyle, type ResolvedSeriesLook } from "./chartStyle";
import { THEME_COLORS, type ResolvedTheme } from "./theme";

/** The image formats, in the order the dialog offers them. */
export const EXPORT_FORMATS = ["png", "svg", "pdf"] as const;
export type ExportFormat = (typeof EXPORT_FORMATS)[number];

/** What the "Save image" dialog asked for. */
export interface SaveImageChoice {
  width: number;
  height: number;
  format: ExportFormat;
  /** Draws on white instead of the theme's background. */
  white: boolean;
}

export const EXPORT_MIN_PX = 320;
export const EXPORT_MAX_PX = 7680;

function sizeOk(px: number): boolean {
  return Number.isInteger(px) && px >= EXPORT_MIN_PX && px <= EXPORT_MAX_PX;
}

/** A sentence naming both limits for a size outside them, `null` for a good size. */
export function exportSizeError(width: number, height: number): string | null {
  if (sizeOk(width) && sizeOk(height)) return null;
  return `Width and height must be whole numbers from ${EXPORT_MIN_PX} to ${EXPORT_MAX_PX} px.`;
}

const two = (n: number): string => String(n).padStart(2, "0");

/** The default file name: `powerz-chart-YYYYMMDD-HHMMSS.<format>` in local time, with no
 * serial or UID in it. */
export function chartImageName(format: ExportFormat, now: Date): string {
  const date = `${now.getFullYear()}${two(now.getMonth() + 1)}${two(now.getDate())}`;
  const time = `${two(now.getHours())}${two(now.getMinutes())}${two(now.getSeconds())}`;
  return `powerz-chart-${date}-${time}.${format}`;
}

/** The title band above the plot in a saved image with a title, px (AC-AXHK2V.6). */
export const EXPORT_TITLE_PX = 28;

/** A saved image's background and text colours: the theme's, or black on white
 * (AC-AXHK2V.6). */
export function exportInk(theme: ResolvedTheme, white: boolean): { bg: string; fg: string } {
  const p = THEME_COLORS[theme];
  return white ? { bg: "#ffffff", fg: "#000000" } : { bg: p.bg, fg: p.text };
}

/** The legend row's height at the foot of a saved image, px (AC-AXHK2V.6). */
export const EXPORT_LEGEND_PX = 24;

/** One legend entry of a saved image: a swatch line in the series' look and its label. */
export interface ExportLegendItem {
  look: ResolvedSeriesLook;
  /** `<label> (<unit>)`, e.g. `VBUS (V)`. */
  label: string;
  /** The swatch line, from `x0` to `x1` at `y`. */
  x0: number;
  x1: number;
  y: number;
  /** Where the label starts; it is centred on `y`. */
  textX: number;
}

/** What a saved image draws besides the plot (AC-AXHK2V.6): the title band (if any)
 * at the top, the legend row at the foot. */
export interface ExportLayout {
  /** The title and where its baseline is centred; `null` for an empty title. */
  title: { text: string; x: number; y: number } | null;
  /** Px above the plot taken by the title band. */
  top: number;
  /** Px below the plot taken by the legend row. */
  bottom: number;
  legend: ExportLegendItem[];
}

/** The title band and legend row of a `width` × `height` image, the same for the PNG
 * (and so the PDF) and the SVG (AC-AXHK2V.6). */
export function exportLayout(input: {
  width: number;
  height: number;
  title: string;
  theme: ResolvedTheme;
  style: ChartStyle;
  visibility: SeriesVisibility;
}): ExportLayout {
  const { width, height, title, theme, style, visibility } = input;
  const looks = seriesLooks(theme, style);
  const shown = CHART_SERIES.flatMap((def, i) => {
    const look = looks[i];
    return visibility[def.key] && look !== undefined ? [{ def, look }] : [];
  });
  const y = height - EXPORT_LEGEND_PX / 2;
  const slot = Math.min(110, (width - 16) / Math.max(1, shown.length));
  const start = Math.max(8, (width - slot * shown.length) / 2);
  return {
    title: title === "" ? null : { text: title, x: width / 2, y: EXPORT_TITLE_PX - 8 },
    top: title === "" ? 0 : EXPORT_TITLE_PX,
    bottom: EXPORT_LEGEND_PX,
    legend: shown.map(({ def, look }, k) => {
      const x0 = start + k * slot;
      return { look, label: `${def.label} (${def.unit})`, x0, x1: x0 + 18, y, textX: x0 + 24 };
    }),
  };
}

/** Every text a layout draws: the title and the legend labels. */
export function exportTexts(layout: ExportLayout): string[] {
  return [...(layout.title ? [layout.title.text] : []), ...layout.legend.map((l) => l.label)];
}

/** Each value axis' range in a saved image (AC-AXHK2V.6): its preset over the view's
 * data of its shown series, with zero-based axes as on the live chart. The SVG and the
 * PNG both use it, so the same view saves on the same ranges. */
export function exportAxisRanges(input: {
  style: ChartStyle;
  visibility: SeriesVisibility;
  view: { t0Ms: number; t1Ms: number };
  data: readonly (ArrayLike<number | null | undefined> | undefined)[];
  zeroBased?: boolean;
}): Record<ChartAxisId, [number, number]> {
  const { style, visibility, view, data, zeroBased = false } = input;
  const presets = axisPresets(style);
  const xs = data[0] ?? [];
  const ranges = {} as Record<ChartAxisId, [number, number]>;
  for (const a of CHART_AXES) {
    let lo: number | null = null;
    let hi: number | null = null;
    CHART_SERIES.forEach((def, i) => {
      if (def.axis !== a.id || !visibility[def.key]) return;
      const ys = data[i + 1] ?? [];
      for (let k = 0; k < xs.length; k++) {
        const t = xs[k];
        const y = ys[k];
        if (t == null || y == null || t < view.t0Ms || t > view.t1Ms || !Number.isFinite(y)) {
          continue;
        }
        lo = lo === null ? y : Math.min(lo, y);
        hi = hi === null ? y : Math.max(hi, y);
      }
    });
    ranges[a.id] = presetRange(a.id, presets[a.id], lo, hi, zeroBased);
  }
  return ranges;
}
