/**
 * FR C6VR9K, AC .5: every per-frame canvas loop over data has an iteration cap tied to
 * the plot's pixel size, so no layout fault (a hidden chart laid out at 1 × 1, points a
 * million px off the plot) can make one frame cost millions of canvas calls.
 *
 * API pinned here (`./drawCap`, a new module):
 *
 * - `drawLoopCap(widthPx, heightPx)`: the most iterations one draw loop may take on a plot
 *   of that size (device px). Finite and a whole number for any input (NaN, negative,
 *   zero, Infinity), at least a fixed floor for a tiny or unlaid plot, growing with the
 *   plot's size, at least two per pixel column and row (two PD status dots per column)
 *   and at most a small multiple of the plot's perimeter.
 * - `capStride(count, cap)`: the step a loop over `count` items takes so it visits at
 *   most `cap` of them: a whole number ≥ 1 with `ceil(count / stride) <= cap`.
 *
 * The dashed series stroke through `dashedPaths`, which caps its dash walk at
 * `drawLoopCap(bbox.width, bbox.height)` pieces.
 */

import { describe, expect, it } from "vitest";
import type uPlot from "uplot";
import { dashedPaths } from "./chartSeries";
import { capStride, drawLoopCap } from "./drawCap";

describe("drawLoopCap (AC-C6VR9K.5)", () => {
  it("AC-C6VR9K.5: is a finite whole number for any plot size", () => {
    for (const [w, h] of [
      [1600, 400],
      [1, 1],
      [0, 0],
      [-132, -5],
      [Number.NaN, Number.NaN],
      [Number.POSITIVE_INFINITY, 400],
      [1600, Number.POSITIVE_INFINITY],
      [1e12, 1e12],
    ] as const) {
      const cap = drawLoopCap(w, h);
      expect(Number.isFinite(cap), `drawLoopCap(${w}, ${h}) = ${cap}`).toBe(true);
      expect(Number.isInteger(cap), `drawLoopCap(${w}, ${h}) = ${cap}`).toBe(true);
      expect(cap).toBeGreaterThan(0);
    }
  });

  it("AC-C6VR9K.5: a tiny or unlaid plot gets the same small floor", () => {
    const floor = drawLoopCap(0, 0);
    expect(floor).toBeGreaterThanOrEqual(64);
    expect(floor).toBeLessThanOrEqual(10_000);
    expect(drawLoopCap(1, 1)).toBe(floor);
    expect(drawLoopCap(-149, -132)).toBe(floor);
    expect(drawLoopCap(Number.NaN, 3)).toBe(floor);
  });

  it("AC-C6VR9K.5: grows with the plot, with room for a normal draw", () => {
    expect(drawLoopCap(3200, 1200)).toBeGreaterThan(drawLoopCap(320, 120));
    expect(drawLoopCap(3200, 120)).toBeGreaterThan(drawLoopCap(320, 120));
    expect(drawLoopCap(320, 1200)).toBeGreaterThan(drawLoopCap(320, 120));
    for (const [w, h] of [
      [3200, 1200],
      [1600, 400],
      [800, 160],
    ] as const) {
      expect(drawLoopCap(w, h)).toBeGreaterThanOrEqual(2 * (w + h));
    }
  });

  it("AC-C6VR9K.5: stays a small multiple of the plot's size", () => {
    for (const [w, h] of [
      [3200, 1200],
      [1600, 400],
      [800, 160],
    ] as const) {
      expect(drawLoopCap(w, h)).toBeLessThanOrEqual(32 * (w + h) + 10_000);
    }
    // A plot laid out absurdly wide is still bounded: never more than for a 16k × 16k one.
    expect(drawLoopCap(1e9, 1e9)).toBeLessThanOrEqual(drawLoopCap(16_384, 16_384));
  });
});

describe("capStride (AC-C6VR9K.5)", () => {
  it("AC-C6VR9K.5: visits at most `cap` items", () => {
    for (const [count, cap] of [
      [0, 100],
      [5, 100],
      [100, 100],
      [101, 100],
      [100_000, 3_200],
      [1_000_000, 7],
      [10, 1],
    ] as const) {
      const stride = capStride(count, cap);
      expect(Number.isInteger(stride), `capStride(${count}, ${cap}) = ${stride}`).toBe(true);
      expect(stride).toBeGreaterThanOrEqual(1);
      expect(Math.ceil(count / stride)).toBeLessThanOrEqual(cap);
    }
  });

  it("AC-C6VR9K.5: takes every item while they fit", () => {
    expect(capStride(100, 100)).toBe(1);
    expect(capStride(5, 100)).toBe(1);
    expect(capStride(200, 100)).toBe(2);
  });
});

/** A uPlot stand-in for one series path build: device-px `bbox`, points mapped by
 * `toX` / `toY`, a context that counts path calls. */
function fakePlot(
  bbox: { left: number; top: number; width: number; height: number },
  xs: number[],
  ys: number[],
  toX: (v: number) => number,
  toY: (v: number) => number,
) {
  const calls = { moveTo: 0, lineTo: 0 };
  const noop = (): void => {};
  const ctx = {
    save: noop,
    restore: noop,
    beginPath: noop,
    rect: noop,
    clip: noop,
    translate: noop,
    stroke: noop,
    setLineDash: noop,
    moveTo: () => calls.moveTo++,
    lineTo: () => calls.lineTo++,
    strokeStyle: "",
    lineWidth: 0,
    lineJoin: "",
    lineCap: "",
  };
  const u = {
    ctx,
    bbox,
    data: [xs, ys],
    series: [{}, { scale: "y" }],
    valToPos: (v: number, scale: string) => (scale === "x" ? toX(v) : toY(v)),
  } as unknown as uPlot;
  return { u, calls };
}

describe("dashedPaths stops at the draw cap (AC-C6VR9K.5)", () => {
  it("AC-C6VR9K.5: a dense zigzag inside the plot strokes at most drawLoopCap pieces", () => {
    // 200 000 points zigzagging the whole height of a 100 × 50 plot: ~1 million dashes.
    const bbox = { left: 0, top: 0, width: 100, height: 50 };
    const n = 200_000;
    const xs = Array.from({ length: n }, (_, i) => i);
    const ys = Array.from({ length: n }, (_, i) => (i % 2 === 0 ? 0 : 1));
    const { u, calls } = fakePlot(
      bbox,
      xs,
      ys,
      (v) => (v / n) * 100,
      (v) => (v === 0 ? 50 : 0),
    );
    const build = dashedPaths([2, 2], "#c00", 2);
    build(u, 1, 0, n - 1);
    expect(calls.lineTo).toBeGreaterThan(0);
    expect(calls.lineTo).toBeLessThanOrEqual(drawLoopCap(bbox.width, bbox.height));
    expect(calls.moveTo).toBeLessThanOrEqual(drawLoopCap(bbox.width, bbox.height));
  });
});
