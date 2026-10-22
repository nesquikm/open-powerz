/**
 * AC-KQF5GM.5: the render-step tests (pan, overview drag, wheel zoom, shuttle) keep the
 * 100 ms budget without CI set and use 300 ms (the budget times a CI factor of 3) when
 * CI is set; every failure message names the budget it used.
 */

import { describe, expect, it } from "vitest";
import {
  CI_FACTOR,
  LIVE_REDRAW_MAX_PER_S,
  LIVE_REDRAW_MIN_PER_S,
  RENDER_STEP_BUDGET_MS,
  ciLatencyBudgetMs,
  ciRateFloor,
  liveRedrawBand,
  renderReport,
  renderStepBudgetMs,
} from "./budget";

describe("renderStepBudgetMs", () => {
  it("is 100 ms without CI set", () => {
    expect(renderStepBudgetMs({})).toBe(100);
  });

  it("treats an empty CI as unset", () => {
    expect(renderStepBudgetMs({ CI: "" })).toBe(100);
  });

  it("is 300 ms when CI is set", () => {
    expect(renderStepBudgetMs({ CI: "true" })).toBe(300);
    expect(renderStepBudgetMs({ CI: "1" })).toBe(300);
  });

  it("is the budget times the CI factor", () => {
    expect(RENDER_STEP_BUDGET_MS).toBe(100);
    expect(CI_FACTOR).toBe(3);
    expect(renderStepBudgetMs({ CI: "true" })).toBe(RENDER_STEP_BUDGET_MS * CI_FACTOR);
  });

  it("reads process.env by default", () => {
    const expected = process.env.CI ? 300 : 100;
    expect(renderStepBudgetMs()).toBe(expected);
  });
});

describe("renderReport", () => {
  it("names the step, its times and the budget it used", () => {
    const msg = renderReport("pan step", [12.34, 216.0], 300);
    expect(msg).toContain("pan step");
    expect(msg).toContain("12.3");
    expect(msg).toContain("216.0");
    expect(msg).toMatch(/budget 300 ms/);
  });

  it("names the local budget too", () => {
    expect(renderReport("shuttle step", [50], 100)).toMatch(/budget 100 ms/);
  });
});

/**
 * AC-KQF5GM.9: the 1000 SPS live redraw-rate test keeps its 20–60 per second band
 * without CI; with CI set only the lower bound is divided by the CI factor of 3.
 */
describe("liveRedrawBand", () => {
  it("is 20–60 per second without CI set", () => {
    expect(LIVE_REDRAW_MIN_PER_S).toBe(20);
    expect(LIVE_REDRAW_MAX_PER_S).toBe(60);
    expect(liveRedrawBand({})).toEqual({ minPerSecond: 20, maxPerSecond: 60 });
  });

  it("treats an empty CI as unset", () => {
    expect(liveRedrawBand({ CI: "" })).toEqual({ minPerSecond: 20, maxPerSecond: 60 });
  });

  it("divides only the lower bound by the CI factor when CI is set", () => {
    const band = liveRedrawBand({ CI: "true" });
    expect(band.minPerSecond).toBeCloseTo(20 / CI_FACTOR, 10);
    expect(band.maxPerSecond).toBe(60);
  });

  it("accepts the 17.9/s a macOS CI runner measured, but not on a dev machine", () => {
    // CI run 39763805516 (macOS chromium) measured 17.9 redraws per second.
    expect(17.9).toBeGreaterThanOrEqual(liveRedrawBand({ CI: "true" }).minPerSecond);
    expect(17.9).toBeLessThan(liveRedrawBand({}).minPerSecond);
  });

  it("reads process.env by default", () => {
    const expected = process.env.CI ? 20 / CI_FACTOR : 20;
    expect(liveRedrawBand().minPerSecond).toBeCloseTo(expected, 10);
    expect(liveRedrawBand().maxPerSecond).toBe(60);
  });
});

describe("ciRateFloor", () => {
  it("keeps a rate floor without CI and divides it by the CI factor with CI", () => {
    expect(ciRateFloor(30, {})).toBe(30);
    expect(ciRateFloor(30, { CI: "" })).toBe(30);
    expect(ciRateFloor(30, { CI: "1" })).toBe(10);
  });
});

/** AC-KQF5GM.11: a kept latency budget is widened by the CI factor on CI only. */
describe("ciLatencyBudgetMs", () => {
  it("keeps a latency budget without CI and multiplies it by the CI factor with CI", () => {
    expect(ciLatencyBudgetMs(500, {})).toBe(500);
    expect(ciLatencyBudgetMs(500, { CI: "" })).toBe(500);
    expect(ciLatencyBudgetMs(500, { CI: "true" })).toBe(500 * CI_FACTOR);
  });

  it("is what the render-step budget uses", () => {
    expect(ciLatencyBudgetMs(RENDER_STEP_BUDGET_MS, { CI: "1" })).toBe(
      renderStepBudgetMs({ CI: "1" }),
    );
    expect(ciLatencyBudgetMs(RENDER_STEP_BUDGET_MS, {})).toBe(renderStepBudgetMs({}));
  });
});
