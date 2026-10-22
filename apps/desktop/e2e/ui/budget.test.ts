/**
 * AC-KQF5GM.5: the render-step tests (pan, overview drag, wheel zoom, shuttle) keep the
 * 100 ms budget without CI set and use 300 ms (the budget times a CI factor of 3) when
 * CI is set; every failure message names the budget it used.
 */

import { describe, expect, it } from "vitest";
import { CI_FACTOR, RENDER_STEP_BUDGET_MS, renderReport, renderStepBudgetMs } from "./budget";

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
