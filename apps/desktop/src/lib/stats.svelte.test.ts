import { describe, expect, it, vi } from "vitest";
import type { UiError } from "./backend";
import { StatsControls } from "./stats.svelte";

const refused: UiError = { kind: "usage", message: "No meter is connected.", hint: null };

function deps() {
  return {
    resetStats: vi.fn(async () => {}),
    clearHistory: vi.fn(async () => {}),
  };
}

describe("StatsControls reset (AC-AH2AEJ.5, AC-AH2AEJ.6)", () => {
  it("asks the backend to reset the statistics", async () => {
    const c = new StatsControls();
    const d = deps();
    await c.reset(d);
    expect(d.resetStats).toHaveBeenCalledTimes(1);
    expect(d.clearHistory).not.toHaveBeenCalled();
    expect(c.error).toBeNull();
  });

  it("shows a refused reset as a plain-language error", async () => {
    const c = new StatsControls();
    const d = deps();
    d.resetStats.mockRejectedValueOnce(refused);
    await c.reset(d);
    expect(c.error).toEqual(refused);
  });
});

describe("StatsControls clear history needs a confirmation (AC-AH2AEJ.5)", () => {
  it("never clears without a request", async () => {
    const c = new StatsControls();
    const d = deps();
    await c.confirmClear(d);
    expect(d.clearHistory).not.toHaveBeenCalled();
  });

  it("asks first, then clears once on confirmation", async () => {
    const c = new StatsControls();
    const d = deps();
    expect(c.clearPending).toBe(false);
    c.requestClear();
    expect(c.clearPending).toBe(true);
    expect(d.clearHistory).not.toHaveBeenCalled();
    await c.confirmClear(d);
    expect(d.clearHistory).toHaveBeenCalledTimes(1);
    expect(d.resetStats).not.toHaveBeenCalled();
    expect(c.clearPending).toBe(false);
    // The confirmation is used up.
    await c.confirmClear(d);
    expect(d.clearHistory).toHaveBeenCalledTimes(1);
  });

  it("cancelling drops the request", async () => {
    const c = new StatsControls();
    const d = deps();
    c.requestClear();
    c.cancelClear();
    expect(c.clearPending).toBe(false);
    await c.confirmClear(d);
    expect(d.clearHistory).not.toHaveBeenCalled();
  });

  it("shows a failed clear as an error and leaves the confirmation closed", async () => {
    const c = new StatsControls();
    const d = deps();
    d.clearHistory.mockRejectedValueOnce(refused);
    c.requestClear();
    await c.confirmClear(d);
    expect(c.error).toEqual(refused);
    expect(c.clearPending).toBe(false);
  });
});

describe("StatsControls edges (review round 1)", () => {
  it("reports whether the reset went through, so the panel can blank old values", async () => {
    const c = new StatsControls();
    const d = deps();
    await expect(c.reset(d)).resolves.toBe(true);
    d.resetStats.mockRejectedValueOnce(refused);
    await expect(c.reset(d)).resolves.toBe(false);
  });
});

describe("StatsControls clears the chart after the backend clear (AC-5X154V.2)", () => {
  function depsWithChart() {
    const calls: string[] = [];
    let finishBackend: () => void = () => {};
    const d = {
      resetStats: vi.fn(async () => {}),
      clearHistory: vi.fn(
        () =>
          new Promise<void>((resolve) => {
            calls.push("backend:start");
            finishBackend = () => {
              calls.push("backend:done");
              resolve();
            };
          }),
      ),
      clearChart: vi.fn(() => {
        calls.push("chart");
      }),
    };
    return { d, calls, finishBackend: () => finishBackend() };
  }

  it("empties the chart once, only after the backend clear succeeded", async () => {
    const c = new StatsControls();
    const { d, calls, finishBackend } = depsWithChart();
    c.requestClear();
    const done = c.confirmClear(d);
    // The backend has not answered yet, so the chart still shows its data.
    await Promise.resolve();
    expect(d.clearChart).not.toHaveBeenCalled();
    finishBackend();
    await done;
    expect(d.clearChart).toHaveBeenCalledTimes(1);
    expect(calls).toEqual(["backend:start", "backend:done", "chart"]);
    expect(c.error).toBeNull();
  });

  it("leaves the chart as it is when the backend refuses the clear", async () => {
    const c = new StatsControls();
    const d = { ...deps(), clearChart: vi.fn() };
    d.clearHistory.mockRejectedValueOnce(refused);
    c.requestClear();
    await c.confirmClear(d);
    expect(d.clearChart).not.toHaveBeenCalled();
    expect(c.error).toEqual(refused);
  });

  it("never empties the chart without a confirmed request", async () => {
    const c = new StatsControls();
    const d = { ...deps(), clearChart: vi.fn() };
    await c.confirmClear(d);
    c.requestClear();
    c.cancelClear();
    await c.confirmClear(d);
    expect(d.clearChart).not.toHaveBeenCalled();
  });

  it("a stats reset never touches the chart", async () => {
    const c = new StatsControls();
    const d = { ...deps(), clearChart: vi.fn() };
    await c.reset(d);
    expect(d.clearChart).not.toHaveBeenCalled();
  });
});
