/**
 * FR AXHK2T (AC-AXHK2T.1, .3): a chart's ruler. One `range_stats` call in flight, the
 * newest span wins; while an answer is on its way the readout says so rather than show
 * the old values next to the new delta time; the screen reader hears the values once
 * they settle (500 ms after the last move, or at once on a key or pointer release), not
 * on every step.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { RangeStats } from "./backend";
import { RULER_PENDING } from "./ruler";
import { RulerState } from "./rulerState.svelte";

function stats(chargeAh: number): RangeStats {
  return {
    samples: 10,
    charge_ah: chargeAh,
    energy_wh: chargeAh * 5,
    vbus_avg_v: 5,
    ibus_avg_a: 1,
    power_avg_w: 5,
  };
}

function deferred() {
  const calls: { span: [number, number]; resolve: (s: RangeStats | null) => void }[] = [];
  const fetch = vi.fn(
    (t0: number, t1: number) =>
      new Promise<RangeStats | null>((resolve) => calls.push({ span: [t0, t1], resolve })),
  );
  return { fetch, calls };
}

const flush = () => vi.advanceTimersByTimeAsync(0);

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

describe("RulerState (AC-AXHK2T.1, .3)", () => {
  it("shows a pending value until the answer for the cursors as they are now", async () => {
    const { fetch, calls } = deferred();
    const ruler = new RulerState(fetch);
    ruler.set({ t1Ms: 0, t2Ms: 1_000 });
    calls[0]!.resolve(stats(1));
    await flush();
    expect(ruler.readout[1]!.text).toBe("1.000000 Ah");

    ruler.set({ t1Ms: 0, t2Ms: 2_000 });
    expect(ruler.readout[0]!.text).toBe("2.000 s");
    expect(ruler.readout[1]!.text).toBe(RULER_PENDING);
    // A move while that answer is on its way: still pending after it lands.
    ruler.set({ t1Ms: 0, t2Ms: 3_000 });
    calls[1]!.resolve(stats(2));
    await flush();
    expect(ruler.readout[1]!.text).toBe(RULER_PENDING);
    expect(calls[2]!.span).toEqual([0, 3_000]);
    calls[2]!.resolve(stats(3));
    await flush();
    expect(ruler.readout[1]!.text).toBe("3.000000 Ah");
  });

  it("announces once the values settle, not on every step", async () => {
    const { fetch, calls } = deferred();
    const ruler = new RulerState(fetch);
    ruler.set({ t1Ms: 0, t2Ms: 1_000 });
    calls[0]!.resolve(stats(1));
    await flush();
    await vi.advanceTimersByTimeAsync(600);
    expect(ruler.announcement).toContain("Delta charge 1.000000 Ah");
    const first = ruler.announcement;

    for (let k = 2; k <= 6; k++) {
      ruler.set({ t1Ms: 0, t2Ms: k * 1_000 });
      await vi.advanceTimersByTimeAsync(100);
      calls.at(-1)!.resolve(stats(k));
      await flush();
    }
    // Moving every 100 ms: nothing new is said yet.
    expect(ruler.announcement).toBe(first);
    await vi.advanceTimersByTimeAsync(500);
    expect(ruler.announcement).toContain("Delta charge 6.000000 Ah");
    expect(ruler.announcement).toContain("Delta time 6.000 s");
  });

  it("a key or pointer release announces as soon as the answer is in", async () => {
    const { fetch, calls } = deferred();
    const ruler = new RulerState(fetch);
    ruler.set({ t1Ms: 0, t2Ms: 1_000 });
    ruler.settle();
    expect(ruler.announcement).toBe("");
    calls[0]!.resolve(stats(1));
    await flush();
    expect(ruler.announcement).toContain("Delta charge 1.000000 Ah");
  });

  it("reset puts both cursors back in the view", () => {
    const { fetch } = deferred();
    const ruler = new RulerState(fetch);
    ruler.set({ t1Ms: 0, t2Ms: 50 });
    ruler.reset(3_000, 6_000);
    expect(ruler.ruler).toEqual({ t1Ms: 4_000, t2Ms: 5_000 });
    expect(fetch).toHaveBeenCalledTimes(1);
  });

  it("clear forgets the cursors, the values and what was to be said", async () => {
    const { fetch, calls } = deferred();
    const ruler = new RulerState(fetch);
    ruler.set({ t1Ms: 0, t2Ms: 1_000 });
    ruler.clear();
    calls[0]!.resolve(stats(1));
    await vi.advanceTimersByTimeAsync(1_000);
    expect(ruler.ruler).toBeNull();
    expect(ruler.stats).toBeNull();
    expect(ruler.announcement).toBe("");
  });
});
