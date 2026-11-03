/**
 * FR AXHK2W, AC .5: axis redraws reuse their labels. Each axis's `values` callback keeps
 * the labels of the last tick set: a redraw with the same ticks (the usual case on a
 * steady stream, each frame passing a fresh `splits` array) gets the same label array
 * back, and a new tick set gets new labels.
 */

import type uPlot from "uplot";
import { describe, expect, it } from "vitest";
import { chartOptions, defaultVisibility, timeTicks } from "./chartSeries";

type Values = (u: uPlot, splits: number[], axis: number, space: number, incr: number) => unknown;

function axisValues(originMs = 0): Values[] {
  const opts = chartOptions({
    theme: "light",
    visibility: defaultVisibility(),
    width: 600,
    height: 300,
    originMs: () => originMs,
  });
  return (opts.axes ?? []).map((a, i) => {
    const values = a.values;
    if (typeof values !== "function") throw new Error(`axis ${i} has no values callback`);
    return values as unknown as Values;
  });
}

const u = {} as uPlot;

describe("axis labels per tick set (AC-AXHK2W.5)", () => {
  it("AC-AXHK2W.5: the time axis gives the same labels back for the same ticks", () => {
    const [time] = axisValues(1_000);
    const a = time!(u, [1_000, 2_000, 3_000], 0, 50, 1_000);
    for (let frame = 0; frame < 30; frame++) {
      expect(time!(u, [1_000, 2_000, 3_000], 0, 50, 1_000)).toBe(a);
    }
    expect(a).toEqual(timeTicks([0, 1_000, 2_000], 1_000));
  });

  it("AC-AXHK2W.5: every value axis gives the same labels back for the same ticks", () => {
    const [, ...valueAxes] = axisValues();
    expect(valueAxes.length).toBeGreaterThan(0);
    valueAxes.forEach((values, i) => {
      const a = values(u, [0, 0.5, 1, 1.5], i + 1, 50, 0.5);
      for (let frame = 0; frame < 30; frame++) {
        expect(values(u, [0, 0.5, 1, 1.5], i + 1, 50, 0.5)).toBe(a);
      }
    });
  });

  it("AC-AXHK2W.5: new ticks get new labels", () => {
    const [time, volts] = axisValues();
    const t1 = time!(u, [0, 1_000], 0, 50, 1_000);
    const t2 = time!(u, [0, 2_000], 0, 50, 2_000);
    expect(t2).not.toBe(t1);
    expect(t2).not.toEqual(t1);
    const v1 = volts!(u, [0, 1, 2], 1, 50, 1);
    const v2 = volts!(u, [0, 0.5, 1], 1, 50, 0.5);
    expect(v2).not.toEqual(v1);
    // Same splits, a finer step: more decimals, so new labels too.
    const v3 = volts!(u, [0, 1, 2], 1, 50, 0.5);
    expect(v3).not.toEqual(v1);
    // And back to the first set: its labels again.
    expect(volts!(u, [0, 1, 2], 1, 50, 1)).toEqual(v1);
  });

  it("AC-AXHK2W.5: a moved session origin relabels the same ticks", () => {
    let origin = 0;
    const opts = chartOptions({
      theme: "light",
      visibility: defaultVisibility(),
      width: 600,
      height: 300,
      originMs: () => origin,
    });
    const time = opts.axes![0]!.values as unknown as Values;
    const before = time(u, [5_000, 6_000], 0, 50, 1_000);
    origin = 5_000;
    const after = time(u, [5_000, 6_000], 0, 50, 1_000);
    expect(after).not.toEqual(before);
  });
});
