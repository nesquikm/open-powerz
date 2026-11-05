/**
 * The chart ruler (FR AXHK2T): two cursors, T1 and T2, and what lies between them.
 * Pure: the chart components hold a {@link Ruler} and draw it; the backend's
 * `range_stats` answers charge, energy and the averages from the raw samples.
 */

import type { RangeStats } from "./backend";
import type { TimeRange } from "./chartView";
import { formatSessionTime } from "./chartSeries";
import { formatFixed } from "./format";

/** The two cursors' times, ms; T2 may lie before T1. */
export interface Ruler {
  t1Ms: number;
  t2Ms: number;
}

export type RulerCursorId = "t1" | "t2";

/** What the ruler reads between its cursors; `null` while the backend has not answered. */
export interface RulerValues {
  deltaMs: number;
  chargeAh: number | null;
  energyWh: number | null;
  vbusV: number | null;
  ibusA: number | null;
  powerW: number | null;
}

/** One readout value: its accessible name, its short visible label and its text. */
export interface RulerReadoutItem {
  name: string;
  label: string;
  text: string;
}

function clamp(ms: number, bounds: TimeRange): number {
  return Math.min(Math.max(ms, bounds.t0Ms), bounds.t1Ms);
}

/** T1 a third and T2 two thirds into `view`. */
export function placeRuler(view: TimeRange): Ruler {
  const span = view.t1Ms - view.t0Ms;
  return { t1Ms: view.t0Ms + span / 3, t2Ms: view.t0Ms + (2 * span) / 3 };
}

/** Moves cursor `id` to `ms`, kept inside `bounds`; the other stays. */
export function moveRulerCursor(r: Ruler, id: RulerCursorId, ms: number, bounds: TimeRange): Ruler {
  const at = clamp(ms, bounds);
  return id === "t1" ? { ...r, t1Ms: at } : { ...r, t2Ms: at };
}

/** One pixel of `view` across `widthPx`, or a tenth of the view when `coarse`; never
 * less than a millisecond. */
export function rulerStepMs(view: TimeRange, widthPx: number, coarse: boolean): number {
  const span = view.t1Ms - view.t0Ms;
  const step = coarse ? span / 10 : span / Math.max(1, widthPx);
  return Math.max(1, step);
}

/** Moves cursor `id` by `stepMs` in `direction`, kept inside `bounds`. */
export function stepRulerCursor(
  r: Ruler,
  id: RulerCursorId,
  direction: -1 | 1,
  stepMs: number,
  bounds: TimeRange,
): Ruler {
  const from = id === "t1" ? r.t1Ms : r.t2Ms;
  return moveRulerCursor(r, id, from + direction * stepMs, bounds);
}

/** The cursor a press at `ms` grabs: the nearer one (T1 on a tie). */
export function nearestRulerCursor(r: Ruler, ms: number): RulerCursorId {
  return Math.abs(ms - r.t1Ms) <= Math.abs(ms - r.t2Ms) ? "t1" : "t2";
}

/** The cursors' times in order. */
export function rulerSpan(r: Ruler): TimeRange {
  return { t0Ms: Math.min(r.t1Ms, r.t2Ms), t1Ms: Math.max(r.t1Ms, r.t2Ms) };
}

/** Delta time from the cursors, the rest from the backend's `stats` (null: not yet). */
export function rulerValues(r: Ruler, stats: RangeStats | null): RulerValues {
  return {
    deltaMs: Math.abs(r.t2Ms - r.t1Ms),
    chargeAh: stats?.charge_ah ?? null,
    energyWh: stats?.energy_wh ?? null,
    vbusV: stats?.vbus_avg_v ?? null,
    ibusA: stats?.ibus_avg_a ?? null,
    powerW: stats?.power_avg_w ?? null,
  };
}

/** Shown for a value whose answer is on its way. */
export const RULER_PENDING = "…";

/** The six readout values, in order. While `pending` (the backend has not answered for
 * the cursors as they are now) every value but delta time reads {@link RULER_PENDING}. */
export function rulerReadout(v: RulerValues, pending = false): RulerReadoutItem[] {
  /** `value` at `decimals` places and its unit; a dash and the unit while unknown. */
  const withUnit = (value: number | null, decimals: number, unit: string): string =>
    pending ? RULER_PENDING : `${formatFixed(value, decimals)} ${unit}`;
  return [
    { name: "Delta time", label: "ΔT", text: formatSessionTime(v.deltaMs, 3) },
    { name: "Delta charge", label: "ΔAh", text: withUnit(v.chargeAh, 6, "Ah") },
    { name: "Delta energy", label: "ΔWh", text: withUnit(v.energyWh, 6, "Wh") },
    { name: "Average voltage", label: "Avg V", text: withUnit(v.vbusV, 3, "V") },
    { name: "Average current", label: "Avg A", text: withUnit(v.ibusA, 3, "A") },
    { name: "Average power", label: "Avg W", text: withUnit(v.powerW, 3, "W") },
  ];
}

/** The readout as one sentence for a screen reader. */
export function rulerAnnouncement(items: readonly RulerReadoutItem[]): string {
  return `Ruler: ${items.map((i) => `${i.name} ${i.text}`).join(", ")}`;
}

/** Where `ms` lies against `view`: before it, in it (ends included) or after it. */
export function rulerCursorSide(ms: number, view: TimeRange): "before" | "in" | "after" {
  if (ms < view.t0Ms) return "before";
  if (ms > view.t1Ms) return "after";
  return "in";
}

/** The tag a ruler cursor shows: its name alone while in view; at an edge an arrow
 * towards that edge, the name, " · " and the time. */
export function rulerTag(name: string, side: "before" | "in" | "after", time: string): string {
  if (side === "before") return `◀ ${name} · ${time}`;
  if (side === "after") return `${name} · ${time} ▶`;
  return name;
}

/**
 * Cursor `id` after `key` on its slider, or null for a key it does not take. ← / → step
 * one pixel of `view` (Shift: a tenth of it), PageUp / PageDown a tenth, Home / End go to
 * the ends of `bounds`. A cursor in view stays in view, so the focused cursor never
 * scrolls away; one out of view comes to the view's near edge on a key towards it, and
 * steps within `bounds` on a key away from it.
 */
export function rulerKey(
  r: Ruler,
  id: RulerCursorId,
  key: string,
  shiftKey: boolean,
  view: TimeRange,
  bounds: TimeRange,
  widthPx: number,
): Ruler | null {
  if (key === "Home") return moveRulerCursor(r, id, bounds.t0Ms, bounds);
  if (key === "End") return moveRulerCursor(r, id, bounds.t1Ms, bounds);
  const direction =
    key === "ArrowRight" || key === "PageUp"
      ? 1
      : key === "ArrowLeft" || key === "PageDown"
        ? -1
        : 0;
  if (direction === 0) return null;
  const coarse = shiftKey || key === "PageUp" || key === "PageDown";
  const step = rulerStepMs(view, widthPx, coarse);
  const from = id === "t1" ? r.t1Ms : r.t2Ms;
  const side = rulerCursorSide(from, view);
  if (side === "in") {
    const inView = {
      t0Ms: Math.max(view.t0Ms, bounds.t0Ms),
      t1Ms: Math.min(view.t1Ms, bounds.t1Ms),
    };
    return moveRulerCursor(r, id, from + direction * step, inView);
  }
  if ((side === "before" && direction > 0) || (side === "after" && direction < 0)) {
    return moveRulerCursor(r, id, side === "before" ? view.t0Ms : view.t1Ms, bounds);
  }
  return stepRulerCursor(r, id, direction, step, bounds);
}

/** A ruler tag's box as laid out, CSS px. */
export interface RulerTagBox {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

/** The gap kept between two tags moved apart, px. */
const TAG_GAP_PX = 2;

/**
 * How far to move each cursor tag down, px (one per tag, in order), so no two tags touch
 * or overlap (AC-AXHK2V.14). Tags that do not touch stay put; the first tag never moves,
 * and each later one drops below any earlier one it would touch.
 */
export function rulerTagOffsets(tags: readonly RulerTagBox[]): number[] {
  const placed: RulerTagBox[] = [];
  return tags.map((t) => {
    let dy = 0;
    for (let moved = true; moved;) {
      moved = false;
      for (const p of placed) {
        const top = t.top + dy;
        const bottom = t.bottom + dy;
        if (t.left <= p.right && p.left <= t.right && top <= p.bottom && p.top <= bottom) {
          dy = p.bottom + TAG_GAP_PX - t.top;
          moved = true;
        }
      }
    }
    placed.push({ ...t, top: t.top + dy, bottom: t.bottom + dy });
    return dy;
  });
}
