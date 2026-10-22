import { describe, expect, it } from "vitest";
import type { GapSpan } from "./chartData";
import { formatDuration } from "./chartSeries";
import {
  BREAK_PX,
  COLLAPSE_MIN_GAP_MS,
  MAX_BREAK_PX,
  MIN_SPAN_MS,
  SHUTTLE_SPEEDS,
  buildWarp,
  dragOverview,
  followPlayhead,
  minSpanMs,
  overviewClick,
  overviewWindow,
  presetRange,
  shuttlePress,
  shuttleStep,
  sliderToSpan,
  spanToSlider,
  stepPlayhead,
  stripBreaks,
  zoomInDisplay,
  type TimeRange,
  type TimeWarp,
} from "./timeline";

/** 20 s of data, a 40-minute outage, then 20 s more (the `?mock=outage` shape). */
const GAP_FROM = 20_000;
const GAP_TO = GAP_FROM + 2_400_000;
const END = GAP_TO + 20_000;
const OUTAGE: GapSpan[] = [{ from_ms: GAP_FROM, to_ms: GAP_TO }];
const WIDTH = 1000;

/** On-screen pixels between two session times for a view drawn across `widthPx`. */
function px(w: TimeWarp, view: TimeRange, widthPx: number, a: number, b: number): number {
  const span = w.fwd(view.t1Ms) - w.fwd(view.t0Ms);
  return ((w.fwd(b) - w.fwd(a)) / span) * widthPx;
}

describe("buildWarp: collapsed long gaps (AC-FENCD9.11)", () => {
  it("is the identity with no gap longer than a minute", () => {
    expect(COLLAPSE_MIN_GAP_MS).toBe(60_000);
    const short: GapSpan[] = [{ from_ms: 10_000, to_ms: 40_000 }];
    for (const gaps of [[], short]) {
      const w = buildWarp(gaps, { t0Ms: 0, t1Ms: 100_000 }, WIDTH);
      expect(w.breaks).toEqual([]);
      for (const t of [0, 12_345, 25_000, 99_999]) {
        expect(w.fwd(t)).toBe(t);
        expect(w.bwd(t)).toBe(t);
      }
    }
  });

  it("squeezes a 40-minute gap to BREAK_PX (never more than MAX_BREAK_PX) when fully in view", () => {
    expect(BREAK_PX).toBe(24);
    expect(MAX_BREAK_PX).toBe(32);
    const view = { t0Ms: 0, t1Ms: END };
    const w = buildWarp(OUTAGE, view, WIDTH);
    expect(w.breaks).toEqual([{ fromMs: GAP_FROM, toMs: GAP_TO }]);
    const breakPx = px(w, view, WIDTH, GAP_FROM, GAP_TO);
    expect(breakPx).toBeLessThanOrEqual(MAX_BREAK_PX);
    expect(breakPx).toBeCloseTo(BREAK_PX, 0);
    // The 40 s of data fill the rest of the plot.
    expect(px(w, view, WIDTH, 0, GAP_FROM) + px(w, view, WIDTH, GAP_TO, END)).toBeCloseTo(
      WIDTH - BREAK_PX,
      0,
    );
  });

  it("keeps a narrow break on a narrow plot too", () => {
    const view = { t0Ms: 0, t1Ms: END };
    for (const width of [200, 600, 3000]) {
      const w = buildWarp(OUTAGE, view, width);
      expect(px(w, view, width, GAP_FROM, GAP_TO)).toBeLessThanOrEqual(MAX_BREAK_PX);
      expect(px(w, view, width, GAP_FROM, GAP_TO)).toBeGreaterThan(0);
    }
  });

  it("maps time monotonically", () => {
    const w = buildWarp(OUTAGE, { t0Ms: 0, t1Ms: END }, WIDTH);
    let prev = -Infinity;
    for (let t = 0; t <= END; t += 7_919) {
      const d = w.fwd(t);
      expect(d).toBeGreaterThanOrEqual(prev);
      prev = d;
    }
    expect(w.fwd(END)).toBeGreaterThan(w.fwd(0));
  });

  it("inverts outside the gap: bwd(fwd(t)) ≈ t on both sides", () => {
    const w = buildWarp(OUTAGE, { t0Ms: 0, t1Ms: END }, WIDTH);
    for (const t of [0, 1_234, 19_999, GAP_TO + 1, GAP_TO + 10_000, END]) {
      expect(w.bwd(w.fwd(t))).toBeCloseTo(t, 3);
    }
  });

  it("keeps times on both sides true: one second of data spans the same display time anywhere", () => {
    const w = buildWarp(OUTAGE, { t0Ms: 0, t1Ms: END }, WIDTH);
    expect(w.fwd(6_000) - w.fwd(5_000)).toBeCloseTo(1_000, 6);
    expect(w.fwd(GAP_TO + 6_000) - w.fwd(GAP_TO + 5_000)).toBeCloseTo(1_000, 6);
    // The data after the gap starts right after the break, not 40 minutes later.
    expect(w.fwd(GAP_TO) - w.fwd(GAP_FROM)).toBeLessThan(2_400_000 / 10);
  });

  it("squeezes a gap only partly in view in proportion to the part in view", () => {
    // Half the gap in view: its visible half gets about half a break.
    const view = { t0Ms: 10_000, t1Ms: GAP_FROM + 1_200_000 };
    const w = buildWarp(OUTAGE, view, WIDTH);
    expect(w.breaks).toHaveLength(1);
    const visiblePx = px(w, view, WIDTH, GAP_FROM, view.t1Ms);
    expect(visiblePx).toBeLessThan(BREAK_PX);
    expect(Math.abs(visiblePx - BREAK_PX / 2)).toBeLessThanOrEqual(1.5);
  });

  it("changes the mapping smoothly while panning across a gap", () => {
    const span = 30_000 + 2_400_000;
    let prevPx: number | null = null;
    for (let t0 = 0; t0 <= 15_000; t0 += 500) {
      const view = { t0Ms: t0, t1Ms: t0 + span };
      const w = buildWarp(OUTAGE, view, WIDTH);
      const at = px(w, view, WIDTH, t0, GAP_FROM);
      if (prevPx !== null) expect(Math.abs(at - prevPx)).toBeLessThan(WIDTH / 10);
      prevPx = at;
    }
  });

  it("neither marks nor budgets a long gap shorter than its would-be break", () => {
    // A 10-hour view: one break is worth ~14 minutes of display, so a 61 s gap stays
    // at its true width and only the 40-minute outage collapses.
    const short = { from_ms: 3_600_000, to_ms: 3_661_000 };
    const outage = { from_ms: 7_200_000, to_ms: 9_600_000 };
    const view = { t0Ms: 0, t1Ms: 36_000_000 };
    const w = buildWarp([short, outage], view, WIDTH);
    expect(w.breaks).toEqual([{ fromMs: outage.from_ms, toMs: outage.to_ms }]);
    // The outage gets the whole break budget: BREAK_PX, not more.
    expect(px(w, view, WIDTH, outage.from_ms, outage.to_ms)).toBeCloseTo(BREAK_PX, 1);
    // The short gap keeps its true time.
    expect(w.fwd(short.to_ms) - w.fwd(short.from_ms)).toBeCloseTo(61_000, 6);
  });

  it("maps many breaks in logarithmic time per point, the same as a linear walk", () => {
    // 5000 two-minute gaps, one minute of data between them.
    const gaps: GapSpan[] = [];
    for (let i = 0; i < 5_000; i++) {
      const from = i * 180_000 + 60_000;
      gaps.push({ from_ms: from, to_ms: from + 120_000 });
    }
    const view = { t0Ms: 0, t1Ms: 5_000 * 180_000 };
    const w = buildWarp(gaps, view, WIDTH);
    expect(w.breaks).toHaveLength(5_000);
    // Each gap is squeezed by the same factor: data before gap i is i minutes plus i
    // squeezed gaps.
    const squeezed = w.fwd(gaps[0]!.to_ms) - w.fwd(gaps[0]!.from_ms);
    for (const i of [0, 1, 2_500, 4_999]) {
      const g = gaps[i]!;
      expect(w.fwd(g.from_ms)).toBeCloseTo(60_000 * (i + 1) + squeezed * i, 3);
      expect(w.bwd(w.fwd(g.from_ms - 1_000))).toBeCloseTo(g.from_ms - 1_000, 3);
      expect(w.bwd(w.fwd(g.to_ms + 1_000))).toBeCloseTo(g.to_ms + 1_000, 3);
    }
    // uPlot maps every point through fwd / bwd: 200 000 calls stay fast.
    const n = 100_000;
    const t0 = performance.now();
    let sink = 0;
    for (let k = 0; k < n; k++) {
      const t = (k / n) * view.t1Ms;
      sink += w.fwd(t) + w.bwd(t);
    }
    expect(Number.isFinite(sink)).toBe(true);
    expect(performance.now() - t0).toBeLessThan(250);
  });
});

describe("J/K/L shuttle (AC-FENCD9.7)", () => {
  it("L plays forwards at 1×, 2×, 4×, 8× on repeated presses and stays at 8×", () => {
    expect([...SHUTTLE_SPEEDS]).toEqual([1, 2, 4, 8]);
    expect(shuttlePress(0, "L")).toBe(1);
    expect(shuttlePress(1, "L")).toBe(2);
    expect(shuttlePress(2, "L")).toBe(4);
    expect(shuttlePress(4, "L")).toBe(8);
    expect(shuttlePress(8, "L")).toBe(8);
  });

  it("J mirrors L backwards", () => {
    expect(shuttlePress(0, "J")).toBe(-1);
    expect(shuttlePress(-1, "J")).toBe(-2);
    expect(shuttlePress(-2, "J")).toBe(-4);
    expect(shuttlePress(-4, "J")).toBe(-8);
    expect(shuttlePress(-8, "J")).toBe(-8);
  });

  it("a press in the other direction reverses at 1×", () => {
    expect(shuttlePress(-4, "L")).toBe(1);
    expect(shuttlePress(8, "J")).toBe(-1);
  });

  it("K stops", () => {
    for (const s of [-8, -1, 0, 2, 8]) expect(shuttlePress(s, "K")).toBe(0);
  });

  it("moves the playhead by speed × elapsed time", () => {
    const session = { firstMs: 0, lastMs: 10_000 };
    const mid = { reachedEnd: false, reachedStart: false };
    expect(shuttleStep(1_000, 2, 100, session)).toEqual({ playheadMs: 1_200, ...mid });
    expect(shuttleStep(1_000, -4, 100, session)).toEqual({ playheadMs: 600, ...mid });
    expect(shuttleStep(1_000, 0, 100, session)).toEqual({ playheadMs: 1_000, ...mid });
  });

  it("stops at the session start, and reaching the newest data forwards says so", () => {
    const session = { firstMs: 0, lastMs: 10_000 };
    expect(shuttleStep(100, -8, 100, session)).toEqual({
      playheadMs: 0,
      reachedEnd: false,
      reachedStart: true,
    });
    expect(shuttleStep(9_950, 1, 100, session)).toEqual({
      playheadMs: 10_000,
      reachedEnd: true,
      reachedStart: false,
    });
    expect(shuttleStep(9_000, 8, 1_000, session)).toEqual({
      playheadMs: 10_000,
      reachedEnd: true,
      reachedStart: false,
    });
  });

  it("says so when moving backwards reaches the session start, never forwards", () => {
    const session = { firstMs: 5_000, lastMs: 10_000 };
    expect(shuttleStep(5_000, -1, 16, session).reachedStart).toBe(true);
    expect(shuttleStep(5_000, -1, 0, session).reachedStart).toBe(true);
    expect(shuttleStep(5_100, -1, 16, session).reachedStart).toBe(false);
    expect(shuttleStep(5_000, 1, 0, session).reachedStart).toBe(false);
  });
});

describe("arrow keys step the playhead (AC-FENCD9.6)", () => {
  const view = { t0Ms: 10_000, t1Ms: 20_000 };
  const session = { firstMs: 0, lastMs: 30_000 };

  it("moves one pixel's worth of time, or a tenth of the view with Shift", () => {
    // 10 s across 1000 px: 10 ms a pixel.
    expect(stepPlayhead(15_000, 1, view, 1000, false, session)).toBeCloseTo(15_010, 6);
    expect(stepPlayhead(15_000, -1, view, 1000, false, session)).toBeCloseTo(14_990, 6);
    expect(stepPlayhead(15_000, 1, view, 1000, true, session)).toBeCloseTo(16_000, 6);
    expect(stepPlayhead(15_000, -1, view, 1000, true, session)).toBeCloseTo(14_000, 6);
  });

  it("starts a missing playhead at the view centre", () => {
    const r = stepPlayhead(null, 1, view, 1000, false, session);
    expect(Math.abs(r - 15_000)).toBeLessThanOrEqual(10);
    const c = stepPlayhead(null, -1, view, 1000, true, session);
    expect(Math.abs(c - 15_000)).toBeLessThanOrEqual(1_000);
  });

  it("stays inside the session", () => {
    expect(stepPlayhead(5, -1, { t0Ms: 0, t1Ms: 1_000 }, 100, true, session)).toBe(0);
    expect(stepPlayhead(29_999, 1, { t0Ms: 20_000, t1Ms: 30_000 }, 100, true, session)).toBe(
      30_000,
    );
  });

  it("the view follows when the playhead would leave it, by the least amount", () => {
    expect(followPlayhead(view, 15_000, session)).toEqual(view);
    expect(followPlayhead(view, 21_000, session)).toEqual({ t0Ms: 11_000, t1Ms: 21_000 });
    expect(followPlayhead(view, 8_000, session)).toEqual({ t0Ms: 8_000, t1Ms: 18_000 });
    // Never past the session.
    expect(followPlayhead({ t0Ms: 25_000, t1Ms: 35_000 }, 34_000, session)).toEqual({
      t0Ms: 20_000,
      t1Ms: 30_000,
    });
  });
});

describe("zoom slider scale (AC-FENCD9.9)", () => {
  const max = 3_600_000;

  it("runs from 10 ms to the whole session", () => {
    expect(MIN_SPAN_MS).toBe(10);
    expect(spanToSlider(10, max)).toBeCloseTo(0, 9);
    expect(spanToSlider(max, max)).toBeCloseTo(1, 9);
    expect(sliderToSpan(0, max)).toBeCloseTo(10, 6);
    expect(sliderToSpan(1, max)).toBeCloseTo(max, 3);
  });

  it("is logarithmic: the geometric middle sits at the slider's middle", () => {
    expect(spanToSlider(Math.sqrt(10 * max), max)).toBeCloseTo(0.5, 6);
    expect(sliderToSpan(0.5, max)).toBeCloseTo(Math.sqrt(10 * max), 3);
  });

  it("round-trips a span", () => {
    for (const s of [10, 100, 5_000, 60_000, 1_000_000, max]) {
      expect(sliderToSpan(spanToSlider(s, max), max) / s).toBeCloseTo(1, 6);
    }
  });

  it("clamps out-of-range spans and values", () => {
    expect(spanToSlider(1, max)).toBe(0);
    expect(spanToSlider(2 * max, max)).toBe(1);
    expect(sliderToSpan(-1, max)).toBeCloseTo(10, 6);
    expect(sliderToSpan(2, max)).toBeCloseTo(max, 3);
  });
});

describe("paused presets (AC-FENCD9.10)", () => {
  const session = { firstMs: 0, lastMs: 100_000 };

  it("centres the preset span on the anchor", () => {
    expect(presetRange(10_000, 50_000, session)).toEqual({ t0Ms: 45_000, t1Ms: 55_000 });
  });

  it("Whole is the session", () => {
    expect(presetRange(null, 50_000, session)).toEqual({ t0Ms: 0, t1Ms: 100_000 });
  });

  it("stays inside the session, keeping the span", () => {
    expect(presetRange(10_000, 2_000, session)).toEqual({ t0Ms: 0, t1Ms: 10_000 });
    expect(presetRange(10_000, 99_000, session)).toEqual({ t0Ms: 90_000, t1Ms: 100_000 });
    expect(presetRange(600_000, 50_000, session)).toEqual({ t0Ms: 0, t1Ms: 100_000 });
  });
});

describe("overview strip window (AC-FENCD9.1 / AC-FENCD9.2)", () => {
  const session = { firstMs: 0, lastMs: 100_000 };
  const whole = { t0Ms: 0, t1Ms: 100_000 };
  const flat = buildWarp([], whole, WIDTH); // 100 ms a pixel
  const start = { t0Ms: 20_000, t1Ms: 30_000 };

  it("places the window over the view's range", () => {
    expect(overviewWindow(start, session, WIDTH, flat)).toEqual({ leftPx: 200, widthPx: 100 });
    expect(overviewWindow(whole, session, WIDTH, flat)).toEqual({ leftPx: 0, widthPx: WIDTH });
  });

  it("uses the collapsed-gap map: the last 10 s after an outage is a wide window at the right", () => {
    const s = { firstMs: 0, lastMs: END };
    const w = buildWarp(OUTAGE, { t0Ms: 0, t1Ms: END }, WIDTH);
    const box = overviewWindow({ t0Ms: END - 10_000, t1Ms: END }, s, WIDTH, w);
    expect(box.leftPx + box.widthPx).toBeCloseTo(WIDTH, 0);
    // 10 s of the 40 s of data in the 976 px left beside the break.
    expect(box.widthPx).toBeCloseTo((WIDTH - BREAK_PX) / 4, -1);
  });

  it("dragging the window pans, keeping the span", () => {
    expect(dragOverview(start, "move", 100, session, WIDTH, flat)).toEqual({
      t0Ms: 30_000,
      t1Ms: 40_000,
    });
    expect(dragOverview(start, "move", -50, session, WIDTH, flat)).toEqual({
      t0Ms: 15_000,
      t1Ms: 25_000,
    });
  });

  it("a pan stops at either end of the session", () => {
    expect(dragOverview(start, "move", -500, session, WIDTH, flat)).toEqual({
      t0Ms: 0,
      t1Ms: 10_000,
    });
    expect(dragOverview(start, "move", 1_000, session, WIDTH, flat)).toEqual({
      t0Ms: 90_000,
      t1Ms: 100_000,
    });
  });

  it("dragging an edge resizes the range from that side", () => {
    expect(dragOverview(start, "left", -50, session, WIDTH, flat)).toEqual({
      t0Ms: 15_000,
      t1Ms: 30_000,
    });
    expect(dragOverview(start, "right", 50, session, WIDTH, flat)).toEqual({
      t0Ms: 20_000,
      t1Ms: 35_000,
    });
    expect(dragOverview(start, "left", -1_000, session, WIDTH, flat)).toEqual({
      t0Ms: 0,
      t1Ms: 30_000,
    });
    expect(dragOverview(start, "right", 1_000, session, WIDTH, flat)).toEqual({
      t0Ms: 20_000,
      t1Ms: 100_000,
    });
  });

  it("an edge dragged past the other never makes the range narrower than 10 ms", () => {
    const l = dragOverview(start, "left", 300, session, WIDTH, flat);
    expect(l.t1Ms).toBe(30_000);
    expect(l.t1Ms - l.t0Ms).toBeGreaterThanOrEqual(MIN_SPAN_MS);
    const r = dragOverview(start, "right", -300, session, WIDTH, flat);
    expect(r.t0Ms).toBe(20_000);
    expect(r.t1Ms - r.t0Ms).toBeGreaterThanOrEqual(MIN_SPAN_MS);
  });

  it("a click outside the window centres the same span on the clicked time", () => {
    expect(overviewClick(700, start, session, WIDTH, flat)).toEqual({
      t0Ms: 65_000,
      t1Ms: 75_000,
    });
    expect(overviewClick(995, start, session, WIDTH, flat)).toEqual({
      t0Ms: 90_000,
      t1Ms: 100_000,
    });
  });
});

describe("zoom on the chart as drawn (AC-FENCD9.14)", () => {
  const SESSION = { firstMs: 0, lastMs: END };
  const WHOLE: TimeRange = { t0Ms: 0, t1Ms: END };

  /** Display time of a range under `w`. */
  function shown(w: TimeWarp, r: TimeRange): number {
    return w.fwd(r.t1Ms) - w.fwd(r.t0Ms);
  }
  /** Data time of a range (the outage cut out). */
  function dataMs(r: TimeRange): number {
    const cut = Math.max(0, Math.min(r.t1Ms, GAP_TO) - Math.max(r.t0Ms, GAP_FROM));
    return r.t1Ms - r.t0Ms - cut;
  }

  it("keeps the anchor at its display position and scales the displayed time by the factor", () => {
    const w = buildWarp(OUTAGE, WHOLE, WIDTH);
    for (const [atMs, factor] of [
      [END - 2_000, 0.8],
      [10_000, 0.8],
      [GAP_FROM + 1_200_000, 0.5],
      [END - 5_000, 0.64],
    ] as const) {
      const next = zoomInDisplay(WHOLE, atMs, factor, w, SESSION);
      const before = shown(w, WHOLE);
      const after = shown(w, next);
      expect(after / before).toBeCloseTo(factor, 6);
      const fracBefore = (w.fwd(atMs) - w.fwd(WHOLE.t0Ms)) / before;
      const fracAfter = (w.fwd(atMs) - w.fwd(next.t0Ms)) / after;
      expect(fracAfter).toBeCloseTo(fracBefore, 6);
      expect(next.t0Ms).toBeGreaterThanOrEqual(SESSION.firstMs);
      expect(next.t1Ms).toBeLessThanOrEqual(SESSION.lastMs);
    }
  });

  it("zooming in near the end keeps the data on screen instead of an empty gap", () => {
    // Two + presses around a playhead near the end of the whole session.
    let view = WHOLE;
    const at = END - 1_000;
    for (let i = 0; i < 2; i++) {
      const w = buildWarp(OUTAGE, view, WIDTH);
      view = zoomInDisplay(view, at, 0.8, w, SESSION);
    }
    expect(dataMs(view) / dataMs(WHOLE)).toBeGreaterThan(0.64 * 0.85);
    expect(dataMs(view) / dataMs(WHOLE)).toBeLessThan(0.64 * 1.15);
    // The break that is left still takes at most MAX_BREAK_PX of the plot.
    const w = buildWarp(OUTAGE, view, WIDTH);
    for (const b of w.breaks) {
      const a = Math.max(b.fromMs, view.t0Ms);
      const z = Math.min(b.toMs, view.t1Ms);
      expect(px(w, view, WIDTH, a, z)).toBeLessThanOrEqual(MAX_BREAK_PX);
    }
  });

  it("zooming out is clamped to the whole session", () => {
    const view: TimeRange = { t0Ms: END - 10_000, t1Ms: END - 2_000 };
    const w = buildWarp(OUTAGE, view, WIDTH);
    const next = zoomInDisplay(view, END - 3_000, 1_000, w, SESSION);
    expect(next.t0Ms).toBeCloseTo(SESSION.firstMs, 3);
    expect(next.t1Ms).toBeCloseTo(SESSION.lastMs, 3);
    // A partial zoom out past the end shifts back inside, keeping its displayed span.
    const out = zoomInDisplay(view, END - 2_500, 1.25, w, SESSION);
    expect(out.t1Ms).toBeLessThanOrEqual(SESSION.lastMs);
    expect(out.t0Ms).toBeGreaterThanOrEqual(SESSION.firstMs);
    expect(shown(w, out) / shown(w, view)).toBeCloseTo(1.25, 6);
  });

  it("is the plain session-time zoom with no gap to collapse", () => {
    const session = { firstMs: 0, lastMs: 100_000 };
    const view: TimeRange = { t0Ms: 20_000, t1Ms: 60_000 };
    const w = buildWarp([], view, WIDTH);
    const next = zoomInDisplay(view, 30_000, 0.8, w, session);
    // 40 s → 32 s, the anchor a quarter of the way in.
    expect(next.t1Ms - next.t0Ms).toBeCloseTo(32_000, 6);
    expect((30_000 - next.t0Ms) / (next.t1Ms - next.t0Ms)).toBeCloseTo(0.25, 6);
  });

  it("never zooms in below MIN_SPAN_MS", () => {
    const view: TimeRange = { t0Ms: 1_000, t1Ms: 1_012 };
    const w = buildWarp([], view, WIDTH);
    const next = zoomInDisplay(view, 1_006, 0.1, w, { firstMs: 0, lastMs: 100_000 });
    expect(next.t1Ms - next.t0Ms).toBeCloseTo(MIN_SPAN_MS, 6);
  });
});

describe("zoomInDisplay edge cases (AC-FENCD9.14)", () => {
  const session = { firstMs: 0, lastMs: 100_000 };
  const view = { t0Ms: 40_000, t1Ms: 60_000 };
  const warp = buildWarp([], view, 1000);

  it("leaves the view as it is for a non-finite factor or anchor", () => {
    expect(zoomInDisplay(view, 50_000, Number.NaN, warp, session)).toEqual(view);
    expect(zoomInDisplay(view, Number.NaN, 0.8, warp, session)).toEqual(view);
    expect(zoomInDisplay(view, 50_000, Number.POSITIVE_INFINITY, warp, session)).toEqual(view);
  });

  it("lands exactly on the session end when the zoom reaches it", () => {
    const atEnd = { t0Ms: 90_000, t1Ms: 100_000 };
    const out = zoomInDisplay(atEnd, 99_000, 1.3, buildWarp([], atEnd, 1000), session);
    expect(out.t1Ms).toBe(session.lastMs);
    expect(out.t1Ms - out.t0Ms).toBeCloseTo(13_000, 6);
    const atStart = { t0Ms: 0, t1Ms: 10_000 };
    const back = zoomInDisplay(atStart, 1_000, 1.3, buildWarp([], atStart, 1000), session);
    expect(back.t0Ms).toBe(session.firstMs);
  });
});

describe("narrowest view follows the rate (AC-9CFMD3.4)", () => {
  const ID: TimeWarp = { fwd: (ms) => ms, bwd: (d) => d, breaks: [] };
  const LONG = { firstMs: 0, lastMs: 600_000 };

  it("is two sample periods at the current rate, never under the 10 ms floor", () => {
    expect(minSpanMs(50)).toBe(40);
    expect(minSpanMs(10)).toBe(200);
    expect(minSpanMs(2)).toBe(1000);
    // 2 ms at 1000 SPS is under the floor.
    expect(minSpanMs(1000)).toBe(MIN_SPAN_MS);
    expect(MIN_SPAN_MS).toBe(10);
  });

  it("falls back to the 10 ms floor while the rate is unknown", () => {
    expect(minSpanMs(null)).toBe(MIN_SPAN_MS);
    expect(minSpanMs(0)).toBe(MIN_SPAN_MS);
    expect(minSpanMs(Number.NaN)).toBe(MIN_SPAN_MS);
  });

  it("puts the zoom slider's bottom end at the rate's narrowest view", () => {
    expect(sliderToSpan(0, 600_000, minSpanMs(50))).toBeCloseTo(40, 6);
    expect(sliderToSpan(1, 600_000, minSpanMs(50))).toBeCloseTo(600_000, 3);
    expect(spanToSlider(40, 600_000, minSpanMs(50))).toBeCloseTo(0, 6);
    expect(spanToSlider(10, 600_000, minSpanMs(50))).toBe(0);
  });

  it("stops zooming in at two sample periods", () => {
    let view: TimeRange = { t0Ms: 100_000, t1Ms: 110_000 };
    const session = { ...LONG, minSpanMs: minSpanMs(50) };
    for (let k = 0; k < 200; k++) view = zoomInDisplay(view, 105_000, 0.5, ID, session);
    expect(view.t1Ms - view.t0Ms).toBeCloseTo(40, 6);
  });
});

describe("stripBreaks (AC-CWWKJ6.1)", () => {
  // 0..500 s: a 10 s blip at 20 s (shorter than the collapse threshold) and a
  // 5-minute outage at 100 s (collapsed).
  const BLIP: GapSpan = { from_ms: 20_000, to_ms: 30_000 };
  const LONG: GapSpan = { from_ms: 100_000, to_ms: 400_000 };
  const session = { firstMs: 0, lastMs: 500_000 };
  const WIDTH = 1000;
  const warpOf = (gaps: GapSpan[]): TimeWarp =>
    buildWarp(gaps, { t0Ms: session.firstMs, t1Ms: session.lastMs }, WIDTH);
  /** x of `ms` on the strip through `w`, CSS px. */
  const xOf = (w: TimeWarp, ms: number): number =>
    ((w.fwd(ms) - w.fwd(session.firstMs)) / (w.fwd(session.lastMs) - w.fwd(session.firstMs))) *
    WIDTH;

  it("marks a gap shorter than the collapse threshold as an uncollapsed band with its length", () => {
    expect(BLIP.to_ms - BLIP.from_ms).toBeLessThan(COLLAPSE_MIN_GAP_MS);
    const w = warpOf([BLIP]);
    const breaks = stripBreaks([BLIP], w, session, WIDTH);
    expect(breaks).toHaveLength(1);
    const [b] = breaks;
    expect(b!.collapsed).toBe(false);
    expect(b!.label).toBe(formatDuration(10_000));
    // Its true share of the strip: 10 s of 500 s across 1000 px.
    expect(b!.leftPx).toBeCloseTo(40, 6);
    expect(b!.widthPx).toBeCloseTo(20, 6);
  });

  it("keeps collapsed gaps as collapsed breaks next to the short ones, in time order", () => {
    const w = warpOf([BLIP, LONG]);
    const breaks = stripBreaks([LONG, BLIP], w, session, WIDTH);
    expect(breaks.map((b) => b.collapsed)).toEqual([false, true]);
    const [short, long] = breaks;
    expect(short!.leftPx).toBeCloseTo(xOf(w, BLIP.from_ms), 6);
    expect(short!.widthPx).toBeCloseTo(xOf(w, BLIP.to_ms) - xOf(w, BLIP.from_ms), 6);
    expect(short!.label).toBe(formatDuration(10_000));
    expect(long!.leftPx).toBeCloseTo(xOf(w, LONG.from_ms), 6);
    expect(long!.widthPx).toBeGreaterThan(0);
    expect(long!.widthPx).toBeLessThanOrEqual(MAX_BREAK_PX + 1e-9);
    expect(long!.label).toBe(formatDuration(300_000));
  });

  it("marks exactly the collapsed breaks the warp has, collapsed", () => {
    const w = warpOf([BLIP, LONG]);
    const collapsed = stripBreaks([BLIP, LONG], w, session, WIDTH).filter((b) => b.collapsed);
    expect(collapsed).toHaveLength(w.breaks.length);
  });

  it("leaves out gaps outside the session and draws nothing without gaps", () => {
    const w = warpOf([]);
    expect(stripBreaks([], w, session, WIDTH)).toEqual([]);
    const outside: GapSpan = { from_ms: 600_000, to_ms: 610_000 };
    expect(stripBreaks([outside], w, session, WIDTH)).toEqual([]);
  });

  it("draws nothing on an empty session", () => {
    const empty = { firstMs: 1000, lastMs: 1000 };
    const w = buildWarp([BLIP], { t0Ms: 1000, t1Ms: 1000 }, WIDTH);
    expect(stripBreaks([BLIP], w, empty, WIDTH)).toEqual([]);
  });
});
