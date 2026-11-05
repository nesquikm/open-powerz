/**
 * FR AXHK2W, AC .2: the page's `PdStatusBuffer` keeps at most 100,000 status samples.
 * The oldest go first and are counted for a notice, and time ranges stay right across
 * the drop.
 *
 * API pinned here (`./pdTimeline`):
 *
 * - `PD_STATUS_MAX_SAMPLES`: samples a buffer keeps by default (100,000, as many as the
 *   backend's status log and `PD_MAX_ROWS`).
 * - `new PdStatusBuffer({ maxSamples })`: a buffer with a smaller cap (for tests).
 * - `PdStatusBuffer.dropped`: samples dropped since the last clear.
 * - `pdStatusDroppedText(dropped)`: the notice, or null when nothing was dropped.
 */

import { describe, expect, it } from "vitest";
import type { PdStatusSample } from "./backend";
import { PD_MAX_ROWS } from "./pdStore";
import { PD_STATUS_MAX_SAMPLES, PdStatusBuffer, pdStatusDroppedText } from "./pdTimeline";

function sample(ms: number): PdStatusSample {
  return { session_ms: ms, vbus_v: ms / 1000, ibus_a: -ms / 1000, cc1_v: 1.6, cc2_v: 0 };
}

function samples(from: number, n: number): PdStatusSample[] {
  return Array.from({ length: n }, (_, i) => sample(from + i));
}

describe("the status buffer's cap (AC-AXHK2W.2)", () => {
  it("AC-AXHK2W.2: keeps 100,000 samples by default, as many as the PD table", () => {
    expect(PD_STATUS_MAX_SAMPLES).toBe(100_000);
    expect(PD_STATUS_MAX_SAMPLES).toBe(PD_MAX_ROWS);
    const buf = new PdStatusBuffer();
    buf.append(samples(0, 100_000));
    expect(buf.length).toBe(100_000);
    expect(buf.dropped).toBe(0);
    buf.append(samples(100_000, 5));
    expect(buf.length).toBe(100_000);
    expect(buf.dropped).toBe(5);
    const all = buf.range(0, Infinity);
    expect(all.t[0]).toBe(5);
    expect(all.t.at(-1)).toBe(100_004);
  });

  it("AC-AXHK2W.2: drops the oldest first, batch after batch, and counts them", () => {
    const buf = new PdStatusBuffer({ maxSamples: 10 });
    buf.append(samples(0, 6));
    buf.append(samples(6, 6));
    expect(buf.length).toBe(10);
    expect(buf.dropped).toBe(2);
    buf.append(samples(12, 25));
    expect(buf.length).toBe(10);
    expect(buf.dropped).toBe(27);
    expect(buf.range(0, Infinity).t).toEqual(Array.from({ length: 10 }, (_, i) => 27 + i));
  });

  it("AC-AXHK2W.2: ranges read the right samples across the drop", () => {
    const buf = new PdStatusBuffer({ maxSamples: 8 });
    // Several drops, so the kept samples no longer start where the first ones did.
    for (let from = 0; from < 30; from += 3) buf.append(samples(from, 3));
    // Kept: 22..29.
    expect(buf.length).toBe(8);
    expect(buf.range(0, 21)).toEqual({ t: [], vbus: [], ibus: [], cc1: [], cc2: [] });
    const mid = buf.range(21, 25);
    expect(mid.t).toEqual([22, 23, 24, 25]);
    expect(mid.vbus).toEqual([0.022, 0.023, 0.024, 0.025]);
    expect(mid.ibus).toEqual([-0.022, -0.023, -0.024, -0.025]);
    expect(mid.cc1).toEqual([1.6, 1.6, 1.6, 1.6]);
    expect(buf.range(27, 100).t).toEqual([27, 28, 29]);
    expect(buf.range(26.5, 26.9).t).toEqual([]);
  });

  it("AC-AXHK2W.2: a clear empties the buffer and resets the count", () => {
    const buf = new PdStatusBuffer({ maxSamples: 4 });
    buf.append(samples(0, 9));
    expect(buf.dropped).toBe(5);
    buf.clear();
    expect(buf.length).toBe(0);
    expect(buf.dropped).toBe(0);
    buf.append(samples(100, 3));
    expect(buf.range(0, Infinity).t).toEqual([100, 101, 102]);
  });

  it("AC-AXHK2W.2: the notice says how many oldest samples went, and of what cap", () => {
    expect(pdStatusDroppedText(0)).toBeNull();
    const one = pdStatusDroppedText(1);
    expect(one).toMatch(/\b1\b/);
    expect(one).toContain("100,000");
    const many = pdStatusDroppedText(12_345);
    expect(many).toContain("12,345");
    expect(many).toMatch(/oldest/i);
    expect(many).toContain("100,000");
  });
});
