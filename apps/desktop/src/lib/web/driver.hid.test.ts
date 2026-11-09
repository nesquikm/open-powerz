/**
 * FR QFTCWZ, AC-QFTCWZ.3: a HID session (`MeterDriver.connect({ ..., hid: true })`) is
 * ADC-snapshot only. Every frame it sends, across connect, streaming, refused rate / PD
 * requests and disconnect, is GET_DATA with attribute mask 0x001, polled at the
 * snapshot period (100 ms); it never sends CONNECT, auth, a queue start/stop/poll, the
 * PD sniffer or a memory read. Rate changes and PD capture are refused as `usage`
 * errors with no traffic. A full (non-HID) session over the real WebHID transport is
 * stopped by the transport's guard before anything reaches the device.
 *
 * Runs against the wasm demo meter on Vitest's fake clock.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { UiError } from "../backend";
import { DemoTransport } from "./demoTransport";
import { MeterDriver } from "./driver";
import type { PowerzWasm } from "./driver";
import { FakeHidDevice, toReports } from "./fakeDevices.test";
import { classify, loadWasm, type FrameName } from "./testWasm.test";
import type { WebTransport } from "./transport";
import { openWebHid } from "./webhid";
import * as features from "./features";

type DriverOptions = Parameters<typeof MeterDriver.connect>[0];
type Sample = { time_ms: number; vbus_v: number; ibus_a: number };

/** Records every frame the driver hands to the transport, with the fake-clock time. */
class Recorder implements WebTransport {
  frames: { at: number; frame: Uint8Array }[] = [];
  closed = false;
  constructor(
    private w: PowerzWasm,
    private inner: WebTransport,
  ) {}
  names(): FrameName[] {
    return this.frames.map((f) => classify(this.w, f.frame));
  }
  async transact(frame: Uint8Array, timeoutMs: number): Promise<Uint8Array> {
    this.frames.push({ at: Date.now(), frame: frame.slice() });
    return this.inner.transact(frame, timeoutMs);
  }
  read(timeoutMs: number): Promise<Uint8Array> {
    return this.inner.read(timeoutMs);
  }
  async close(): Promise<void> {
    this.closed = true;
    await this.inner.close();
  }
}

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

async function settle<T>(p: Promise<T>, maxMs = 5_000): Promise<T> {
  let done = false;
  let value: T | undefined;
  let error: unknown;
  let failed = false;
  p.then(
    (v) => {
      done = true;
      value = v;
    },
    (e: unknown) => {
      done = true;
      failed = true;
      error = e;
    },
  );
  for (let i = 0; i < 5 && !done; i++) await Promise.resolve();
  for (let t = 0; t < maxMs && !done; t += 10) await vi.advanceTimersByTimeAsync(10);
  if (!done) throw new Error("promise did not settle on the fake clock");
  if (failed) throw error;
  return value as T;
}

interface HidRun {
  rec: Recorder;
  driver: MeterDriver;
  batches: Sample[][];
  errors: UiError[];
}

async function startHid(scenario = "load-5v-1a", rateCode = 2): Promise<HidRun> {
  const rec = new Recorder(w, new DemoTransport(w, scenario, () => Date.now()));
  const batches: Sample[][] = [];
  const errors: UiError[] = [];
  const options = {
    wasm: w,
    transport: rec,
    rateCode,
    hid: true,
    onBatch: (samples: Sample[]) => batches.push(samples),
    onError: (e: UiError) => errors.push(e),
    random: () => 0x1234_5678_9abc_def0n,
  } as unknown as DriverOptions;
  const driver = await settle(MeterDriver.connect(options));
  return { rec, driver, batches, errors };
}

const onlyAdc = (names: FrameName[]) => names.filter((n) => n !== "get_data:adc");

describe("HID session (AC-QFTCWZ.3)", () => {
  it("sends only GET_DATA 0x001 across connect, streaming and disconnect", async () => {
    const { rec, driver, batches, errors } = await startHid();
    await vi.advanceTimersByTimeAsync(2_000);
    await settle(driver.disconnect());
    await vi.advanceTimersByTimeAsync(500);
    const names = rec.names();
    expect(names.length).toBeGreaterThan(5);
    expect(onlyAdc(names)).toEqual([]);
    expect(errors).toEqual([]);
    expect(rec.closed).toBe(true);
    // The snapshots arrive as samples.
    const all = batches.flat();
    expect(all.length).toBeGreaterThan(5);
    expect(all.at(-1)?.vbus_v).toBeCloseTo(5, 0);
    expect(all.at(-1)?.ibus_a).toBeCloseTo(1, 0);
  });

  it("never connects, authenticates, reads memory or starts the queue, whatever the rate asked", async () => {
    for (const rateCode of [0, 1, 2, 3]) {
      const { rec, driver } = await startHid("load-5v-1a", rateCode);
      await vi.advanceTimersByTimeAsync(600);
      await settle(driver.disconnect());
      expect(onlyAdc(rec.names()), `rate code ${rateCode}`).toEqual([]);
      expect(driver.mode).toBe("snapshot");
    }
  });

  it("polls at the 100 ms snapshot period", async () => {
    const { rec, driver } = await startHid();
    const from = Date.now();
    rec.frames = [];
    await vi.advanceTimersByTimeAsync(1_000);
    await settle(driver.disconnect());
    const inWindow = rec.frames.filter((f) => f.at >= from && f.at < from + 1_000);
    expect(inWindow.map((f) => classify(w, f.frame)).every((n) => n === "get_data:adc")).toBe(true);
    expect(inWindow.length).toBeGreaterThanOrEqual(9);
    expect(inWindow.length).toBeLessThanOrEqual(11);
    for (let i = 1; i < inWindow.length; i++) {
      expect(inWindow[i]!.at - inWindow[i - 1]!.at).toBeGreaterThanOrEqual(95);
    }
  });

  it("refuses a rate change with no traffic", async () => {
    const { rec, driver } = await startHid();
    await vi.advanceTimersByTimeAsync(50);
    const before = rec.frames.length;
    await expect(settle(driver.setRate(3))).rejects.toMatchObject({ kind: "usage" });
    // Only the poll's own snapshots may have gone out meanwhile.
    expect(onlyAdc(rec.names().slice(before))).toEqual([]);
    await settle(driver.disconnect());
  });

  it("refuses PD capture with no traffic", async () => {
    const { rec, driver } = await startHid();
    await vi.advanceTimersByTimeAsync(50);
    const rows: unknown[] = [];
    await expect(settle(driver.startPdCapture((r) => rows.push(...r)))).rejects.toMatchObject({
      kind: "usage",
    });
    await vi.advanceTimersByTimeAsync(500);
    await settle(driver.stopPdCapture());
    await settle(driver.disconnect());
    expect(onlyAdc(rec.names())).toEqual([]);
    expect(rows).toEqual([]);
  });
});

describe("the WebHID guard under a full session (AC-QFTCWZ.3)", () => {
  it("a non-HID session over WebHID is refused before any report is sent", async () => {
    const meter = new w.DemoMeter("load-5v-1a");
    const dev = new FakeHidDevice((report) => toReports(meter.transact(report.slice(0, 4))));
    const transport = await openWebHid(dev as unknown as Parameters<typeof openWebHid>[0]);
    const options = {
      wasm: w,
      transport,
      rateCode: 2,
      onBatch: () => {},
      onError: () => {},
      random: () => 1n,
    } as unknown as DriverOptions;
    await expect(settle(MeterDriver.connect(options))).rejects.toMatchObject({ kind: "usage" });
    expect(dev.sent).toEqual([]);
  });

  it("a HID session over the real WebHID transport streams with only ADC reports sent", async () => {
    const meter = new w.DemoMeter("load-5v-1a");
    const dev = new FakeHidDevice((report) => toReports(meter.transact(report.slice(0, 4))));
    const transport = await openWebHid(dev as unknown as Parameters<typeof openWebHid>[0]);
    const batches: Sample[][] = [];
    const errors: UiError[] = [];
    const options = {
      wasm: w,
      transport,
      rateCode: 2,
      hid: true,
      onBatch: (s: Sample[]) => batches.push(s),
      onError: (e: UiError) => errors.push(e),
      random: () => 1n,
    } as unknown as DriverOptions;
    const driver = await settle(MeterDriver.connect(options));
    await vi.advanceTimersByTimeAsync(1_000);
    await settle(driver.disconnect());
    expect(errors).toEqual([]);
    expect(batches.flat().length).toBeGreaterThan(5);
    const adcLen = w.getDataRequest(0, 0x001).length;
    expect(dev.sent.length).toBeGreaterThan(5);
    for (const { data } of dev.sent) {
      expect(classify(w, data.slice(0, adcLen))).toBe("get_data:adc");
      expect(data.slice(adcLen).every((b) => b === 0)).toBe(true);
    }
  });
});

/** FR 09R2FK: the PD refusal uses the shared HID wording helper from ./features. */
describe("the HID refusal wording (AC-09R2FK.2)", () => {
  it("refuses PD capture with the helper's text", async () => {
    const { driver } = await startHid();
    await vi.advanceTimersByTimeAsync(50);
    const text = (features as unknown as { hidOnlyBasicText?: (what: string) => string })
      .hidOnlyBasicText;
    expect(typeof text).toBe("function");
    await expect(settle(driver.startPdCapture(() => {}))).rejects.toMatchObject({
      kind: "usage",
      message: "PD capture is not available over HID: it gives basic readings only.",
    });
    expect(text!("PD capture")).toBe(
      "PD capture is not available over HID: it gives basic readings only.",
    );
    await settle(driver.disconnect());
  });
});
