/**
 * The chart as an SVG document (FR AXHK2V), rendered from the chart's frame by a pure
 * function: no DOM, no canvas.
 */

import {
  CHART_AXES,
  CHART_SERIES,
  TIME_INCRS,
  axisTicks,
  timeSplits,
  timeTicks,
  type ChartAxisId,
  type SeriesVisibility,
} from "./chartSeries";
import { exportAxisRanges, exportInk, exportLayout } from "./chartExport";
import { seriesLooks, type ChartStyle } from "./chartStyle";
import { THEME_COLORS, type ResolvedTheme } from "./theme";

export interface ChartSvgInput {
  /** The image size in px. */
  width: number;
  height: number;
  /** The chart title; empty for none. */
  title: string;
  theme: ResolvedTheme;
  /** Draws on white instead of the theme's background. */
  white: boolean;
  style: ChartStyle;
  /** The series shown. */
  visibility: SeriesVisibility;
  /** The x range shown, in x units. */
  view: { t0Ms: number; t1Ms: number };
  /** The session start in x units. */
  originMs: number;
  /** The frame in uPlot's layout: `[x, …one array per CHART_SERIES entry]`, `null` a gap. */
  data: readonly (readonly (number | null)[])[];
  /** Every value axis includes zero, as on the live chart (AC-AH2AEM.9). */
  zeroBased?: boolean;
  /** The amps axis drawn upside down, as on the live chart (AC-AXHK2T.7). */
  reverseCurrent?: boolean;
}

/** `text` with the XML specials escaped, so it reads as text, never as markup
 * (AC-AXHK2V.4). */
export function escapeXml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&apos;");
}

/** Room beside the plot for a value axis (tick labels plus its rotated title). */
const SIDE_PX = 64;
/** Room below the plot for the time tick labels. */
const TIME_PX = 22;
/** Least px between two time ticks. */
const TIME_TICK_GAP_PX = 90;
/** Value ticks per axis, roughly. */
const VALUE_TICKS = 5;
const FONT = 'font-family="sans-serif"';

/** `n` written short, for coordinates. */
function num(n: number): string {
  return String(Math.round(n * 100) / 100);
}

/** A 1-2-5 step near `raw`. */
function niceStep(raw: number): number {
  if (!(raw > 0) || !Number.isFinite(raw)) return 1;
  const p = 10 ** Math.floor(Math.log10(raw));
  const f = raw / p;
  return (f <= 1 ? 1 : f <= 2 ? 2 : f <= 5 ? 5 : 10) * p;
}

/** The `stroke-dasharray` attribute for `dash`, empty for a solid line. */
function dashAttr(dash: readonly number[]): string {
  return dash.length > 0 ? ` stroke-dasharray="${dash.join(" ")}"` : "";
}

/** The whole SVG document for `input`: the current view with its title, axes and
 * legend, on the theme's background or on white (AC-AXHK2V.6). */
export function chartSvg(input: ChartSvgInput): string {
  const { width, height, title, theme, white, style, visibility, view, originMs, data } = input;
  const p = THEME_COLORS[theme];
  const { bg, fg } = exportInk(theme, white);
  const grid = white ? "#d0d0d0" : p.border;
  const layout = exportLayout({ width, height, title, theme, style, visibility });
  const top = layout.top + 8;
  const plot = {
    x0: SIDE_PX,
    x1: Math.max(SIDE_PX + 1, width - SIDE_PX),
    y0: top,
    y1: Math.max(top + 1, height - TIME_PX - layout.bottom),
  };
  const t0 = view.t0Ms;
  const t1 = view.t1Ms > t0 ? view.t1Ms : t0 + 1;
  const xAt = (t: number): number => plot.x0 + ((t - t0) / (t1 - t0)) * (plot.x1 - plot.x0);

  const xs = data[0] ?? [];
  const looks = seriesLooks(theme, style);
  const shown = CHART_SERIES.map((s, i) => ({ def: s, i })).filter(
    ({ def }) => visibility[def.key],
  );

  // Each axis' range over the view's data of its shown series, as the PNG has it.
  const ranges = exportAxisRanges({
    style,
    visibility,
    view: { t0Ms: t0, t1Ms: t1 },
    data,
    zeroBased: input.zeroBased,
  });
  const yAt = (axis: ChartAxisId, v: number): number => {
    const [lo, hi] = ranges[axis];
    const span = hi > lo ? hi - lo : 1;
    const f = (v - lo) / span;
    // Reverse current draws the amps axis upside down, like the live chart.
    const flip = input.reverseCurrent === true && axis === "amps";
    const y = flip ? plot.y0 + f * (plot.y1 - plot.y0) : plot.y1 - f * (plot.y1 - plot.y0);
    return Math.min(plot.y1, Math.max(plot.y0, y));
  };

  const parts: string[] = [
    '<?xml version="1.0" encoding="UTF-8"?>',
    `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}">`,
    `<rect x="0" y="0" width="${width}" height="${height}" fill="${bg}"/>`,
  ];
  if (layout.title !== null) {
    parts.push(
      `<text x="${num(layout.title.x)}" y="${num(layout.title.y)}" text-anchor="middle" ${FONT} font-size="16" fill="${fg}">${escapeXml(layout.title.text)}</text>`,
    );
  }

  // The time axis: ticks from the session start, inside the view only.
  const maxTicks = Math.max(2, Math.floor((plot.x1 - plot.x0) / TIME_TICK_GAP_PX));
  const incr =
    TIME_INCRS.find((s) => (t1 - t0) / s <= maxTicks) ?? TIME_INCRS[TIME_INCRS.length - 1] ?? 1;
  const tSplits = timeSplits(t0, t1, incr, originMs);
  const tLabels = timeTicks(
    tSplits.map((t) => t - originMs),
    incr,
  );
  tSplits.forEach((t, k) => {
    const x = num(xAt(t));
    parts.push(
      `<line x1="${x}" y1="${num(plot.y0)}" x2="${x}" y2="${num(plot.y1)}" stroke="${grid}" stroke-width="1"/>`,
      `<text x="${x}" y="${num(plot.y1 + 16)}" text-anchor="middle" ${FONT} font-size="11" fill="${fg}">${escapeXml(tLabels[k] ?? "")}</text>`,
    );
  });

  // The value axes: volts on the left, amps (and watts, when power shows) on the right.
  const axes = CHART_AXES.filter((a) => a.id !== "watts" || visibility.power);
  let rightOffset = 0;
  for (const a of axes) {
    const left = a.side === "left";
    const edge = left ? plot.x0 : plot.x1 + rightOffset;
    if (!left) rightOffset += SIDE_PX;
    const [lo, hi] = ranges[a.id];
    const step = niceStep((hi - lo) / VALUE_TICKS);
    const splits: number[] = [];
    for (let v = Math.ceil(lo / step) * step; v <= hi + step * 1e-9; v += step) {
      splits.push(Math.abs(v) < step * 1e-9 ? 0 : v);
      if (splits.length > 50) break;
    }
    const labels = axisTicks(splits, step);
    splits.forEach((v, k) => {
      const y = num(yAt(a.id, v));
      parts.push(
        `<text x="${num(left ? edge - 6 : edge + 6)}" y="${y}" dy="4" text-anchor="${left ? "end" : "start"}" ${FONT} font-size="11" fill="${fg}">${escapeXml(labels[k] ?? "")}</text>`,
      );
    });
    const tx = left ? 14 : Math.min(width - 6, edge + SIDE_PX - 10);
    const ty = (plot.y0 + plot.y1) / 2;
    parts.push(
      `<line x1="${num(edge)}" y1="${num(plot.y0)}" x2="${num(edge)}" y2="${num(plot.y1)}" stroke="${fg}" stroke-width="1"/>`,
      `<text x="${num(tx)}" y="${num(ty)}" transform="rotate(-90 ${num(tx)} ${num(ty)})" text-anchor="middle" ${FONT} font-size="12" fill="${fg}">${escapeXml(a.title)}</text>`,
    );
  }
  parts.push(
    `<line x1="${num(plot.x0)}" y1="${num(plot.y1)}" x2="${num(plot.x1)}" y2="${num(plot.y1)}" stroke="${fg}" stroke-width="1"/>`,
  );

  // Each shown series as one path over the view; a gap starts a new piece.
  for (const { def, i } of shown) {
    const look = looks[i];
    if (look === undefined) continue;
    const ys = data[i + 1] ?? [];
    const d: string[] = [];
    let pen = false;
    for (let k = 0; k < xs.length; k++) {
      const t = xs[k];
      const y = ys[k];
      if (t == null || y == null || !Number.isFinite(y) || t < t0 || t > t1) {
        pen = false;
        continue;
      }
      d.push(`${pen ? "L" : "M"}${num(xAt(t))} ${num(yAt(def.axis, y))}`);
      pen = true;
    }
    if (d.length === 0) continue;
    parts.push(
      `<path d="${d.join(" ")}" fill="none" stroke="${look.color}" stroke-width="${look.width}" stroke-opacity="${look.opacity}"${dashAttr(look.dash)} stroke-linejoin="round"/>`,
    );
  }

  // The legend: a swatch and the label of each shown series.
  for (const { look, label, x0, x1, y, textX } of layout.legend) {
    parts.push(
      `<line x1="${num(x0)}" y1="${num(y)}" x2="${num(x1)}" y2="${num(y)}" stroke="${look.color}" stroke-width="${look.width}" stroke-opacity="${look.opacity}"${dashAttr(look.dash)}/>`,
      `<text x="${num(textX)}" y="${num(y)}" dy="4" ${FONT} font-size="12" fill="${fg}">${escapeXml(label)}</text>`,
    );
  }

  parts.push("</svg>");
  return parts.join("\n");
}
