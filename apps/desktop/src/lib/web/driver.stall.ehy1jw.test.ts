/**
 * FR EHY1JW: the web driver's stall notice no longer flickers on stray samples.
 *
 * Mid-stall the real meter now and then answers one queue poll with samples while
 * another program polls it over CDC `[device]`. The stall used to end on that one poll.
 * Now it ends once polls with samples have run for 1 s of polling, with at most 750 ms
 * of polling between two of them.
 *
 * - AC-EHY1JW.1: a stray poll with samples followed by empty polls does not clear it.
 * - AC-EHY1JW.2: 1 s of polls with samples clears it; a recovered stream at 2, 10, 50 and
 *   1000 SPS clears it within 1.2 s of its first samples.
 * - AC-EHY1JW.3: stray samples further apart than 750 ms of polling never clear it.
 * - AC-EHY1JW.5: the stall still starts after 2 s of unheld empty polls.
 *
 * Over the fake WebUSB meter that answers like the real KM003C, whose queue replies can
 * be switched to the bare `41 id 02 00` the meter sends while the CLI polls it over CDC,
 * with a switch that lets the next queue poll through (a stray reply with samples).
 * Runs on Vitest's fake clock (setTimeout and Date).
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { UiError } from "../backend";
import { MeterDriver, openUsbOrOfferHid, type PowerzWasm } from "./driver";
import { realMeterDevice, type RealMeterDevice } from "./realMeter.test";
import { classify, loadWasm } from "./testWasm.test";
import { settle } from "./webBackendFakes.test";

type DriverOptions = Parameters<typeof MeterDriver.connect>[0];
type Sample = { time_ms: number; vbus_v: number };

let w: PowerzWasm;

beforeEach(async () => {
  w = await loadWasm();
  vi.useFakeTimers({
    toFake: ["setTimeout", "clearTimeout", "setInterval", "clearInterval", "Date"],
  });
  vi.setSystemTime(1_790_000_000_000);
});

afterEach(() => {
  vi.useRealTimers();
});

interface Run {
  dev: RealMeterDevice;
  driver: MeterDriver;
  /** Every batch, with the fake-clock time it arrived. */
  batches: { at: number; samples: Sample[] }[];
  /** Every stall callback, with the fake-clock time it came. */
  stalls: { at: number; on: boolean }[];
  errors: UiError[];
  /** While true, every queue poll gets the bare `41 id 02 00` (no samples). */
  setStalled(on: boolean): void;
  /** Let the next queue poll through to the meter while stalled (a stray reply). */
  strayNext(): void;
}

async function session(rateCode = 2): Promise<Run> {
  const dev = realMeterDevice(w);
  let stalled = false;
  let stray = false;
  const inner = dev.respond;
  dev.respond = (frame) => {
    if (stalled && classify(w, frame) === "get_data:adc_queue") {
      if (stray) {
        stray = false;
        return inner(frame);
      }
      const empty = new Uint8Array([0x41, frame[1] ?? 0, 0x02, 0x00]);
      dev.exchanges.push({ name: "get_data:adc_queue", request: frame.slice(), replies: [empty] });
      return [empty];
    }
    return inner(frame);
  };
  const opened = await settle(
    openUsbOrOfferHid(dev as unknown as Parameters<typeof openUsbOrOfferHid>[0], {
      debug: false,
    }),
  );
  if (!("transport" in opened)) throw new Error("expected the WebUSB transport");
  const batches: Run["batches"] = [];
  const stalls: Run["stalls"] = [];
  const errors: UiError[] = [];
  const driver = await settle(
    MeterDriver.connect({
      wasm: w,
      transport: opened.transport,
      rateCode,
      onBatch: (s: Sample[]) => batches.push({ at: Date.now(), samples: s }),
      onTemp: () => {},
      onError: (e: UiError) => errors.push(e),
      onStall: (on: boolean) => stalls.push({ at: Date.now(), on }),
      random: () => 0x1234_5678_9abc_def0n,
    } as unknown as DriverOptions),
  );
  expect(driver.mode).toBe("queue");
  return {
    dev,
    driver,
    batches,
    stalls,
    errors,
    setStalled: (on) => {
      stalled = on;
    },
    strayNext: () => {
      stray = true;
    },
  };
}

const flags = (run: Run) => run.stalls.map((s) => s.on);
const sampleCount = (run: Run) => run.batches.reduce((n, b) => n + b.samples.length, 0);

/** Stream 1 s, then stall for `ms` of empty queue polls; the stall is on. */
async function stalledRun(rateCode: number, ms = 3_000): Promise<Run> {
  const run = await session(rateCode);
  await vi.advanceTimersByTimeAsync(1_000);
  expect(sampleCount(run)).toBeGreaterThan(0);
  run.setStalled(true);
  await vi.advanceTimersByTimeAsync(ms);
  expect(flags(run)).toEqual([true]);
  return run;
}

/** Hand over one stray reply with samples every `gapMs`, `count` times. */
async function strays(run: Run, gapMs: number, count: number): Promise<void> {
  for (let i = 0; i < count; i++) {
    const before = sampleCount(run);
    run.strayNext();
    await vi.advanceTimersByTimeAsync(gapMs);
    // The stray poll really delivered samples.
    expect(sampleCount(run)).toBeGreaterThan(before);
  }
}

describe("a stray poll with samples mid-stall (AC-EHY1JW.1)", () => {
  it("ehy1jw: one stray reply followed by empty polls keeps the notice", async () => {
    const run = await stalledRun(2);
    await strays(run, 1_500, 1);
    expect(flags(run)).toEqual([true]);
    // The stall goes on: still the one notice, never cleared, never sent again.
    await vi.advanceTimersByTimeAsync(4_000);
    expect(flags(run)).toEqual([true]);
    expect(run.errors).toEqual([]);
    await settle(run.driver.disconnect());
  });

  it("ehy1jw: three strays in a 60 s stall never flicker the notice", async () => {
    const run = await stalledRun(2);
    for (let i = 0; i < 3; i++) {
      await strays(run, 100, 1);
      await vi.advanceTimersByTimeAsync(3_000);
    }
    await vi.advanceTimersByTimeAsync(50_000);
    expect(flags(run)).toEqual([true]);
    await settle(run.driver.disconnect());
  });
});

describe("1 s of polls with samples clears the notice (AC-EHY1JW.2)", () => {
  it("ehy1jw: strays 600 ms apart clear it once they span 1 s, not before", async () => {
    const run = await stalledRun(2);
    // Strays at about 0, 600 ms: the run spans 600 ms, not enough.
    await strays(run, 600, 2);
    expect(flags(run)).toEqual([true]);
    // The third, about 1.2 s after the first: the run spans 1 s.
    await strays(run, 600, 1);
    expect(flags(run)).toEqual([true, false]);
    // Back to empty: no new notice before another 2 s of empty polls.
    await vi.advanceTimersByTimeAsync(1_000);
    expect(flags(run)).toEqual([true, false]);
    await settle(run.driver.disconnect());
  });

  const rates: [number, string][] = [
    [0, "2 SPS"],
    [1, "10 SPS"],
    [2, "50 SPS"],
    [3, "1000 SPS"],
  ];
  for (const [rateCode, label] of rates) {
    it(`ehy1jw: a recovered ${label} stream clears it 1 to 1.2 s after its first samples`, async () => {
      const run = await stalledRun(rateCode, 8_000);
      const batchesBefore = run.batches.length;
      run.setStalled(false);
      await vi.advanceTimersByTimeAsync(3_000);
      const first = run.batches[batchesBefore];
      expect(first).toBeDefined();
      expect(flags(run)).toEqual([true, false]);
      const cleared = run.stalls[1]?.at ?? Number.NaN;
      const after = cleared - (first?.at ?? Number.NaN);
      // Not on the first samples: the polls with samples ran about 1 s first.
      expect(after).toBeGreaterThanOrEqual(900);
      expect(after).toBeLessThanOrEqual(1_200);
      // The stream keeps going: nothing more to say.
      await vi.advanceTimersByTimeAsync(5_000);
      expect(flags(run)).toEqual([true, false]);
      expect(run.errors).toEqual([]);
      await settle(run.driver.disconnect());
    });
  }
});

describe("strays further apart than 750 ms start over (AC-EHY1JW.3)", () => {
  it("ehy1jw: single strays 1.6 s apart never clear the notice", async () => {
    const run = await stalledRun(2);
    await strays(run, 1_600, 8);
    expect(flags(run)).toEqual([true]);
    await settle(run.driver.disconnect());
  });

  it("ehy1jw: strays 900 ms apart never span 1 s and never clear it", async () => {
    const run = await stalledRun(2);
    await strays(run, 900, 10);
    expect(flags(run)).toEqual([true]);
    await settle(run.driver.disconnect());
  });

  it("ehy1jw: a gap over 750 ms restarts the run", async () => {
    const run = await stalledRun(2);
    // 600 ms of run, then a 1.6 s gap, then 600 ms more: never 1 s in one run.
    await strays(run, 600, 1);
    await strays(run, 1_600, 1);
    await strays(run, 600, 1);
    await strays(run, 1_600, 1);
    expect(flags(run)).toEqual([true]);
    await settle(run.driver.disconnect());
  });
});

describe("the stall still starts after 2 s of empty polls (AC-EHY1JW.5)", () => {
  it("ehy1jw: the notice comes 2 s into the empty polls, not before", async () => {
    const run = await session(2);
    await vi.advanceTimersByTimeAsync(1_000);
    const began = Date.now();
    run.setStalled(true);
    await vi.advanceTimersByTimeAsync(1_800);
    expect(flags(run)).toEqual([]);
    await vi.advanceTimersByTimeAsync(1_000);
    expect(flags(run)).toEqual([true]);
    const at = (run.stalls[0]?.at ?? Number.NaN) - began;
    expect(at).toBeGreaterThanOrEqual(2_000);
    expect(at).toBeLessThanOrEqual(2_300);
    await settle(run.driver.disconnect());
  });

  it("ehy1jw: strays that do not clear the notice do not send it again", async () => {
    const run = await stalledRun(2);
    // Each stray restarts the empty run, and the 2 s of empty polls after it would
    // start a stall: the stall is still on, so nothing is sent.
    for (let i = 0; i < 3; i++) {
      await strays(run, 100, 1);
      await vi.advanceTimersByTimeAsync(2_500);
    }
    expect(flags(run)).toEqual([true]);
    await settle(run.driver.disconnect());
  });

  it("ehy1jw: after a recovery, 2 s of empty polls start a new stall, once", async () => {
    const run = await stalledRun(2);
    run.setStalled(false);
    await vi.advanceTimersByTimeAsync(2_000);
    expect(flags(run)).toEqual([true, false]);
    run.setStalled(true);
    await vi.advanceTimersByTimeAsync(1_800);
    expect(flags(run)).toEqual([true, false]);
    await vi.advanceTimersByTimeAsync(3_000);
    expect(flags(run)).toEqual([true, false, true]);
    await settle(run.driver.disconnect());
  });
});
