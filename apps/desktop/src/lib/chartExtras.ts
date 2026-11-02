import type uPlot from "uplot";
import type { ThemeStorage } from "./theme";

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

/** Draws `marks` on `u` as dashed vertical lines, each labelled with its percentage at
 * the top of the plot (AC-AXHK2T.4). */
export function paintSegmentMarks(u: uPlot, marks: readonly SegmentMark[], color: string): void {
  const min = u.scales.x?.min;
  const max = u.scales.x?.max;
  if (marks.length === 0 || min == null || max == null) return;
  const ratio = globalThis.devicePixelRatio || 1;
  const { top, height } = u.bbox;
  const ctx = u.ctx;
  ctx.save();
  ctx.strokeStyle = color;
  ctx.fillStyle = color;
  ctx.globalAlpha = 0.8;
  ctx.lineWidth = Math.max(1, ratio);
  ctx.setLineDash([4 * ratio, 3 * ratio]);
  ctx.font = `${Math.round(10 * ratio)}px system-ui, sans-serif`;
  ctx.textAlign = "left";
  ctx.textBaseline = "top";
  for (const m of marks) {
    if (m.tMs < min || m.tMs > max) continue;
    const x = Math.round(u.valToPos(m.tMs, "x", true));
    ctx.beginPath();
    ctx.moveTo(x, top);
    ctx.lineTo(x, top + height);
    ctx.stroke();
    ctx.fillText(m.label, x + 2 * ratio, top + 2 * ratio);
  }
  ctx.restore();
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

/** Draws the E.D. points on `u` as a dashed line, 0% at the bottom of the plot and 100%
 * at the top (AC-AXHK2T.5). */
export function paintEnergyShare(u: uPlot, points: EnergySharePoints, color: string): void {
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
  ctx.restore();
}
