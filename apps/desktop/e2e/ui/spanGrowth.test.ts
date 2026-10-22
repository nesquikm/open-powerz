/**
 * AC-KQF5GM.8: the overview-strip drag and click tests bound how much the view's span
 * may grow by the real time that passed between the draw they read and the pan, not by
 * a fixed 500 ms. The live window widens by at most one session ms per real ms, so the
 * allowance is that elapsed time plus a small fixed slack for rounding.
 */

import { describe, expect, it } from "vitest";
import { SPAN_GROWTH_SLACK_MS, maxSpanGrowthMs } from "./spanGrowth";

describe("maxSpanGrowthMs", () => {
  it("is the elapsed real time plus the slack", () => {
    expect(maxSpanGrowthMs(1_000, 1_250)).toBe(250 + SPAN_GROWTH_SLACK_MS);
    expect(maxSpanGrowthMs(10_000.5, 12_000.5)).toBe(2_000 + SPAN_GROWTH_SLACK_MS);
  });

  it("keeps the slack small next to the old fixed 500 ms bound", () => {
    expect(SPAN_GROWTH_SLACK_MS).toBeGreaterThan(0);
    expect(SPAN_GROWTH_SLACK_MS).toBeLessThanOrEqual(100);
  });

  it("is only the slack when no time passed", () => {
    expect(maxSpanGrowthMs(5_000, 5_000)).toBe(SPAN_GROWTH_SLACK_MS);
  });

  it("covers the 640-820 ms growth a slow WebKit runner showed after about 0.9 s", () => {
    // CI run 39750985754: the view grew by up to 820 ms because ~0.8-0.9 s passed.
    expect(maxSpanGrowthMs(0, 820)).toBeGreaterThan(820);
    expect(maxSpanGrowthMs(0, 900)).toBeGreaterThan(820);
  });

  it("grows with the elapsed time instead of capping at a fixed bound", () => {
    expect(maxSpanGrowthMs(0, 3_000)).toBeGreaterThan(3_000);
    expect(maxSpanGrowthMs(0, 3_000) - maxSpanGrowthMs(0, 1_000)).toBe(2_000);
  });

  it("still catches a span that grew by more than the time that passed", () => {
    // 200 ms passed: a 1 s growth is a real bug (the pan did not keep the span).
    expect(maxSpanGrowthMs(0, 200)).toBeLessThan(1_000);
  });

  it("rejects a pan read before the draw, or a non-finite time", () => {
    expect(() => maxSpanGrowthMs(2_000, 1_000)).toThrow(/before/);
    expect(() => maxSpanGrowthMs(Number.NaN, 1_000)).toThrow();
    expect(() => maxSpanGrowthMs(0, Number.POSITIVE_INFINITY)).toThrow();
  });
});
