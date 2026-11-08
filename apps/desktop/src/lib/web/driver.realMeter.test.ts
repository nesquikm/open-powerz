/**
 * FR F3M587: the whole web session over the fake WebUSB meter that answers like the
 * real KM003C (`realMeter.test.ts`).
 *
 * - AC-F3M587.3: an ADC snapshot without an ADC section while the queue streams costs
 *   only the temperature; the stream goes on.
 * - AC-F3M587.4: sign-in, device info, ADC snapshot, the queue at each of the four
 *   rates and the PD sniffer run with no error.
 * - AC-F3M587.6: regression: the first queue poll gets the empty level-1 reply and the
 *   stream keeps running.
 *
 * Runs on Vitest's fake clock (setTimeout and Date).
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { UiError } from "../backend";
import { MeterDriver, openUsbOrOfferHid, type PowerzWasm } from "./driver";
import { realMeterDevice, type RealMeterDevice, type RealMeterOptions } from "./realMeter.test";
import { loadWasm } from "./testWasm.test";
import { sentNames, settle } from "./webBackendFakes.test";

type DriverOptions = Parameters<typeof MeterDriver.connect>[0];
type Sample = { time_ms: number; vbus_v: number; ibus_a: number };

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

interface Session {
  dev: RealMeterDevice;
  driver: MeterDriver;
  batches: Sample[][];
  temps: (number | null)[];
  errors: UiError[];
}

/** Open the fake real meter over WebUSB and run the driver's sign-in at `rateCode`. */
async function session(rateCode: number, options: RealMeterOptions = {}): Promise<Session> {
  const dev = realMeterDevice(w, options);
  const opened = await settle(
    openUsbOrOfferHid(dev as unknown as Parameters<typeof openUsbOrOfferHid>[0], {
      debug: false,
    }),
  );
  if (!("transport" in opened)) throw new Error("expected the WebUSB transport");
  const batches: Sample[][] = [];
  const temps: (number | null)[] = [];
  const errors: UiError[] = [];
  const driver = await settle(
    MeterDriver.connect({
      wasm: w,
      transport: opened.transport,
      rateCode,
      onBatch: (s: Sample[]) => batches.push(s),
      onTemp: (t: number | null) => temps.push(t),
      onError: (e: UiError) => errors.push(e),
      random: () => 0x1234_5678_9abc_def0n,
    } as unknown as DriverOptions),
  );
  return { dev, driver, batches, temps, errors };
}

const isBareHeader = (r: Uint8Array, level: number) =>
  r.length === 4 && r[0] === 0x41 && r[2] === level << 1 && r[3] === 0;

describe("the real-meter fake (AC-F3M587.4)", () => {
  it("answers like the real KM003C", async () => {
    const { dev, driver } = await session(2);
    await vi.advanceTimersByTimeAsync(1_000);
    // A stale frame waited at open.
    expect(dev.inResults[0]).toEqual(new Uint8Array([0x03, 0x2a, 0x00, 0x00]));
    // The memory-read confirmation and its data came in separate transfers.
    const reads = dev.exchanges.filter((e) => e.name.startsWith("memory_read:"));
    expect(reads).toHaveLength(4);
    for (const r of reads) {
      expect(r.replies.map((p) => p.length)[0]).toBe(20);
      expect(r.replies.length).toBe(2);
    }
    // After sign-in every reply header carries level 1.
    expect(dev.level).toBe(1);
    const auth = dev.exchanges.findIndex((e) => e.name === "auth");
    const after = dev.exchanges.slice(auth + 1).flatMap((e) => e.replies.slice(0, 1));
    expect(after.length).toBeGreaterThan(5);
    for (const r of after) expect(((r[2] ?? 0) >> 1) & 0b11).toBe(1);
    // The first queue poll got the bare level-1 header; later ones run past 64 bytes.
    const polls = dev.repliesTo("get_data:adc_queue");
    expect(polls.length).toBeGreaterThan(3);
    expect(isBareHeader(polls[0] ?? new Uint8Array(), 1)).toBe(true);
    expect(polls.slice(1).some((r) => r.length > 64)).toBe(true);
    await settle(driver.disconnect());
  });
});

describe("the whole web session over the real-meter fake (AC-F3M587.4)", () => {
  for (const rateCode of [0, 1, 2, 3]) {
    it(`signs in, reads the device info and streams the queue at rate code ${rateCode}`, async () => {
      const { dev, driver, batches, temps, errors } = await session(rateCode);
      expect(driver.info).toMatchObject({
        model: "KM003C",
        firmware_version: "2.0.6",
        serial: "123456",
      });
      expect(driver.authLevel).toBe(1);
      expect(driver.mode).toBe("queue");
      expect(sentNames(w, dev)).toContain(`start_queue:${rateCode}`);
      await vi.advanceTimersByTimeAsync(3_000);
      expect(errors).toEqual([]);
      // The ADC snapshot for the temperature ran while the queue streamed.
      expect(sentNames(w, dev)).toContain("get_data:adc");
      expect(temps.length).toBeGreaterThan(0);
      // The queue kept polling past the empty first reply and delivered samples.
      expect(dev.repliesTo("get_data:adc_queue").length).toBeGreaterThan(10);
      expect(batches.flat().length).toBeGreaterThan(0);
      expect(batches.flat()[0]?.vbus_v).toBeCloseTo(5, 0);
      if (rateCode >= 2) {
        expect(dev.repliesTo("get_data:adc_queue").some((r) => r.length > 64)).toBe(true);
      }
      await settle(driver.disconnect());
      expect(errors).toEqual([]);
    });
  }

  it("changes the rate: the first poll after the restart is empty too, and the stream goes on", async () => {
    const { dev, driver, batches, errors } = await session(2);
    await vi.advanceTimersByTimeAsync(1_000);
    await settle(driver.setRate(3));
    const before = batches.flat().length;
    const pollsBefore = dev.repliesTo("get_data:adc_queue").length;
    await vi.advanceTimersByTimeAsync(1_000);
    expect(errors).toEqual([]);
    const after = dev.repliesTo("get_data:adc_queue").slice(pollsBefore);
    expect(isBareHeader(after[0] ?? new Uint8Array(), 1)).toBe(true);
    expect(after.length).toBeGreaterThan(10);
    expect(batches.flat().length).toBeGreaterThan(before);
    await settle(driver.disconnect());
  });

  it("captures PD traffic past the empty first PD poll", async () => {
    const { dev, driver, errors } = await session(2, { scenario: "pd-9v-2a" });
    const rows: { name?: string }[] = [];
    await settle(driver.startPdCapture((r: { name?: string }[]) => rows.push(...r)));
    await vi.advanceTimersByTimeAsync(3_000);
    expect(errors).toEqual([]);
    const pdPolls = [...dev.repliesTo("pd_poll"), ...dev.repliesTo("pd_poll_adc")];
    expect(pdPolls.length).toBeGreaterThan(3);
    expect(pdPolls.some((r) => isBareHeader(r, 1))).toBe(true);
    expect(rows.some((r) => r.name === "Source_Capabilities")).toBe(true);
    await settle(driver.stopPdCapture());
    await settle(driver.disconnect());
    expect(errors).toEqual([]);
  });
});

describe("regression: the meter's empty first queue reply (AC-F3M587.6)", () => {
  it("the first queue poll gets 41 id 02 00 and the stream keeps running", async () => {
    const { dev, driver, batches, errors } = await session(2);
    await vi.advanceTimersByTimeAsync(100);
    const first = dev.exchanges.find((e) => e.name === "get_data:adc_queue");
    if (!first) throw new Error("no queue poll was sent");
    expect(first.replies).toEqual([new Uint8Array([0x41, first.request[1] ?? 0, 0x02, 0x00])]);
    await vi.advanceTimersByTimeAsync(2_000);
    expect(errors).toEqual([]);
    expect(dev.repliesTo("get_data:adc_queue").length).toBeGreaterThan(10);
    expect(batches.flat().length).toBeGreaterThan(20);
    await settle(driver.disconnect());
  });
});

describe("an ADC snapshot without an ADC section while the queue streams (AC-F3M587.3)", () => {
  it("costs only the temperature: the stream goes on with no error", async () => {
    const { dev, driver, batches, temps, errors } = await session(2, { noAdcWhileQueue: true });
    await vi.advanceTimersByTimeAsync(3_000);
    const snapshots = dev.repliesTo("get_data:adc");
    expect(snapshots.length).toBeGreaterThan(1);
    expect(snapshots.every((r) => isBareHeader(r, 1))).toBe(true);
    expect(temps).toEqual([]);
    expect(errors).toEqual([]);
    expect(dev.repliesTo("get_data:adc_queue").length).toBeGreaterThan(10);
    expect(batches.flat().length).toBeGreaterThan(20);
    await settle(driver.disconnect());
  });
});
