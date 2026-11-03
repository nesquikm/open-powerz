/**
 * FR SX1VTC: dashed series crawl while the chart scrolls.
 *
 * `forEachDashPiece` started the pattern at phase 0 on the first point it was handed, and
 * the chart hands it the points in view: a scroll changes that point every frame, so the
 * dashes slid along the line. The fix anchors the pattern in data time.
 *
 * Interface pinned here:
 * - `forEachDashPiece(xs, ys, pattern, piece, clip?, cap?, anchor?)`: with
 *   `anchor = { ts, stepMs }` (`ts[i]` is point i's data time, ms), the phase resets to 0
 *   at every multiple of `stepMs` the line crosses (at the point interpolated on the
 *   segment), and inside a cell it advances by screen arc length. The last dash before a
 *   grid time may be cut short. Without `anchor` nothing changes (dashPath.test.ts).
 * - `dashGridStepMs(msPerPx, patternPeriodPx)`: the smallest 1-2-5 × 10^k ms step whose
 *   cell spans at least four pattern lengths on screen (`step / msPerPx ≥ 4 × period`).
 *
 * The caller (`dashedPaths`) hands the builder points from the grid time at or before the
 * view's left edge, so the first cell in view is laid from its own grid time; the pure
 * frames below do the same.
 */

import { describe, expect, it } from "vitest";
import { dashGridStepMs, forEachDashPiece, type DashClip, type DashPiece } from "./dashPath";

const EXACT = 1e-6;

/** A dense, wavy series in screen-y px at data time `t` (ms). */
function wave(t: number): number {
  return 150 + 60 * Math.sin(t / 90) + 9 * Math.sin(t / 7);
}

/** Sample times of a 1000 SPS stream from 0.37 ms on, so grid times fall between samples. */
function sampleTimes(endMs: number): number[] {
  return Array.from({ length: endMs + 1 }, (_, i) => i + 0.37);
}

interface Frame {
  t0: number;
  t1: number;
  msPerPx: number;
  pieces: DashPiece[];
}

/** One drawn frame: view `[t0, t0 + widthPx × msPerPx]`, x = (t − t0) / msPerPx, the
 * points from the sample at or before the previous grid time to the first sample past
 * the view, clipped to the view. */
function drawFrame(
  t0: number,
  opts: {
    msPerPx: number;
    widthPx: number;
    stepMs: number;
    pattern: readonly number[];
    y?: (t: number) => number;
    samples?: readonly number[];
  },
): Frame {
  const { msPerPx, widthPx, stepMs, pattern } = opts;
  const y = opts.y ?? wave;
  const all = opts.samples ?? sampleTimes(20_000);
  const t1 = t0 + widthPx * msPerPx;
  const grid = Math.floor(t0 / stepMs) * stepMs;
  let first = 0;
  for (let i = 0; i < all.length; i++) if (all[i]! <= grid) first = i;
  let last = all.length - 1;
  for (let i = all.length - 1; i >= 0; i--) if (all[i]! >= t1) last = i;
  const ts = all.slice(first, last + 1);
  const xs = ts.map((t) => (t - t0) / msPerPx);
  const ys = ts.map(y);
  const clip: DashClip = { left: 0, top: -1e6, right: widthPx, bottom: 1e6 };
  const pieces: DashPiece[] = [];
  forEachDashPiece(
    xs,
    ys,
    pattern,
    (x0, y0, x1, y1) => pieces.push([x0, y0, x1, y1]),
    clip,
    Infinity,
    { ts, stepMs },
  );
  return { t0, t1, msPerPx, pieces };
}

/** The frame's dashes as data-time spans `[from, to]`: pieces that join end to start are
 * one dash. */
function dashSpans(f: Frame): [number, number][] {
  const out: [number, number][] = [];
  let end: [number, number] | null = null;
  for (const [x0, y0, x1, y1] of f.pieces) {
    const a = f.t0 + x0 * f.msPerPx;
    const b = f.t0 + x1 * f.msPerPx;
    if (end && Math.abs(end[0] - x0) < EXACT && Math.abs(end[1] - y0) < EXACT) {
      out[out.length - 1]![1] = b;
    } else {
      out.push([a, b]);
    }
    end = [x1, y1];
  }
  return out;
}

/** Every dash fully in view on both frames (1 px inside both edges) covers the same data
 * times in both, within `tolPx` screen px. Returns how many were compared. */
function expectGlued(a: Frame, b: Frame, tolPx: number, what: string): number {
  const msPerPx = a.msPerPx;
  const lo = Math.max(a.t0, b.t0) + msPerPx;
  const hi = Math.min(a.t1, b.t1) - msPerPx;
  const inside = (s: [number, number]): boolean => s[0] >= lo && s[1] <= hi;
  const da = dashSpans(a).filter(inside);
  const db = dashSpans(b);
  for (const s of da) {
    const best = db.reduce(
      (m, o) => Math.min(m, Math.max(Math.abs(o[0] - s[0]), Math.abs(o[1] - s[1]))),
      Infinity,
    );
    expect(
      best / msPerPx,
      `${what}: the dash at ${s[0].toFixed(2)}..${s[1].toFixed(2)} ms moved on screen`,
    ).toBeLessThanOrEqual(tolPx);
  }
  // And the other way round: the second frame shows no dash the first does not.
  const da2 = dashSpans(a);
  for (const s of dashSpans(b).filter(inside)) {
    const best = da2.reduce(
      (m, o) => Math.min(m, Math.max(Math.abs(o[0] - s[0]), Math.abs(o[1] - s[1]))),
      Infinity,
    );
    expect(best / msPerPx, `${what}: a new dash at ${s[0].toFixed(2)} ms`).toBeLessThanOrEqual(
      tolPx,
    );
  }
  return da.length;
}

/** Grid times inside the frame's view, with their screen points interpolated on `ts`. */
function gridPoints(
  ts: readonly number[],
  xs: readonly number[],
  ys: readonly number[],
  stepMs: number,
): { t: number; x: number; y: number }[] {
  const out: { t: number; x: number; y: number }[] = [];
  for (let i = 1; i < ts.length; i++) {
    const ta = ts[i - 1]!;
    const tb = ts[i]!;
    for (let g = Math.ceil(ta / stepMs) * stepMs; g <= tb; g += stepMs) {
      if (g <= ta) continue;
      const f = (g - ta) / (tb - ta);
      out.push({
        t: g,
        x: xs[i - 1]! + f * (xs[i]! - xs[i - 1]!),
        y: ys[i - 1]! + f * (ys[i]! - ys[i - 1]!),
      });
    }
  }
  return out;
}

/** Every grid point gets a dash starting on it (phase 0), and no piece runs across one. */
function expectPhaseZeroAtGrid(
  pieces: readonly DashPiece[],
  grid: readonly { t: number; x: number; y: number }[],
  what: string,
): void {
  expect(grid.length, `${what}: grid points to check`).toBeGreaterThan(2);
  for (const g of grid) {
    const starts = pieces.some(
      ([x0, y0]) => Math.abs(x0 - g.x) < 1e-4 && Math.abs(y0 - g.y) < 1e-4,
    );
    expect(starts, `${what}: a dash starts at grid time ${g.t} ms (x ${g.x.toFixed(3)})`).toBe(
      true,
    );
    const across = pieces.find(([x0, , x1]) => x0 < g.x - 1e-4 && x1 > g.x + 1e-4);
    expect(across, `${what}: a piece runs across grid time ${g.t} ms`).toBeUndefined();
  }
}

/** Shifts of the view, ms: sub-pixel, whole-pixel and fractional, a cell and more, a
 * Go-live jump. At 2.5 ms/px, 7.3 ms is 2.92 px. */
const SHIFTS = [1, 7.3, 12.5, 133.7, 250, 999.9, 3_217.4];

describe("dashes stay on their samples while the view shifts (AC-SX1VTC.1)", () => {
  for (const pattern of [
    [12, 4],
    [10, 3, 2, 3],
    [2, 3],
    [12, 3, 2, 3, 2, 3],
  ]) {
    it(`AC-SX1VTC.1: [${pattern.join(", ")}] dashes in view on both frames match within 0.5 px`, () => {
      const opts = { msPerPx: 2.5, widthPx: 400, stepMs: 200, pattern };
      const a = drawFrame(1_003.1, opts);
      for (const dt of SHIFTS) {
        const b = drawFrame(1_003.1 + dt, opts);
        const n = expectGlued(a, b, 0.5, `Δt ${dt} ms`);
        if (dt < 900) expect(n, `Δt ${dt} ms: dashes compared`).toBeGreaterThan(5);
      }
    });
  }
});

describe("the pattern restarts at grid times fixed in data time (AC-SX1VTC.2)", () => {
  it("AC-SX1VTC.2: dashGridStepMs picks the smallest 1-2-5 step with four pattern lengths per cell", () => {
    expect(dashGridStepMs(1, 16)).toBe(100); // needs ≥ 64 ms
    expect(dashGridStepMs(0.1, 16)).toBe(10); // ≥ 6.4 ms
    expect(dashGridStepMs(2, 15)).toBe(200); // ≥ 120 ms
    expect(dashGridStepMs(1, 12.5)).toBe(50); // exactly 50 ms
    expect(dashGridStepMs(2.5, 5)).toBe(50); // ≥ 50 ms
    expect(dashGridStepMs(2.5, 18)).toBe(200); // ≥ 180 ms
    expect(dashGridStepMs(40, 16)).toBe(5_000); // ≥ 2560 ms
    expect(dashGridStepMs(0.003, 9)).toBeCloseTo(0.2, 9); // ≥ 0.108 ms
  });

  it("AC-SX1VTC.2: the step is always 1, 2 or 5 × 10^k and the smallest that fits", () => {
    const ladder = (s: number): number => {
      const e = Math.floor(Math.log10(s) + 1e-9);
      return s / 10 ** e;
    };
    const below = (s: number): number => {
      const m = Math.round(ladder(s));
      const e = Math.round(Math.log10(s / m));
      return m === 1 ? 5 * 10 ** (e - 1) : m === 2 ? 10 ** e : 2 * 10 ** e;
    };
    for (const msPerPx of [0.013, 0.37, 1, 2.5, 7.77, 61, 1_234]) {
      for (const period of [5, 8, 9, 16, 18, 22]) {
        const s = dashGridStepMs(msPerPx, period);
        expect([1, 2, 5]).toContain(Math.round(ladder(s) * 1e6) / 1e6);
        expect(s / msPerPx).toBeGreaterThanOrEqual(4 * period * (1 - 1e-9));
        expect(below(s) / msPerPx, `${msPerPx} ms/px, ${period} px`).toBeLessThan(4 * period);
      }
    }
  });

  it("AC-SX1VTC.2: dashGridStepMs stays finite and positive on bad input", () => {
    for (const [m, p] of [
      [0, 16],
      [Number.NaN, 16],
      [Number.POSITIVE_INFINITY, 16],
      [-3, 16],
      [1, 0],
      [1, Number.NaN],
      [1, Number.POSITIVE_INFINITY],
      [Number.NaN, Number.NaN],
    ]) {
      const s = dashGridStepMs(m!, p!);
      expect(Number.isFinite(s), `${m} ms/px, ${p} px → ${s}`).toBe(true);
      expect(s, `${m} ms/px, ${p} px`).toBeGreaterThan(0);
    }
  });

  it("AC-SX1VTC.2: each cell starts a dash at its grid time and the last dash before it is cut short", () => {
    // A flat line, 1 ms = 1 px, offset 0.3 px; grid every 100 ms; [12, 4] (16 px): six
    // whole dashes per cell, then a 4 px one cut by the next grid time.
    const ts = sampleTimes(1_000);
    const xs = ts.map((t) => t + 0.3);
    const ys = ts.map(() => 50);
    const pieces: DashPiece[] = [];
    forEachDashPiece(
      xs,
      ys,
      [12, 4],
      (x0, y0, x1, y1) => pieces.push([x0, y0, x1, y1]),
      undefined,
      Infinity,
      { ts, stepMs: 100 },
    );
    for (let g = 100; g < 900; g += 100) {
      const gx = g + 0.3;
      // The cell's dashes, merged across vertices, from gx to the next grid time.
      const spans: [number, number][] = [];
      for (const [x0, , x1] of pieces) {
        if (x0 < gx - EXACT || x1 > gx + 100 + EXACT) continue;
        const prev = spans.at(-1);
        if (prev && Math.abs(prev[1] - x0) < EXACT) prev[1] = x1;
        else spans.push([x0, x1]);
      }
      const want: [number, number][] = [0, 16, 32, 48, 64, 80].map((d) => [gx + d, gx + d + 12]);
      want.push([gx + 96, gx + 100]);
      expect(spans.length, `cell at ${g} ms: ${JSON.stringify(spans)}`).toBe(want.length);
      spans.forEach((s, k) => {
        expect(s[0], `cell ${g} dash ${k} start`).toBeCloseTo(want[k]![0], 6);
        expect(s[1], `cell ${g} dash ${k} end`).toBeCloseTo(want[k]![1], 6);
      });
    }
  });

  it("AC-SX1VTC.2: the restart sits on the point interpolated where the segment crosses the grid time", () => {
    // 100 ms falls halfway along the segment 95 → 105 ms: (40, 20).
    const ts = [0, 95, 105, 200];
    const xs = [0, 30, 50, 90];
    const ys = [0, 10, 30, 30];
    const pieces: DashPiece[] = [];
    forEachDashPiece(
      xs,
      ys,
      [6, 3],
      (x0, y0, x1, y1) => pieces.push([x0, y0, x1, y1]),
      undefined,
      Infinity,
      { ts, stepMs: 100 },
    );
    const at = pieces.findIndex(
      ([x0, y0]) => Math.abs(x0 - 40) < EXACT && Math.abs(y0 - 20) < EXACT,
    );
    expect(at, `pieces: ${JSON.stringify(pieces)}`).toBeGreaterThanOrEqual(0);
    // 6 px along the segment's direction (2, 2)/√8 · … : the dash is 6 px long.
    const [x0, y0, x1, y1] = pieces[at]!;
    expect(Math.hypot(x1 - x0, y1 - y0)).toBeCloseTo(6, 6);
    for (const [a, , b] of pieces) expect(a < 40 - EXACT && b > 40 + EXACT).toBe(false);
  });

  it("AC-SX1VTC.2: inside a cell dashes are laid by screen arc length from the grid time", () => {
    // 37 px flat before the grid time at x = 37, then three 50 px diagonal legs.
    const ts = [-37, 0, 25, 50, 75];
    const xs = [0, 37, 67, 97, 127];
    const ys = [0, 0, 40, 0, 40];
    const pieces: DashPiece[] = [];
    forEachDashPiece(
      xs,
      ys,
      [12, 4],
      (x0, y0, x1, y1) => pieces.push([x0, y0, x1, y1]),
      undefined,
      Infinity,
      { ts, stepMs: 100 },
    );
    // Arc length from the grid point (37, 0) to a point on the zigzag.
    const arcAt = (x: number, y: number): number => {
      let s = 0;
      for (let i = 1; i < xs.length; i++) {
        const ax = xs[i - 1]!;
        const bx = xs[i]!;
        if (bx <= 37) continue;
        const ay = ys[i - 1]!;
        const by = ys[i]!;
        if (x <= bx + EXACT) return s + Math.hypot(x - ax, y - ay);
        s += Math.hypot(bx - ax, by - ay);
      }
      return s;
    };
    const starts: number[] = [];
    let end: [number, number] | null = null;
    for (const [x0, y0, x1, y1] of pieces) {
      // A dash cut short at the grid point may end where the cell's first dash starts.
      const atGrid = Math.abs(x0 - 37) < EXACT && Math.abs(y0) < EXACT;
      const joined =
        !atGrid && end && Math.abs(end[0] - x0) < EXACT && Math.abs(end[1] - y0) < EXACT;
      if (x0 >= 37 - EXACT && !joined) starts.push(arcAt(x0, y0));
      end = [x1, y1];
    }
    // 150 px of zigzag: dashes start every 16 px of arc, from 0.
    expect(starts.length).toBe(10);
    starts.forEach((s, k) => expect(s, `dash ${k} arc start`).toBeCloseTo(16 * k, 6));
  });
});

describe("zoom and y-rescale change the layout only inside a cell (AC-SX1VTC.3)", () => {
  it("AC-SX1VTC.3: after a y-axis rescale every cell still starts at phase 0, and scrolling stays glued", () => {
    const stepMs = 200;
    const scaled = (t: number): number => 100 + 1.7 * (wave(t) - 100);
    for (const y of [wave, scaled]) {
      const opts = { msPerPx: 2.5, widthPx: 400, stepMs, pattern: [12, 4], y };
      const a = drawFrame(2_041.6, opts);
      const ts = sampleTimes(20_000).filter((t) => t >= 2_041.6 && t <= a.t1);
      const xs = ts.map((t) => (t - a.t0) / a.msPerPx);
      expectPhaseZeroAtGrid(a.pieces, gridPoints(ts, xs, ts.map(y), stepMs), "rescaled");
      for (const dt of SHIFTS) expectGlued(a, drawFrame(2_041.6 + dt, opts), 0.5, `Δt ${dt}`);
    }
  });

  it("AC-SX1VTC.3: after a zoom the new step's cells start at phase 0, and scrolling stays glued", () => {
    for (const msPerPx of [2.5, 1.25, 0.4]) {
      const stepMs = dashGridStepMs(msPerPx, 16);
      const opts = { msPerPx, widthPx: 400, stepMs, pattern: [12, 4] };
      const a = drawFrame(4_000.7, opts);
      const ts = sampleTimes(20_000).filter((t) => t >= 4_000.7 && t <= a.t1);
      const xs = ts.map((t) => (t - a.t0) / msPerPx);
      expectPhaseZeroAtGrid(a.pieces, gridPoints(ts, xs, ts.map(wave), stepMs), `${msPerPx} ms/px`);
      for (const dt of [msPerPx * 0.37, msPerPx * 3, 77.7, 401.2])
        expectGlued(a, drawFrame(4_000.7 + dt, opts), 0.5, `${msPerPx} ms/px, Δt ${dt}`);
    }
  });
});
