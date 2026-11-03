/**
 * FR AXHK2W, AC .5: the overview strip reuses its envelope buffers. On a steady stream
 * (the strip's width unchanged) no frame makes a new typed array.
 *
 * API pinned here (`./stripEnvelope`):
 *
 * - `new StripEnvelope(series)`: the per-column min / max buffers of `series` series.
 * - `StripEnvelope.reset(cols)`: `{ lo, hi }`, one `Float64Array` of length `cols` per
 *   series, every column empty (`lo = +Infinity`, `hi = -Infinity`). The same arrays
 *   come back while `cols` stays the same; a new width gets new ones.
 *
 * `OverviewStrip.svelte` paints through it: its source makes no typed array of its own.
 */

import { readFileSync } from "node:fs";
import { afterEach, describe, expect, it } from "vitest";
import { StripEnvelope, bridgeColumns } from "./stripEnvelope";

const RealFloat64Array = globalThis.Float64Array;
let made = 0;

/** Counts every `new Float64Array(...)` until restored. */
function countFloat64Arrays(): void {
  made = 0;
  globalThis.Float64Array = new Proxy(RealFloat64Array, {
    construct(target, args, newTarget) {
      made++;
      return Reflect.construct(target, args, newTarget);
    },
  });
}

afterEach(() => {
  globalThis.Float64Array = RealFloat64Array;
});

/** One frame of the strip's paint: reset the buffers, fold samples in, bridge. */
function frame(
  env: StripEnvelope,
  cols: number,
  seed: number,
): { lo: Float64Array[]; hi: Float64Array[] } {
  const { lo, hi } = env.reset(cols);
  for (let s = 0; s < lo.length; s++) {
    for (let c = 0; c < cols; c += 3) {
      const v = Math.sin(c + seed + s);
      if (v < lo[s]![c]!) lo[s]![c] = v;
      if (v > hi[s]![c]!) hi[s]![c] = v;
    }
    bridgeColumns(lo[s]!, hi[s]!, () => false);
  }
  return { lo: [...lo], hi: [...hi] };
}

describe("the strip's envelope buffers (AC-AXHK2W.5)", () => {
  it("AC-AXHK2W.5: reset gives one empty buffer pair per series, `cols` long", () => {
    const env = new StripEnvelope(2);
    const { lo, hi } = env.reset(5);
    expect(lo).toHaveLength(2);
    expect(hi).toHaveLength(2);
    for (let s = 0; s < 2; s++) {
      expect(lo[s]).toBeInstanceOf(Float64Array);
      expect([...lo[s]!]).toEqual(Array(5).fill(Infinity));
      expect([...hi[s]!]).toEqual(Array(5).fill(-Infinity));
    }
  });

  it("AC-AXHK2W.5: a steady stream makes no new typed array per frame", () => {
    const env = new StripEnvelope(2);
    const first = frame(env, 640, 0);
    countFloat64Arrays();
    let last = first;
    for (let f = 1; f <= 120; f++) last = frame(env, 640, f);
    globalThis.Float64Array = RealFloat64Array;
    expect(made).toBe(0);
    expect(last.lo[0]).toBe(first.lo[0]);
    expect(last.lo[1]).toBe(first.lo[1]);
    expect(last.hi[0]).toBe(first.hi[0]);
    expect(last.hi[1]).toBe(first.hi[1]);
  });

  it("AC-AXHK2W.5: a reused buffer starts each frame empty", () => {
    const env = new StripEnvelope(1);
    const { lo, hi } = env.reset(4);
    lo[0]!.fill(1);
    hi[0]!.fill(2);
    const again = env.reset(4);
    expect(again.lo[0]).toBe(lo[0]);
    expect([...again.lo[0]!]).toEqual([Infinity, Infinity, Infinity, Infinity]);
    expect([...again.hi[0]!]).toEqual([-Infinity, -Infinity, -Infinity, -Infinity]);
  });

  it("AC-AXHK2W.5: a new width gets buffers of the new length", () => {
    const env = new StripEnvelope(2);
    env.reset(10);
    const wider = env.reset(12);
    expect(wider.lo[0]).toHaveLength(12);
    expect(wider.hi[1]).toHaveLength(12);
    const narrower = env.reset(7);
    expect(narrower.lo[1]).toHaveLength(7);
    expect([...narrower.hi[0]!]).toEqual(Array(7).fill(-Infinity));
  });

  it("AC-AXHK2W.5: the overview strip makes no typed array itself, it uses the envelope", () => {
    const source = readFileSync(
      new URL("./components/OverviewStrip.svelte", import.meta.url),
      "utf8",
    );
    expect(source).not.toMatch(/new\s+Float(32|64)Array\s*\(/);
    expect(source).toMatch(/\bStripEnvelope\b/);
  });
});
