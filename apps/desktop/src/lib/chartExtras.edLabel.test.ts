/**
 * FR AXHK2V (AC-AXHK2V.15): the "E.D. %" label never overlaps a segment-mark label or an
 * axis tick label; mark labels give way to it.
 *
 * API pinned here, beyond the FR's Technical Design:
 *
 * - `edLabelBox(u, points)` in `chartExtras.ts`: the box of the "E.D. %" label that
 *   `paintEnergyShare(u, points, color)` writes, canvas px (`LabelBox`), or null without a
 *   line. It lies inside the plot and clear of every tick label `tickLabelBoxes(u)`
 *   reports; `paintEnergyShare` writes the label inside that box;
 * - `paintSegmentMarks(u, marks, color, avoid = [])`: a mark label that would overlap an
 *   `avoid` box (the chart passes the E.D. label's box) is left out, as one over a tick
 *   label is; the dashed lines stay;
 * - the e2e hook's `chartExtras()` gains `edLabel` (the "E.D. %" label's box, viewport CSS
 *   px, in the frame of `markLabels` / `tickLabels`) while the label is drawn.
 */

import { describe, expect, it, vi } from "vitest";
import {
  edColor,
  edLabelBox,
  paintEnergyShare,
  paintSegmentMarks,
  tickLabelBoxes,
  type EnergySharePoints,
  type LabelBox,
  type SegmentMark,
} from "./chartExtras";

interface Call {
  op: string;
  args: unknown[];
}

/** A uPlot stand-in that records what is drawn on its context; `axes` as uPlot resolves
 * them. The plot is 400 × 200 px at (40, 10); x 0..4000 ms maps to 40..440 px. */
function fakePlot(axes: unknown[] = []) {
  const calls: Call[] = [];
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
      calls.push({ op, args });
    });
  }
  ctx.measureText = (s: string) => ({ width: s.length * 6 });
  const u = {
    ctx,
    axes,
    bbox: { left: 40, top: 10, width: 400, height: 200 },
    scales: { x: { min: 0, max: 4_000 } },
    valToPos: (v: number) => 40 + v / 10,
  };
  return { u, calls };
}

/** A top axis whose one tick label ("100%", 24 × 10 px) sits at x 338..362, y 12..22:
 * just where the E.D. label of {@link rising} would go. */
const TOP_AXIS = {
  scale: "x",
  side: 0,
  _show: true,
  _pos: 22,
  _size: 20,
  _splits: [3_100],
  _values: ["100%"],
  font: ["10px sans-serif", 10, 10],
  ticks: { show: false },
  gap: 0,
};

/** E.D. rising to 90% at 3 s: the line ends at (340, 30). */
const rising: EnergySharePoints = { tMs: [0, 1_000, 2_000, 3_000], pct: [0, 30, 60, 90] };

/** E.D. reaching 100% at 3 s: the line ends at the top of the plot, in the mark-label row. */
const full: EnergySharePoints = { tMs: [0, 1_000, 2_000, 3_000], pct: [0, 40, 80, 100] };

function overlaps(a: LabelBox, b: LabelBox): boolean {
  return a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom;
}

function mark(tMs: number, percent: number): SegmentMark {
  return { tMs, percent, label: `${percent}%` };
}

describe('the "E.D. %" label keeps clear of other labels (AC-AXHK2V.15)', () => {
  it("AC-AXHK2V.15: the label's box sits inside the plot, where the label is written", () => {
    const { u, calls } = fakePlot();
    const box = edLabelBox(u as never, rising);
    expect(box).not.toBeNull();
    const b = box!;
    expect(b.right - b.left).toBeGreaterThan(0);
    expect(b.left).toBeGreaterThanOrEqual(40);
    expect(b.right).toBeLessThanOrEqual(440);
    expect(b.top).toBeGreaterThanOrEqual(10);
    expect(b.bottom).toBeLessThanOrEqual(210);
    paintEnergyShare(u as never, rising, edColor("light"));
    const label = calls.find((c) => c.op === "fillText" && c.args[0] === "E.D. %");
    expect(label, 'draws "E.D. %"').toBeDefined();
    const x = Number(label?.args[1]);
    const y = Number(label?.args[2]);
    expect(x).toBeGreaterThanOrEqual(b.left);
    expect(x).toBeLessThanOrEqual(b.right);
    expect(y).toBeGreaterThanOrEqual(b.top);
    expect(y).toBeLessThanOrEqual(b.bottom);
  });

  it("AC-AXHK2V.15: no box without a line", () => {
    const { u } = fakePlot();
    expect(edLabelBox(u as never, { tMs: [], pct: [] })).toBeNull();
    expect(edLabelBox(u as never, { tMs: [0, 1_000], pct: [null, null] })).toBeNull();
  });

  it("AC-AXHK2V.15: the label never covers an axis tick label", () => {
    const { u } = fakePlot([TOP_AXIS]);
    const ticks = tickLabelBoxes(u as never);
    expect(ticks).toHaveLength(1);
    const box = edLabelBox(u as never, rising);
    expect(box).not.toBeNull();
    for (const t of ticks) expect(overlaps(box!, t), "E.D. % over a tick label").toBe(false);
  });

  it("AC-AXHK2V.15: a mark label over the E.D. label gives way; the others and every line stay", () => {
    const { u, calls } = fakePlot();
    const box = edLabelBox(u as never, full);
    expect(box).not.toBeNull();
    // Marks at 0.5 s, 1.5 s and one whose label (at x + 2, the row 12..22) starts where
    // the E.D. label does.
    const under = (box!.left - 40 - 2) * 10;
    const underLabel = { left: box!.left, top: 12, right: box!.left + 18, bottom: 22 };
    expect(overlaps(underLabel, box!), "the 95% label would sit on E.D. %").toBe(true);
    const marks = [mark(500, 25), mark(1_500, 50), mark(under, 95)];
    const painted = paintSegmentMarks(u as never, marks, "#888", [box!]);
    const texts = calls.filter((c) => c.op === "fillText").map((c) => c.args[0]);
    expect(texts).toContain("25%");
    expect(texts).toContain("50%");
    expect(texts).not.toContain("95%");
    expect(painted.markLabels.map((m) => m.label)).toEqual(["25%", "50%"]);
    for (const m of painted.markLabels) expect(overlaps(m, box!)).toBe(false);
    // Three dashed lines, one per mark, whatever the labels did.
    expect(calls.filter((c) => c.op === "stroke")).toHaveLength(3);
  });

  it("AC-AXHK2V.15: with nothing to avoid the marks draw as before", () => {
    const { u, calls } = fakePlot();
    paintSegmentMarks(u as never, [mark(500, 25), mark(3_000, 95)], "#888", []);
    const texts = calls.filter((c) => c.op === "fillText").map((c) => c.args[0]);
    expect(texts).toEqual(["25%", "95%"]);
  });
});
