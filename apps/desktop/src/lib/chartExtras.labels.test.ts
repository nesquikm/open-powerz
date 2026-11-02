/**
 * FR AXHK2V: segment-mark labels that never cover an axis tick label (AC .12) and an
 * E.D. line in its own colour with its own label (AC .13), in `chartExtras.ts`.
 *
 * API pinned here, beyond the FR's Technical Design:
 *
 * - `LabelBox`: `{ left, top, right, bottom }`, px, one frame for all boxes;
 *   `markLabelsShown(labels, blocked)` says which mark labels to draw (one boolean per
 *   label, in order) so no drawn label overlaps another or any `blocked` box (the axis
 *   tick labels): all of them when they fit, else every other one (the first, third, …),
 *   else none; a label that would cover a tick label is always left out. The dashed
 *   lines are drawn whatever it says;
 * - `edColor(theme)`: the E.D. line's colour; `paintEnergyShare(u, points, color)` also
 *   writes "E.D. %" at the line's right end.
 */

import { describe, expect, it, vi } from "vitest";
import {
  edColor,
  markLabelsShown,
  paintEnergyShare,
  type EnergySharePoints,
  type LabelBox,
} from "./chartExtras";
import { seriesStyles } from "./chartSeries";
import { THEME_COLORS, contrastRatio } from "./theme";

/** A label `width` px wide at `left`, in the row at the top of the plot. */
function box(left: number, width = 20): LabelBox {
  return { left, top: 10, right: left + width, bottom: 22 };
}

function overlaps(a: LabelBox, b: LabelBox): boolean {
  return a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom;
}

describe("segment-mark labels never cover a tick label (AC-AXHK2V.12)", () => {
  it("AC-AXHK2V.12: all labels show when they fit", () => {
    const labels = [box(50), box(100), box(150), box(200)];
    expect(markLabelsShown(labels, [])).toEqual([true, true, true, true]);
  });

  it("AC-AXHK2V.12: crowded labels thin out to every other one", () => {
    // 20 px labels 15 px apart overlap their neighbours, but not the one after.
    const labels = [0, 15, 30, 45, 60, 75].map((x) => box(100 + x));
    expect(markLabelsShown(labels, [])).toEqual([true, false, true, false, true, false]);
  });

  it("AC-AXHK2V.12: labels too crowded even for every other one are left out", () => {
    const labels = [0, 5, 10, 15, 20, 25].map((x) => box(100 + x));
    expect(markLabelsShown(labels, [])).toEqual([false, false, false, false, false, false]);
  });

  it("AC-AXHK2V.12: a label over a tick label is left out, the rest stay", () => {
    const labels = [box(50), box(100), box(150), box(200)];
    const tick = { left: 145, top: 12, right: 175, bottom: 24 };
    expect(markLabelsShown(labels, [tick])).toEqual([true, true, false, true]);
  });

  it("AC-AXHK2V.12: a tick label in another row blocks nothing", () => {
    const labels = [box(50), box(100)];
    const bottomTick = { left: 95, top: 300, right: 130, bottom: 312 };
    expect(markLabelsShown(labels, [bottomTick])).toEqual([true, true]);
  });

  it("AC-AXHK2V.12: whatever it shows, no two shown labels and no tick label overlap", () => {
    for (let gap = 1; gap <= 40; gap += 3) {
      const labels = Array.from({ length: 19 }, (_, k) => box(40 + k * gap, 24));
      const ticks = [
        { left: 0, top: 4, right: 36, bottom: 16 },
        { left: 40 + 9 * gap, top: 8, right: 40 + 9 * gap + 30, bottom: 20 },
        { left: 300, top: 0, right: 340, bottom: 400 },
      ];
      const shown = markLabelsShown(labels, ticks);
      expect(shown).toHaveLength(labels.length);
      const drawn = labels.filter((_, k) => shown[k]);
      for (const a of drawn) {
        for (const t of ticks) expect(overlaps(a, t)).toBe(false);
        for (const b of drawn) if (a !== b) expect(overlaps(a, b)).toBe(false);
      }
    }
  });
});

/** A uPlot stand-in that records what is drawn on its context. */
function fakePlot() {
  const calls: { op: string; args: unknown[]; strokeStyle: unknown; fillStyle: unknown }[] = [];
  const ctx: Record<string, unknown> = {
    strokeStyle: "",
    fillStyle: "",
    lineWidth: 1,
    font: "",
    textAlign: "start",
    textBaseline: "alphabetic",
    globalAlpha: 1,
  };
  for (const op of [
    "save",
    "restore",
    "beginPath",
    "rect",
    "clip",
    "moveTo",
    "lineTo",
    "stroke",
    "fill",
    "setLineDash",
    "fillText",
    "strokeText",
    "fillRect",
  ]) {
    ctx[op] = vi.fn((...args: unknown[]) => {
      calls.push({ op, args, strokeStyle: ctx.strokeStyle, fillStyle: ctx.fillStyle });
    });
  }
  ctx.measureText = (s: string) => ({ width: s.length * 6 });
  const u = {
    ctx,
    bbox: { left: 40, top: 10, width: 400, height: 200 },
    scales: { x: { min: 0, max: 4_000 } },
    valToPos: (v: number) => 40 + v / 10,
  };
  return { u, calls };
}

describe("the E.D. line has its own colour and label (AC-AXHK2V.13)", () => {
  it("AC-AXHK2V.13: its colour is none of the series colours, nor the accent", () => {
    for (const theme of ["light", "dark"] as const) {
      const ed = edColor(theme).toLowerCase();
      for (const s of seriesStyles(theme)) expect(ed).not.toBe(s.stroke.toLowerCase());
      expect(ed).not.toBe(THEME_COLORS[theme].accent.toLowerCase());
    }
  });

  it("AC-AXHK2V.13: it still reads as a chart line on the theme's background", () => {
    for (const theme of ["light", "dark"] as const) {
      expect(contrastRatio(edColor(theme), THEME_COLORS[theme].bg)).toBeGreaterThanOrEqual(3);
    }
  });

  it('AC-AXHK2V.13: a small "E.D. %" label sits at the line\'s right end', () => {
    const { u, calls } = fakePlot();
    const points: EnergySharePoints = { tMs: [0, 1_000, 2_000, 3_000], pct: [0, 30, 60, 90] };
    paintEnergyShare(u as never, points, edColor("light"));
    const label = calls.find((c) => c.op === "fillText" && c.args[0] === "E.D. %");
    expect(label, 'draws "E.D. %"').toBeDefined();
    // The last point: x = 40 + 3000 / 10 = 340, y = 10 + 200 − 0.9 × 200 = 30.
    const x = Number(label?.args[1]);
    const y = Number(label?.args[2]);
    expect(Math.abs(x - 340)).toBeLessThanOrEqual(80);
    expect(Math.abs(y - 30)).toBeLessThanOrEqual(20);
    expect(String(label?.fillStyle).toLowerCase()).toBe(edColor("light").toLowerCase());
  });

  it("AC-AXHK2V.13: no label without a line", () => {
    const { u, calls } = fakePlot();
    paintEnergyShare(u as never, { tMs: [], pct: [] }, edColor("dark"));
    expect(calls.filter((c) => c.op === "fillText")).toEqual([]);
  });
});
