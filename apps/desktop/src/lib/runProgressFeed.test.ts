/**
 * FR AXHK2T (AC-AXHK2T.4, .5): segment marks and E.D. come from the backend's
 * `run_progress` over the raw samples, never from the drawn buckets. `RunProgressFeed`
 * asks for them: one request in flight, the newest want wins, at most one refresh per
 * `minIntervalMs` while only the data or the window moves (an option change asks at
 * once), never per draw for an unchanged want, and the drawn arrays are refilled in
 * place (WebKit keeps dropped arrays, AC-4NSVC9.2).
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { RunProgress, RunProgressQuery } from "./backend";
import { RunProgressFeed, type ProgressWant } from "./runProgressFeed";

function answer(over: Partial<RunProgress> = {}): RunProgress {
  return {
    session_t0_ms: 0,
    run_t0_ms: 0,
    run_t1_ms: 1_000,
    samples: 1001,
    charge_ah: 1,
    energy_wh: 5,
    marks_total_positive: true,
    marks: [
      { t_ms: 100, percent: 10 },
      { t_ms: 200, percent: 20 },
    ],
    ed_total_wh: 5,
    energy_total_positive: true,
    ed_t_ms: [0, 500, 1_000],
    ed_pct: [0, 50, 100],
    ...over,
  };
}

const want = (over: Partial<ProgressWant> = {}): ProgressWant => ({
  marks: "charge",
  step: 10,
  ed: true,
  edPoints: 3,
  edT0Ms: 0,
  edT1Ms: 1_000,
  stamp: 1,
  ...over,
});

/** A fetch whose answers the test hands out one by one. */
function deferredFetch() {
  const calls: { q: RunProgressQuery; resolve: (a: RunProgress | null) => void }[] = [];
  const fetch = vi.fn(
    (q: RunProgressQuery) =>
      new Promise<RunProgress | null>((resolve) => calls.push({ q, resolve })),
  );
  return { fetch, calls };
}

let now = 0;
beforeEach(() => {
  vi.useFakeTimers();
  now = 0;
});
afterEach(() => {
  vi.useRealTimers();
});

const flush = () => vi.advanceTimersByTimeAsync(0);

describe("RunProgressFeed (AC-AXHK2T.4, .5)", () => {
  it("maps the marks to labelled segment marks and the E.D. answer to points", async () => {
    const { fetch, calls } = deferredFetch();
    const onAnswer = vi.fn();
    const feed = new RunProgressFeed(fetch, onAnswer, { now: () => now });
    feed.want(want());
    expect(fetch).toHaveBeenCalledTimes(1);
    expect(calls[0]!.q).toEqual({
      kind: "charge",
      step_pct: 10,
      ed_points: 3,
      ed_t0_ms: 0,
      ed_t1_ms: 1_000,
      ed_scope: "session",
    });
    calls[0]!.resolve(answer());
    await flush();
    expect(onAnswer).toHaveBeenCalledTimes(1);
    expect(feed.marks).toEqual([
      { tMs: 100, percent: 10, label: "10%" },
      { tMs: 200, percent: 20, label: "20%" },
    ]);
    expect(feed.ed).toEqual({ tMs: [0, 500, 1_000], pct: [0, 50, 100] });
    expect(feed.marksTotalPositive).toBe(true);
    expect(feed.energyTotalPositive).toBe(true);
    expect(feed.extent).toEqual({ t0Ms: 0, t1Ms: 1_000 });
  });

  it("refills the same arrays in place on every answer", async () => {
    const { fetch, calls } = deferredFetch();
    const feed = new RunProgressFeed(fetch, () => {}, { now: () => now });
    const { marks, ed } = feed;
    const { tMs, pct } = ed;
    feed.want(want());
    calls[0]!.resolve(answer());
    await flush();
    now += 300;
    feed.want(want({ stamp: 2 }));
    calls[1]!.resolve(answer({ marks: [{ t_ms: 150, percent: 10 }], ed_t_ms: [1], ed_pct: [7] }));
    await flush();
    expect(feed.marks).toBe(marks);
    expect(feed.ed).toBe(ed);
    expect(feed.ed.tMs).toBe(tMs);
    expect(feed.ed.pct).toBe(pct);
    expect(feed.marks).toEqual([{ tMs: 150, percent: 10, label: "10%" }]);
    expect(feed.ed).toEqual({ tMs: [1], pct: [7] });
  });

  it("an unchanged want asks nothing more, however often it is drawn", async () => {
    const { fetch, calls } = deferredFetch();
    const feed = new RunProgressFeed(fetch, () => {}, { now: () => now });
    feed.want(want());
    calls[0]!.resolve(answer());
    await flush();
    for (let k = 0; k < 50; k++) {
      now += 16;
      feed.want(want());
    }
    await vi.advanceTimersByTimeAsync(2_000);
    expect(fetch).toHaveBeenCalledTimes(1);
  });

  it("new data or a moved window asks at most every 250 ms, the last want always sent", async () => {
    const { fetch, calls } = deferredFetch();
    const feed = new RunProgressFeed(fetch, () => {}, { now: () => now });
    feed.want(want({ stamp: 0 }));
    calls[0]!.resolve(answer());
    await flush();
    // Sixty frames a second of live data for one second.
    for (let k = 1; k <= 60; k++) {
      now += 1000 / 60;
      vi.advanceTimersByTime(1000 / 60);
      feed.want(want({ stamp: k, edT0Ms: k, edT1Ms: 1_000 + k }));
      await flush();
      calls.at(-1)!.resolve(answer());
      await flush();
    }
    expect(fetch.mock.calls.length).toBeGreaterThanOrEqual(4);
    expect(fetch.mock.calls.length).toBeLessThanOrEqual(5);
    // The last want reaches the backend once the gate opens, with no further draw.
    now += 300;
    await vi.advanceTimersByTimeAsync(300);
    expect(calls.at(-1)!.q.ed_t0_ms).toBe(60);
  });

  it("keeps one request in flight; the newest want goes next and an older answer for other options is dropped", async () => {
    const { fetch, calls } = deferredFetch();
    const feed = new RunProgressFeed(fetch, () => {}, { now: () => now });
    feed.want(want({ marks: "charge" }));
    feed.want(want({ marks: "energy" }));
    feed.want(want({ marks: "energy", step: 25 }));
    expect(fetch).toHaveBeenCalledTimes(1);
    calls[0]!.resolve(answer());
    await flush();
    // The charge answer is not for what is wanted now: nothing drawn from it.
    expect(feed.marks).toEqual([]);
    expect(fetch).toHaveBeenCalledTimes(2);
    expect(calls[1]!.q).toMatchObject({ kind: "energy", step_pct: 25 });
    calls[1]!.resolve(answer({ marks: [{ t_ms: 250, percent: 25 }] }));
    await flush();
    expect(feed.marks).toEqual([{ tMs: 250, percent: 25, label: "25%" }]);
  });

  it("an option change asks at once, without waiting for the gate", async () => {
    const { fetch, calls } = deferredFetch();
    const feed = new RunProgressFeed(fetch, () => {}, { now: () => now });
    feed.want(want());
    calls[0]!.resolve(answer());
    await flush();
    now += 10;
    feed.want(want({ step: 20 }));
    expect(fetch).toHaveBeenCalledTimes(2);
  });

  it("nothing wanted asks nothing and draws nothing; marks off empties the marks only", async () => {
    const { fetch, calls } = deferredFetch();
    const feed = new RunProgressFeed(fetch, () => {}, { now: () => now });
    feed.want(want());
    calls[0]!.resolve(answer());
    await flush();
    feed.want(want({ marks: "off" }));
    expect(feed.marks).toEqual([]);
    expect(feed.marksTotalPositive).toBeNull();
    expect(feed.ed.tMs).toHaveLength(3);
    feed.want(null);
    expect(feed.ed).toEqual({ tMs: [], pct: [] });
    expect(feed.energyTotalPositive).toBeNull();
    expect(fetch).toHaveBeenCalledTimes(2);
  });

  it("says when a total is not positive", async () => {
    const { fetch, calls } = deferredFetch();
    const feed = new RunProgressFeed(fetch, () => {}, { now: () => now });
    feed.want(want());
    calls[0]!.resolve(
      answer({
        marks_total_positive: false,
        marks: [],
        energy_total_positive: false,
        ed_t_ms: [],
        ed_pct: [],
      }),
    );
    await flush();
    expect(feed.marksTotalPositive).toBe(false);
    expect(feed.energyTotalPositive).toBe(false);
  });

  it("clear drops an answer still on its way", async () => {
    const { fetch, calls } = deferredFetch();
    const feed = new RunProgressFeed(fetch, () => {}, { now: () => now });
    feed.want(want());
    feed.clear();
    calls[0]!.resolve(answer());
    await flush();
    expect(feed.marks).toEqual([]);
    expect(feed.extent).toBeNull();
  });

  it("a failed request draws nothing and lets the next want through", async () => {
    const fetch = vi
      .fn<(q: RunProgressQuery) => Promise<RunProgress | null>>()
      .mockRejectedValueOnce(new Error("gone"))
      .mockResolvedValue(answer());
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const feed = new RunProgressFeed(fetch, () => {}, { now: () => now });
    feed.want(want());
    await flush();
    expect(feed.marks).toEqual([]);
    now += 300;
    feed.want(want({ stamp: 2 }));
    await flush();
    expect(feed.marks).toHaveLength(2);
    warn.mockRestore();
  });
});
