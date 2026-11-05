/**
 * FR C6VR9K: the Meter tab's chart stays mounted, sized 1 × 1, while another tab shows.
 * Its bbox went negative (height −132) and its points landed ~1.3 million px off the
 * plot, and `dashedPaths` walked those segments one dash at a time: ~1.5 million canvas
 * path calls a frame.
 *
 * - AC .3: a plot whose bbox has no area (hidden or smaller than its axes) strokes nothing.
 * - AC .4: the dash walk is clipped to the plot box, so points far off the plot cost only
 *   the pieces inside it.
 */

import { describe, expect, it } from "vitest";
import type uPlot from "uplot";
import { dashedPaths } from "./chartSeries";

type BBox = { left: number; top: number; width: number; height: number };

function fakePlot(bbox: BBox, n: number, toX: (i: number) => number, toY: (i: number) => number) {
  const calls = { moveTo: 0, lineTo: 0, stroke: 0 };
  const noop = (): void => {};
  const ctx = {
    save: noop,
    restore: noop,
    beginPath: noop,
    rect: noop,
    clip: noop,
    translate: noop,
    setLineDash: noop,
    stroke: () => calls.stroke++,
    moveTo: () => calls.moveTo++,
    lineTo: () => calls.lineTo++,
    strokeStyle: "",
    lineWidth: 0,
    lineJoin: "",
    lineCap: "",
  };
  const xs = Array.from({ length: n }, (_, i) => i);
  const ys = Array.from({ length: n }, (_, i) => i);
  const u = {
    ctx,
    bbox,
    data: [xs, ys],
    series: [{}, { scale: "y" }],
    valToPos: (v: number, scale: string) => (scale === "x" ? toX(v) : toY(v)),
  } as unknown as uPlot;
  return { u, calls };
}

describe("dashedPaths on a hidden or mislaid plot (AC-C6VR9K.3, AC-C6VR9K.4)", () => {
  it("AC-C6VR9K.4: points a million px off either side cost only the pieces inside the plot", () => {
    // The hidden chart's layout: 600 samples spread from −1.3e6 to +1.3e6 device px.
    const bbox: BBox = { left: 0, top: 0, width: 1000, height: 400 };
    const n = 600;
    const { u, calls } = fakePlot(
      bbox,
      n,
      (i) => -1_300_000 + (i / (n - 1)) * 2_600_000,
      (i) => (i % 2 === 0 ? 100 : 300),
    );
    const started = performance.now();
    dashedPaths([6, 4], "#c00", 2)(u, 1, 0, n - 1);
    const took = performance.now() - started;
    // The diagonal is ~1077 px: about 108 [6, 4] dashes, never the ~260 000 of the walk.
    expect(calls.lineTo).toBeLessThanOrEqual(2 * (Math.ceil(1077 / 10) + 2));
    expect(calls.moveTo).toBeLessThanOrEqual(calls.lineTo);
    expect(took, `took ${took.toFixed(1)} ms`).toBeLessThan(50);
  });

  it("AC-C6VR9K.4: a series wholly left of the plot strokes nothing", () => {
    const bbox: BBox = { left: 0, top: 0, width: 1000, height: 400 };
    const n = 2_000;
    const { u, calls } = fakePlot(
      bbox,
      n,
      (i) => -1_300_000 + i * 500,
      (i) => (i % 2 === 0 ? -132 : 500),
    );
    dashedPaths([6, 4], "#c00", 2)(u, 1, 0, n - 1);
    expect(calls.lineTo).toBe(0);
    expect(calls.moveTo).toBe(0);
  });

  it("AC-C6VR9K.3: a plot with no area (hidden at 1 × 1, padded past its size) draws nothing", () => {
    for (const bbox of [
      { left: 30, top: 10, width: -149, height: -132 },
      { left: 0, top: 0, width: 0, height: 0 },
      { left: 0, top: 0, width: 1000, height: -132 },
      { left: 0, top: 0, width: -5, height: 400 },
    ]) {
      const { u, calls } = fakePlot(
        bbox,
        200,
        (i) => i * 3,
        (i) => (i % 2 === 0 ? 0 : 100),
      );
      dashedPaths([6, 4], "#c00", 2)(u, 1, 0, 199);
      expect(calls.lineTo, JSON.stringify(bbox)).toBe(0);
      expect(calls.moveTo, JSON.stringify(bbox)).toBe(0);
      expect(calls.stroke, JSON.stringify(bbox)).toBe(0);
    }
  });

  it("draws as before on a laid-out plot", () => {
    const bbox: BBox = { left: 0, top: 0, width: 1000, height: 400 };
    const { u, calls } = fakePlot(
      bbox,
      2,
      (i) => i * 150,
      () => 5,
    );
    // One 150 px segment of [10, 5]: ten dashes.
    dashedPaths([10, 5], "#c00", 2)(u, 1, 0, 1);
    expect(calls.lineTo).toBe(10);
    expect(calls.moveTo).toBe(10);
  });
});
