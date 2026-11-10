/**
 * FR NVN6M8: the web driver tells when the meter stops filling the ADC queue (another
 * program polls it over CDC) and when samples come back.
 *
 * - AC-NVN6M8.1: queue polls that return no samples for 2 s of polling call `onStall(true)`.
 * - AC-NVN6M8.2: the first poll with samples calls `onStall(false)`; one call per stall.
 * - AC-NVN6M8.3: time with the polling held (a hold, a settings write holding the link)
 *   does not count toward the 2 s; empty runs shorter than 2 s say nothing (the real
 *   meter's empty first poll after each queue start included).
 *
 * Over the fake WebUSB meter that answers like the real KM003C, whose queue replies can be
 * switched to the bare `41 id 02 00` the meter sends while the CLI polls it over CDC.
 * Runs on Vitest's fake clock (setTimeout and Date).
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { UiError } from "../backend";
import { MeterDriver, openUsbOrOfferHid, type PowerzWasm } from "./driver";
import { realMeterDevice, type RealMeterDevice } from "./realMeter.test";
import { classify, loadWasm } from "./testWasm.test";
import { timeoutError, type WebTransport } from "./transport";
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
  batches: Sample[][];
  stalls: boolean[];
  errors: UiError[];
  /** While true, every queue poll gets the bare `41 id 02 00` (no samples). */
  setStalled(on: boolean): void;
}

/** Opens the real-meter fake over WebUSB and signs in at `rateCode`, with the queue
 * stall switch and the driver's stall callback recorded. */
async function session(rateCode = 2): Promise<Run> {
  const dev = realMeterDevice(w);
  let stalled = false;
  const inner = dev.respond;
  dev.respond = (frame) => {
    if (stalled && classify(w, frame) === "get_data:adc_queue") {
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
  const batches: Sample[][] = [];
  const stalls: boolean[] = [];
  const errors: UiError[] = [];
  const driver = await settle(
    MeterDriver.connect({
      wasm: w,
      transport: opened.transport,
      rateCode,
      onBatch: (s: Sample[]) => batches.push(s),
      onTemp: () => {},
      onError: (e: UiError) => errors.push(e),
      onStall: (on: boolean) => stalls.push(on),
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
  };
}

const queuePolls = (run: Run) => run.dev.repliesTo("get_data:adc_queue").length;

describe("a queue that stops filling (AC-NVN6M8.1)", () => {
  it("nvn6m8: 2 s of empty queue polls report a stall, once", async () => {
    const run = await session(2);
    await vi.advanceTimersByTimeAsync(1_000);
    expect(run.batches.flat().length).toBeGreaterThan(0);
    expect(run.stalls).toEqual([]);

    run.setStalled(true);
    const polls = queuePolls(run);
    await vi.advanceTimersByTimeAsync(3_000);
    // The driver kept polling the whole time and saw only empty replies.
    expect(queuePolls(run) - polls).toBeGreaterThan(10);
    expect(run.stalls).toEqual([true]);
    expect(run.errors).toEqual([]);
    await settle(run.driver.disconnect());
  });
});

describe("the stall ends with the first samples (AC-NVN6M8.2)", () => {
  it("nvn6m8: a long stall reports once, and samples coming back clear it", async () => {
    const run = await session(2);
    await vi.advanceTimersByTimeAsync(1_000);
    run.setStalled(true);
    await vi.advanceTimersByTimeAsync(8_000);
    expect(run.stalls).toEqual([true]);

    const before = run.batches.flat().length;
    run.setStalled(false);
    await vi.advanceTimersByTimeAsync(1_000);
    expect(run.batches.flat().length).toBeGreaterThan(before);
    expect(run.stalls).toEqual([true, false]);

    // Samples keep coming: nothing more to say.
    await vi.advanceTimersByTimeAsync(3_000);
    expect(run.stalls).toEqual([true, false]);
    expect(run.errors).toEqual([]);
    await settle(run.driver.disconnect());
  });

  it("nvn6m8: a second stall reports again, once", async () => {
    const run = await session(2);
    await vi.advanceTimersByTimeAsync(1_000);
    run.setStalled(true);
    await vi.advanceTimersByTimeAsync(3_000);
    run.setStalled(false);
    await vi.advanceTimersByTimeAsync(1_000);
    run.setStalled(true);
    await vi.advanceTimersByTimeAsync(5_000);
    expect(run.stalls).toEqual([true, false, true]);
    await settle(run.driver.disconnect());
  });
});

describe("held polling and short empty runs (AC-NVN6M8.3)", () => {
  it("nvn6m8: empty polls shorter than 2 s report nothing", async () => {
    const run = await session(2);
    await vi.advanceTimersByTimeAsync(1_000);
    run.setStalled(true);
    await vi.advanceTimersByTimeAsync(1_500);
    run.setStalled(false);
    await vi.advanceTimersByTimeAsync(1_000);
    run.setStalled(true);
    await vi.advanceTimersByTimeAsync(1_500);
    run.setStalled(false);
    await vi.advanceTimersByTimeAsync(1_000);
    expect(run.stalls).toEqual([]);
    await settle(run.driver.disconnect());
  });

  for (const rateCode of [0, 1, 2, 3]) {
    it(`nvn6m8: a normal session at rate code ${rateCode} never reports a stall`, async () => {
      // The real meter answers the first poll after each queue start empty.
      const run = await session(rateCode);
      await vi.advanceTimersByTimeAsync(10_000);
      expect(run.batches.flat().length).toBeGreaterThan(0);
      expect(run.stalls).toEqual([]);
      await settle(run.driver.disconnect());
    });
  }

  it("nvn6m8: a rate change's empty first poll reports nothing", async () => {
    const run = await session(2);
    await vi.advanceTimersByTimeAsync(1_000);
    await settle(run.driver.setRate(3));
    await vi.advanceTimersByTimeAsync(3_000);
    expect(run.stalls).toEqual([]);
    await settle(run.driver.disconnect());
  });

  it("nvn6m8: time with the polling held does not count toward the 2 s", async () => {
    const run = await session(2);
    await vi.advanceTimersByTimeAsync(1_000);
    run.setStalled(true);
    await vi.advanceTimersByTimeAsync(1_200);
    run.driver.holdPolling(true);
    await vi.advanceTimersByTimeAsync(5_000);
    expect(run.stalls).toEqual([]);
    run.driver.holdPolling(false);
    // About 1.7 s of unheld empty polling in all: still no stall.
    await vi.advanceTimersByTimeAsync(500);
    expect(run.stalls).toEqual([]);
    // The unheld time goes on adding up past 2 s.
    await vi.advanceTimersByTimeAsync(1_500);
    expect(run.stalls).toEqual([true]);
    await settle(run.driver.disconnect());
  });

  it("nvn6m8: a settings write that holds the link does not count toward the 2 s", async () => {
    const run = await session(2);
    await vi.advanceTimersByTimeAsync(1_000);
    run.setStalled(true);
    await vi.advanceTimersByTimeAsync(1_000);
    // A settings pipe that answers nothing for 3 s: the write holds the link that long.
    const slowPipe: WebTransport = {
      transact: () => new Promise((_, reject) => setTimeout(() => reject(timeoutError()), 3_000)),
      read: () => new Promise((_, reject) => setTimeout(() => reject(timeoutError()), 3_000)),
      close: async () => {},
    };
    const write = run.driver
      .writeSettings({ brightness: 80 } as never, async () => slowPipe)
      .catch(() => "failed");
    await vi.advanceTimersByTimeAsync(3_200);
    expect(await write).toBe("failed");
    expect(run.stalls).toEqual([]);
    // About 1.5 s of unheld empty polling in all: still no stall.
    await vi.advanceTimersByTimeAsync(500);
    expect(run.stalls).toEqual([]);
    await vi.advanceTimersByTimeAsync(1_500);
    expect(run.stalls).toEqual([true]);
    await settle(run.driver.disconnect());
  });
});
