import type uPlot from "uplot";
import type { ResolvedTheme, ThemeStorage } from "./theme";

/**
 * The chart's extra options (FR AXHK2T): the ruler, segment marks, the E.D. series, the
 * sub-chart and Reverse current, all off by default so the chart draws as before.
 */

/** The one sentence that defines E.D., shown by the UI and quoted by the docs. */
export const ED_DEFINITION =
  "E.D. is our own definition, not the vendor's number: the energy measured from the " +
  "start of the session up to each moment, as a percentage of the whole session's energy.";

/** What segment marks follow. */
export type MarkKind = "off" | "charge" | "energy";

/** The chart options. */
export interface ChartExtras {
  ruler: boolean;
  marks: MarkKind;
  markStep: number;
  ed: boolean;
  subChart: boolean;
  reverseCurrent: boolean;
}

/** Every option off: the chart as it was. */
export const DEFAULT_CHART_EXTRAS: ChartExtras = {
  ruler: false,
  marks: "off",
  markStep: 10,
  ed: false,
  subChart: false,
  reverseCurrent: false,
};

/** The segment-mark steps offered, in percent; each divides 100. */
export const MARK_STEPS = [5, 10, 20, 25] as const;

/** localStorage key of the chart options (AC-AXHK2T.8). */
export const CHART_EXTRAS_KEY = "open-powerz.chart.extras";

const MARK_KINDS: readonly MarkKind[] = ["off", "charge", "energy"];

/** The fields of a saved value that have the right type; the rest are dropped. */
function savedExtras(saved: Record<string, unknown>): Partial<ChartExtras> {
  const { ruler, marks, markStep, ed, subChart, reverseCurrent } = saved;
  return {
    ...(typeof ruler === "boolean" ? { ruler } : {}),
    ...(MARK_KINDS.includes(marks as MarkKind) ? { marks: marks as MarkKind } : {}),
    ...((MARK_STEPS as readonly unknown[]).includes(markStep)
      ? { markStep: markStep as number }
      : {}),
    ...(typeof ed === "boolean" ? { ed } : {}),
    ...(typeof subChart === "boolean" ? { subChart } : {}),
    ...(typeof reverseCurrent === "boolean" ? { reverseCurrent } : {}),
  };
}

/** The saved chart options; the defaults for nothing saved or anything unreadable, and a
 * bad field falls back to its default alone (AC-AXHK2T.8). */
export function loadChartExtras(storage: ThemeStorage | undefined): ChartExtras {
  try {
    const saved: unknown = JSON.parse(storage?.getItem(CHART_EXTRAS_KEY) ?? "null");
    if (typeof saved !== "object" || saved === null || Array.isArray(saved)) {
      return { ...DEFAULT_CHART_EXTRAS };
    }
    return { ...DEFAULT_CHART_EXTRAS, ...savedExtras(saved as Record<string, unknown>) };
  } catch {
    return { ...DEFAULT_CHART_EXTRAS };
  }
}

/** Persist the chart options; storage failures are ignored (AC-AXHK2T.8). */
export function saveChartExtras(extras: ChartExtras, storage: ThemeStorage | undefined): void {
  try {
    storage?.setItem(CHART_EXTRAS_KEY, JSON.stringify(extras));
  } catch {
    // storage unavailable (private mode, quota, denied): keep the in-memory value
  }
}

/** One segment mark: where the run first reached `percent` of its total. */
export interface SegmentMark {
  tMs: number;
  percent: number;
  label: string;
}

/** Why nothing is drawn: a note for each option that is on but has no positive total in
 * the backend's last answer (`null`: not known yet). */
export function extrasNotes(
  options: { marks: MarkKind; ed: boolean },
  totals: { marks: boolean | null; energy: boolean | null },
): string[] {
  const notes: string[] = [];
  if (options.marks !== "off" && totals.marks === false) {
    notes.push(`No ${options.marks} in this run yet, so no marks.`);
  }
  if (options.ed && totals.energy === false) {
    notes.push("No energy in this session yet, so no E.D. line.");
  }
  return notes;
}

/** The marks inside the x range `[min, max]`. */
export function marksInRange(
  marks: readonly SegmentMark[],
  min: number | undefined,
  max: number | undefined,
): SegmentMark[] {
  if (min === undefined || max === undefined) return [];
  return marks.filter((m) => m.tMs >= min && m.tMs <= max);
}

/** A text label's box, px, every box in one frame (AC-AXHK2V.12). */
export interface LabelBox {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

function boxesOverlap(a: LabelBox, b: LabelBox): boolean {
  return a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom;
}

/** Which of `labels` to draw (one boolean each, in order) so that no drawn label overlaps
 * another or any `blocked` box (the axis tick labels): all of them when they fit, else
 * every other one (the first, third, …), else none. A label over a blocked box is always
 * left out (AC-AXHK2V.12). */
export function markLabelsShown(
  labels: readonly LabelBox[],
  blocked: readonly LabelBox[],
): boolean[] {
  const free = labels.map((l) => !blocked.some((b) => boxesOverlap(l, b)));
  const fits = (pick: (k: number) => boolean): boolean[] | null => {
    const shown = labels.map((_, k) => free[k]! && pick(k));
    const drawn = labels.filter((_, k) => shown[k]);
    for (let a = 0; a < drawn.length; a++) {
      for (let b = a + 1; b < drawn.length; b++) {
        if (boxesOverlap(drawn[a]!, drawn[b]!)) return null;
      }
    }
    return shown;
  };
  return fits(() => true) ?? fits((k) => k % 2 === 0) ?? labels.map(() => false);
}

/** uPlot's resolved axis fields (uplot 1.6, pinned in package.json). */
type ResolvedAxis = uPlot.Axis & {
  _show?: boolean;
  _pos?: number;
  _size?: number;
  _splits?: number[];
  _values?: (string | number | null)[];
  font?: [string, number, number];
  alignTo?: number;
};

/** The box of every axis tick label `u` draws, canvas px (AC-AXHK2V.12). */
export function tickLabelBoxes(u: uPlot): LabelBox[] {
  const ratio = globalThis.devicePixelRatio || 1;
  const ctx = u.ctx;
  const boxes: LabelBox[] = [];
  const before = ctx.font;
  for (const raw of u.axes ?? []) {
    const axis = raw as ResolvedAxis;
    if (axis.show === false || axis._show === false) continue;
    const splits = axis._splits;
    const values = axis._values;
    const scale = axis.scale;
    if (!splits || !values || axis._pos === undefined || scale === undefined) continue;
    const side = axis.side ?? 2;
    const vertical = side % 2 === 1;
    const shiftDir = side === 0 || side === 3 ? -1 : 1;
    const tickSize = axis.ticks?.show === false ? 0 : (axis.ticks?.size ?? 10);
    const gap = axis.gap ?? 5;
    const axisGap = axis.alignTo === 2 ? (axis._size ?? 0) - tickSize - gap : gap;
    const pos = (axis._pos + shiftDir * (tickSize + axisGap)) * ratio;
    const font = axis.font;
    if (font) ctx.font = font[0];
    const h = font?.[1] ?? 10 * ratio;
    for (let k = 0; k < values.length; k++) {
      const v = values[k];
      const at = splits[k];
      if (v === null || v === undefined || at === undefined) continue;
      const w = ctx.measureText(String(v)).width;
      const c = u.valToPos(at, scale, true);
      if (vertical) {
        const left = side === 3 ? pos - w : pos;
        boxes.push({ left, top: c - h / 2, right: left + w, bottom: c + h / 2 });
      } else {
        const top = side === 0 ? pos - h : pos;
        boxes.push({ left: c - w / 2, top, right: c + w / 2, bottom: top + h });
      }
    }
  }
  ctx.font = before;
  return boxes;
}

/** What {@link paintSegmentMarks} drew: the labels shown and the tick labels they kept
 * clear of, canvas px. */
export interface PaintedMarkLabels {
  markLabels: (LabelBox & { label: string })[];
  tickLabels: LabelBox[];
}

/** Draws `marks` on `u` as dashed vertical lines, each labelled with its percentage at
 * the top of the plot (AC-AXHK2T.4). Labels that would cover an axis tick label or each
 * other thin out or are left out; the lines always stay (AC-AXHK2V.12). A label over an
 * `avoid` box (the "E.D. %" label's) is left out too (AC-AXHK2V.15). */
export function paintSegmentMarks(
  u: uPlot,
  marks: readonly SegmentMark[],
  color: string,
  avoid: readonly LabelBox[] = [],
): PaintedMarkLabels {
  const painted: PaintedMarkLabels = { markLabels: [], tickLabels: [] };
  const min = u.scales.x?.min;
  const max = u.scales.x?.max;
  if (marks.length === 0 || min == null || max == null) return painted;
  const ratio = globalThis.devicePixelRatio || 1;
  const { top, height } = u.bbox;
  const ctx = u.ctx;
  const ticks = tickLabelBoxes(u);
  painted.tickLabels = ticks;
  ctx.save();
  ctx.strokeStyle = color;
  ctx.fillStyle = color;
  ctx.globalAlpha = 0.8;
  ctx.lineWidth = Math.max(1, ratio);
  ctx.setLineDash([4 * ratio, 3 * ratio]);
  const fontPx = Math.round(10 * ratio);
  ctx.font = `${fontPx}px system-ui, sans-serif`;
  ctx.textAlign = "left";
  ctx.textBaseline = "top";
  const shown: (LabelBox & { label: string })[] = [];
  for (const m of marks) {
    if (m.tMs < min || m.tMs > max) continue;
    const x = Math.round(u.valToPos(m.tMs, "x", true));
    ctx.beginPath();
    ctx.moveTo(x, top);
    ctx.lineTo(x, top + height);
    ctx.stroke();
    const left = x + 2 * ratio;
    const y = top + 2 * ratio;
    shown.push({
      label: m.label,
      left,
      top: y,
      right: left + ctx.measureText(m.label).width,
      bottom: y + fontPx,
    });
  }
  const keep = markLabelsShown(shown, [...ticks, ...avoid]);
  for (let k = 0; k < shown.length; k++) {
    if (!keep[k]) continue;
    const l = shown[k]!;
    ctx.fillText(l.label, l.left, l.top);
    painted.markLabels.push(l);
  }
  ctx.restore();
  return painted;
}

/** The E.D. points to draw: the share of the session's energy at each time. */
export interface EnergySharePoints {
  tMs: number[];
  pct: (number | null)[];
}

/** How many E.D. points have a value. */
export function energySharePointCount(points: EnergySharePoints): number {
  let n = 0;
  for (const p of points.pct) if (p !== null) n++;
  return n;
}

/** The E.D. line's own colour per theme: no series uses it, nor the accent, so the line
 * is never read against the volts axis (AC-AXHK2V.13). */
const ED_COLORS: Record<ResolvedTheme, string> = { light: "#4d7c0f", dark: "#bef264" };

/** The E.D. line's colour in `theme`. */
export function edColor(theme: ResolvedTheme): string {
  return ED_COLORS[theme];
}

/** The label written at the E.D. line's right end. */
const ED_LABEL = "E.D. %";

/** The last E.D. point with a value, canvas px, or null without one. */
function energyShareEnd(u: uPlot, points: EnergySharePoints): { x: number; y: number } | null {
  const { top, height } = u.bbox;
  for (let k = Math.min(points.tMs.length, points.pct.length) - 1; k >= 0; k--) {
    const pct = points.pct[k];
    if (pct === null || pct === undefined) continue;
    const x = u.valToPos(points.tMs[k]!, "x", true);
    const y = top + height - (Math.min(100, Math.max(0, pct)) / 100) * height;
    return { x, y };
  }
  return null;
}

/** The box of the "E.D. %" label {@link paintEnergyShare} writes, canvas px, or null
 * without a line: by the line's right end, inside the plot and clear of every axis tick
 * label (AC-AXHK2V.15). */
export function edLabelBox(u: uPlot, points: EnergySharePoints): LabelBox | null {
  const last = energyShareEnd(u, points);
  if (last === null) return null;
  const ratio = globalThis.devicePixelRatio || 1;
  const { left, top, width, height } = u.bbox;
  const ctx = u.ctx;
  const fontPx = Math.round(10 * ratio);
  const before = ctx.font;
  ctx.font = `${fontPx}px system-ui, sans-serif`;
  const w = ctx.measureText(ED_LABEL).width;
  ctx.font = before;
  const pad = 4 * ratio;
  const bottom0 = Math.max(top + 12 * ratio, last.y - pad);
  const ticks = tickLabelBoxes(u);
  const right = last.x + pad;
  const leftOf = last.x - pad - w;
  // Right of the end point when it fits, else left of it; the other side next.
  const fitsRight = right + w <= left + width;
  const sides = (fitsRight ? [right, leftOf] : [leftOf, right]).filter(
    (x) => x >= left && x + w <= left + width,
  );
  const at = (x: number, bottom: number): LabelBox => ({
    left: x,
    top: bottom - fontPx,
    right: x + w,
    bottom,
  });
  for (const x of sides) {
    let box = at(x, bottom0);
    // Step below any tick label it would cover, while it stays in the plot.
    for (let n = 0; n <= ticks.length && box.bottom <= top + height; n++) {
      const hit = ticks.filter((t) => boxesOverlap(box, t));
      if (hit.length === 0) return box;
      box = at(x, Math.max(...hit.map((t) => t.bottom)) + fontPx);
    }
  }
  return at(sides[0] ?? Math.max(left, Math.min(right, left + width - w)), bottom0);
}

/** Draws the E.D. points on `u` as a dashed line, 0% at the bottom of the plot and 100%
 * at the top (AC-AXHK2T.5), with a small "E.D. %" label at its right end (AC-AXHK2V.13),
 * in {@link edLabelBox} (AC-AXHK2V.15). A caller that already has this draw's box passes
 * it as `box` so it is computed once. */
export function paintEnergyShare(
  u: uPlot,
  points: EnergySharePoints,
  color: string,
  box: LabelBox | null = edLabelBox(u, points),
): void {
  if (points.tMs.length === 0) return;
  const ratio = globalThis.devicePixelRatio || 1;
  const { left, top, width, height } = u.bbox;
  const ctx = u.ctx;
  ctx.save();
  ctx.beginPath();
  ctx.rect(left, top, width, height);
  ctx.clip();
  ctx.strokeStyle = color;
  ctx.lineWidth = Math.max(1, 1.5 * ratio);
  ctx.setLineDash([6 * ratio, 3 * ratio]);
  ctx.beginPath();
  let pen = false;
  for (let k = 0; k < points.tMs.length; k++) {
    const pct = points.pct[k];
    if (pct === null || pct === undefined) {
      pen = false;
      continue;
    }
    const x = u.valToPos(points.tMs[k]!, "x", true);
    const y = top + height - (Math.min(100, Math.max(0, pct)) / 100) * height;
    if (pen) ctx.lineTo(x, y);
    else ctx.moveTo(x, y);
    pen = true;
  }
  ctx.stroke();
  if (box) {
    ctx.setLineDash([]);
    ctx.font = `${Math.round(10 * ratio)}px system-ui, sans-serif`;
    ctx.fillStyle = color;
    ctx.textBaseline = "bottom";
    ctx.textAlign = "left";
    ctx.fillText(ED_LABEL, box.left, box.bottom);
  }
  ctx.restore();
}
