/**
 * FR SX1VTC: the chart's dashed series keep their dashes on the data while it scrolls.
 *
 * `dashedPaths` is the uPlot paths builder of every series in `chartOptions`, which the
 * main chart, the sub-chart and the recording views all build from (AC .4). uPlot hands
 * it only the window in view (`idx0..idx1`); it anchors the pattern to the data times
 * `u.data[0]` with a grid step from `dashGridStepMs`, and lays the first cell in view from
 * its own grid time (AC .1, .3).
 *
 * The fake plot strokes into a recording context: each `moveTo` starts a dash, each
 * `lineTo` extends it.
 */

import { describe, expect, it } from "vitest";
import type uPlot from "uplot";
import {
  CHART_SERIES,
  chartOptions,
  dashedPaths,
  defaultVisibility,
  seriesStroke,
  seriesStyles,
} from "./chartSeries";

const BBOX = { left: 20, top: 10, width: 800, height: 300 };
/** Sample times, ms: 250 SPS for 30 s. */
const TS = Array.from({ length: 7_501 }, (_, i) => i * 4);
const AMPS = TS.map((t) => 1 + 0.3 * Math.sin(t / 170) + 0.05 * Math.sin(t / 9));

interface Drawn {
  t0: number;
  msPerPx: number;
  /** Each dash's first and last x, canvas px. */
  dashes: [number, number][];
}

/** Draws series `seriesIdx` of `paths` on a plot whose view starts at `t0` ms. */
function drawFrame(
  paths: uPlot.Series.PathBuilder,
  t0: number,
  msPerPx: number,
  series: uPlot.Series[],
  seriesIdx = 2,
): Drawn {
  const dashes: [number, number][] = [];
  const noop = (): void => {};
  const ctx = {
    save: noop,
    restore: noop,
    beginPath: noop,
    rect: noop,
    clip: noop,
    translate: noop,
    setLineDash: noop,
    stroke: noop,
    moveTo: (x: number) => dashes.push([x, x]),
    lineTo: (x: number) => {
      const d = dashes.at(-1);
      if (d) d[1] = x;
    },
    strokeStyle: "",
    lineWidth: 0,
    lineJoin: "",
    lineCap: "",
  };
  const t1 = t0 + BBOX.width * msPerPx;
  const yRange = { min: 0, max: 2 };
  const data = [TS, ...CHART_SERIES.map(() => AMPS)];
  const u = {
    ctx,
    bbox: BBOX,
    data,
    series,
    width: BBOX.width + 40,
    height: BBOX.height + 40,
    pxRatio: 1,
    scales: {
      x: { min: t0, max: t1 },
      amps: yRange,
      volts: yRange,
      watts: yRange,
    },
    valToPos: (v: number, scale: string) =>
      scale === "x"
        ? BBOX.left + (v - t0) / msPerPx
        : BBOX.top + BBOX.height - ((v - yRange.min) / (yRange.max - yRange.min)) * BBOX.height,
    posToVal: (px: number, scale: string) =>
      scale === "x"
        ? t0 + (px - BBOX.left) * msPerPx
        : yRange.min + ((BBOX.top + BBOX.height - px) / BBOX.height) * (yRange.max - yRange.min),
  } as unknown as uPlot;
  // uPlot's window: the last point at or before the left edge to the first at or after
  // the right one.
  let idx0 = 0;
  for (let i = 0; i < TS.length; i++) if (TS[i]! <= t0) idx0 = i;
  let idx1 = TS.length - 1;
  for (let i = TS.length - 1; i >= 0; i--) if (TS[i]! >= t1) idx1 = i;
  paths(u, seriesIdx, idx0, idx1);
  return { t0, msPerPx, dashes };
}

/** Dashes fully inside both frames' plot (1 px in) lie on the same data times, ≤ 0.5 px. */
function expectGlued(a: Drawn, b: Drawn, what: string): number {
  const toT = (f: Drawn, x: number): number => f.t0 + (x - BBOX.left) * f.msPerPx;
  const lo = Math.max(a.t0, b.t0) + a.msPerPx;
  const hi = Math.min(a.t0, b.t0) + BBOX.width * a.msPerPx - a.msPerPx;
  const spans = (f: Drawn): [number, number][] =>
    f.dashes.map(([x0, x1]) => [toT(f, x0), toT(f, x1)]);
  const sa = spans(a).filter(([s, e]) => s >= lo && e <= hi);
  const sb = spans(b);
  for (const [s, e] of sa) {
    const best = sb.reduce(
      (m, [s2, e2]) => Math.min(m, Math.max(Math.abs(s2 - s), Math.abs(e2 - e))),
      Infinity,
    );
    expect(best / a.msPerPx, `${what}: dash at ${s.toFixed(1)} ms moved`).toBeLessThanOrEqual(0.5);
  }
  return sa.length;
}

describe("the chart's dashed series stay on their data while it scrolls (AC-SX1VTC.1, .3, .4)", () => {
  // Whole-pixel shifts at 5 ms/px, so uPlot's rounding to device px is the same in both
  // frames; a cell, several cells and a Go-live jump.
  const SHIFTS = [5, 35, 415, 1_235, 6_000];

  it("AC-SX1VTC.1: dashedPaths keeps every [12, 4] dash in view on the same samples", () => {
    const paths = dashedPaths([12, 4], "#c00", 2);
    const series = [{}, { scale: "amps" }, { scale: "amps" }] as uPlot.Series[];
    const a = drawFrame(paths, 2_000, 5, series);
    expect(a.dashes.length).toBeGreaterThan(20);
    for (const dt of SHIFTS) {
      const n = expectGlued(a, drawFrame(paths, 2_000 + dt, 5, series), `Δt ${dt} ms`);
      if (dt < 3_000) expect(n).toBeGreaterThan(5);
    }
  });

  it("AC-SX1VTC.4: every dashed series of chartOptions (main chart, sub-chart, recording view) is glued", () => {
    for (const pxRatio of [1, 2]) {
      const opts = chartOptions({
        theme: "light",
        visibility: defaultVisibility(),
        width: 860,
        height: 340,
        pxRatio,
      });
      CHART_SERIES.forEach((s, i) => {
        if (seriesStroke(s.key, pxRatio).dash.length === 0) return;
        const paths = opts.series[i + 1]!.paths as uPlot.Series.PathBuilder;
        const a = drawFrame(paths, 3_000, 5, opts.series, i + 1);
        expect(a.dashes.length, `${s.key} at DPR ${pxRatio} is dashed`).toBeGreaterThan(20);
        for (const dt of SHIFTS) {
          expectGlued(
            a,
            drawFrame(paths, 3_000 + dt, 5, opts.series, i + 1),
            `${s.key} at DPR ${pxRatio}, Δt ${dt}`,
          );
        }
      });
    }
  });

  it("AC-SX1VTC.3: a pan back and forth lands every dash where it was", () => {
    const paths = dashedPaths([10, 3, 2, 3], "#c00", 2);
    const series = [{}, { scale: "amps" }, { scale: "amps" }] as uPlot.Series[];
    const a = drawFrame(paths, 9_000, 5, series);
    drawFrame(paths, 9_000 - 2_345, 5, series);
    const back = drawFrame(paths, 9_000, 5, series);
    expect(back.dashes).toEqual(a.dashes);
    expect(expectGlued(a, drawFrame(paths, 9_000 - 115, 5, series), "pan left")).toBeGreaterThan(5);
  });
});

describe("legend swatches and patterns are unchanged (AC-SX1VTC.4)", () => {
  // The legend (SeriesLegend.svelte) draws each swatch with `seriesStyles(theme)[i].dash`
  // as its stroke-dasharray; the exports (chartSvg.test.ts, chartExport.test.ts) keep
  // theirs. The patterns themselves do not change with this fix.
  it("AC-SX1VTC.4: every series keeps its own pattern", () => {
    for (const theme of ["light", "dark"] as const) {
      const dash = Object.fromEntries(seriesStyles(theme).map((s) => [s.key, s.dash]));
      expect(dash).toEqual({
        vbus: [],
        ibus: [12, 4],
        power: [2, 3],
        dp: [10, 3, 2, 3],
        dm: [4, 4],
        cc1: [12, 3, 2, 3, 2, 3],
        cc2: [6, 3, 6, 3, 2, 3],
      });
    }
  });
});
