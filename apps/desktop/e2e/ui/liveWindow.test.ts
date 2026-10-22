/**
 * AC-KQF5GM.10: the reload backfill test checks state, not timing. Each live window
 * starts at the session start, or, once the session is longer than the window, the
 * window is full and ends at the newest sample. So the expected start is
 * max(session start, newest - window length) and the span is min(session length,
 * window), whatever time the runner took to get there.
 */

import { describe, expect, it } from "vitest";
import { LIVE_START_TOLERANCE_MS, liveWindowProblem } from "./liveWindow";

const TEN_S = 10_000;

describe("liveWindowProblem", () => {
  it("accepts a young session drawn from its start", () => {
    expect(liveWindowProblem({ t0Ms: 20, t1Ms: 6_000, windowMs: TEN_S, reachBackMs: 6_000 })).toBe(
      null,
    );
  });

  it("rejects a young session whose window starts after the session start", () => {
    const problem = liveWindowProblem({
      t0Ms: 1_500,
      t1Ms: 6_000,
      windowMs: TEN_S,
      reachBackMs: 4_000,
    });
    expect(problem).toMatch(/starts at 1500 ms/);
    expect(problem).toMatch(/expected 0 ms/);
  });

  it("accepts a full window ending at the newest sample once the session is longer", () => {
    // CI run 39763805516 (Windows WebKit): Elapsed 0:00:12, "Live: 10 s", the window
    // started at 1.4-1.9 s. That is correct behaviour, not a missing backfill.
    expect(
      liveWindowProblem({ t0Ms: 1_900, t1Ms: 11_900, windowMs: TEN_S, reachBackMs: 6_000 }),
    ).toBe(null);
    expect(
      liveWindowProblem({ t0Ms: 1_400, t1Ms: 11_400, windowMs: TEN_S, reachBackMs: 6_000 }),
    ).toBe(null);
  });

  it("rejects a window that is not full once the session is longer than it", () => {
    const problem = liveWindowProblem({
      t0Ms: 4_000,
      t1Ms: 12_000,
      windowMs: TEN_S,
      reachBackMs: 6_000,
    });
    expect(problem).toMatch(/starts at 4000 ms/);
    expect(problem).toMatch(/expected 2000 ms/);
  });

  it("rejects a window wider than its length", () => {
    expect(
      liveWindowProblem({ t0Ms: 0, t1Ms: 12_000, windowMs: TEN_S, reachBackMs: 6_000 }),
    ).toMatch(/expected 2000 ms/);
  });

  it("allows one backfill step of tolerance around the expected start", () => {
    expect(LIVE_START_TOLERANCE_MS).toBe(250);
    expect(liveWindowProblem({ t0Ms: 250, t1Ms: 6_000, windowMs: TEN_S, reachBackMs: 5_000 })).toBe(
      null,
    );
    expect(
      liveWindowProblem({ t0Ms: 251, t1Ms: 6_000, windowMs: TEN_S, reachBackMs: 5_000 }),
    ).not.toBe(null);
    expect(
      liveWindowProblem({ t0Ms: 2_200, t1Ms: 12_000, windowMs: TEN_S, reachBackMs: 6_000 }),
    ).toBe(null);
  });

  it("rejects a window that does not reach back before the reload", () => {
    // 6 s had passed before the reload, but only 3 s of data is drawn: no backfill.
    const problem = liveWindowProblem({
      t0Ms: 20,
      t1Ms: 3_000,
      windowMs: TEN_S,
      reachBackMs: 6_000,
    });
    expect(problem).toMatch(/spans 2980 ms/);
    expect(problem).toMatch(/6000 ms/);
  });

  it("caps the reach-back at the window length", () => {
    // 11 s before the reload, but a 10 s window can only hold 10 s.
    expect(
      liveWindowProblem({ t0Ms: 2_000, t1Ms: 12_000, windowMs: TEN_S, reachBackMs: 11_000 }),
    ).toBe(null);
  });

  it("works for the 1 min and 10 min windows", () => {
    expect(
      liveWindowProblem({ t0Ms: 10, t1Ms: 12_000, windowMs: 60_000, reachBackMs: 6_000 }),
    ).toBe(null);
    expect(
      liveWindowProblem({ t0Ms: 2_000, t1Ms: 12_000, windowMs: 600_000, reachBackMs: 6_000 }),
    ).toMatch(/expected 0 ms/);
  });

  it("measures the start from a given session start", () => {
    expect(
      liveWindowProblem({
        t0Ms: 5_000,
        t1Ms: 9_000,
        windowMs: TEN_S,
        reachBackMs: 4_000,
        sessionStartMs: 5_000,
      }),
    ).toBe(null);
  });

  it("rejects an inverted or non-finite range", () => {
    expect(
      liveWindowProblem({ t0Ms: 6_000, t1Ms: 5_000, windowMs: TEN_S, reachBackMs: 0 }),
    ).toMatch(/ends before it starts/);
    expect(
      liveWindowProblem({ t0Ms: Number.NaN, t1Ms: 5_000, windowMs: TEN_S, reachBackMs: 0 }),
    ).toMatch(/non-finite/);
  });
});
