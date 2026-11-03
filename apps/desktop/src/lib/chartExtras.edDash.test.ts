/**
 * FR SX1VTC: the E.D. line's dashes stay on the data while the chart scrolls (AC .4).
 *
 * `paintEnergyShare` stroked the line with a canvas dash (`setLineDash([6, 3] × dpr)`),
 * which starts at the first point on screen and so slides as the view moves. It now lays
 * its dashes with the shared dash builder anchored to the points' data times, as the
 * series do: no canvas dash on the line, each dash a `moveTo` + `lineTo` run.
 */

import { describe, expect, it } from "vitest";
import { edColor, paintEnergyShare, type EnergySharePoints } from "./chartExtras";

const BBOX = { left: 40, top: 10, width: 600, height: 200 };

/** The E.D. points: every 20 ms over 40 s, a wavy rising share. */
const POINTS: EnergySharePoints = (() => {
  const tMs: number[] = [];
  const pct: (number | null)[] = [];
  for (let t = 0; t <= 40_000; t += 20) {
    tMs.push(t);
    pct.push(Math.min(100, (t / 40_000) * 80 + 10 * Math.sin(t / 300) + 10));
  }
  return { tMs, pct };
})();

interface Drawn {
  t0: number;
  msPerPx: number;
  dashes: [number, number][];
  lineDashes: number[][];
}

function drawFrame(t0: number, msPerPx: number): Drawn {
  const dashes: [number, number][] = [];
  const lineDashes: number[][] = [];
  const noop = (): void => {};
  const ctx = {
    save: noop,
    restore: noop,
    beginPath: noop,
    rect: noop,
    clip: noop,
    translate: noop,
    stroke: noop,
    fillText: noop,
    setLineDash: (d: number[]) => lineDashes.push([...d]),
    moveTo: (x: number) => dashes.push([x, x]),
    lineTo: (x: number) => {
      const d = dashes.at(-1);
      if (d) d[1] = x;
    },
    measureText: (s: string) => ({ width: s.length * 6 }),
    strokeStyle: "",
    fillStyle: "",
    lineWidth: 1,
    font: "",
    textAlign: "start",
    textBaseline: "alphabetic",
  };
  const u = {
    ctx,
    axes: [],
    bbox: BBOX,
    scales: { x: { min: t0, max: t0 + BBOX.width * msPerPx } },
    valToPos: (v: number) => BBOX.left + (v - t0) / msPerPx,
    posToVal: (px: number) => t0 + (px - BBOX.left) * msPerPx,
  };
  // No label box: only the line is drawn.
  paintEnergyShare(u as never, POINTS, edColor("light"), null);
  return { t0, msPerPx, dashes, lineDashes };
}

describe("the E.D. line's dashes are anchored to data time (AC-SX1VTC.4)", () => {
  it("AC-SX1VTC.4: the line sets no canvas dash; it is laid as dash pieces", () => {
    const f = drawFrame(10_000, 10);
    for (const d of f.lineDashes) expect(d, "a canvas dash on the E.D. line").toEqual([]);
    // ~600 px of line in [6, 3] dashes: dozens of separate runs.
    expect(f.dashes.length).toBeGreaterThan(40);
  });

  it("AC-SX1VTC.4: a scroll leaves every E.D. dash on the same data times, within 0.5 px", () => {
    const msPerPx = 10;
    const a = drawFrame(10_000, msPerPx);
    const toT = (f: Drawn, x: number): number => f.t0 + (x - BBOX.left) * f.msPerPx;
    for (const dt of [3.7, 10, 55, 777.7, 2_500, 9_000]) {
      const b = drawFrame(10_000 + dt, msPerPx);
      const lo = 10_000 + dt + msPerPx;
      const hi = 10_000 + BBOX.width * msPerPx - msPerPx;
      const sa = a.dashes
        .map(([x0, x1]): [number, number] => [toT(a, x0), toT(a, x1)])
        .filter(([s, e]) => s >= lo && e <= hi);
      if (dt < 3_000) expect(sa.length, `Δt ${dt}: dashes compared`).toBeGreaterThan(10);
      const sb = b.dashes.map(([x0, x1]): [number, number] => [toT(b, x0), toT(b, x1)]);
      for (const [s, e] of sa) {
        const best = sb.reduce(
          (m, [s2, e2]) => Math.min(m, Math.max(Math.abs(s2 - s), Math.abs(e2 - e))),
          Infinity,
        );
        expect(
          best / msPerPx,
          `Δt ${dt}: E.D. dash at ${s.toFixed(1)} ms moved`,
        ).toBeLessThanOrEqual(0.5);
      }
    }
  });
});
