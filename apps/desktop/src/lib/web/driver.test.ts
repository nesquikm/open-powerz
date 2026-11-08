/**
 * FR QFTCWZ: the browser session driver.
 *
 * - AC-QFTCWZ.3: when the vendor interface cannot be claimed, opening offers WebHID.
 * - AC-QFTCWZ.4: connect → device info → auth → queue start at the requested rate →
 *   polls on the native period (100 ms, 20 ms at 1000 SPS) with samples in batches →
 *   queue stop on disconnect; ADC snapshots when auth fails.
 * - AC-QFTCWZ.5: every frame the driver sends, on every path, is one of the safe read
 *   and stream requests, and the driver has no API that sends caller-supplied bytes.
 * - AC-QFTCWZ.6: a transfer error ends the session as `disconnected`, a timeout as
 *   `timeout`, both as `UiError`s.
 *
 * Runs over WebUSB against the fake meter that answers like the real KM003C
 * (`realMeter.test.ts`, AC-F3M587.5) on Vitest's fake clock (setTimeout and Date), so
 * the poll cadence is measured, not slept.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { UiError } from "../backend";
import * as driverModule from "./driver";
import { MeterDriver, openUsbOrOfferHid } from "./driver";
import type { PowerzWasm } from "./driver";
import { FakeUsbDevice } from "./fakeDevices.test";
import { realMeterDevice } from "./realMeter.test";
import { classify, loadWasm, type FrameName } from "./testWasm.test";
import type { WebTransport } from "./transport";

type DriverOptions = Parameters<typeof MeterDriver.connect>[0];
type Sample = { time_ms: number; vbus_v: number; ibus_a: number };

/** Wraps the meter's transport: records every frame, can fail transfers on demand and can
 * spoil auth replies. */
class Recorder implements WebTransport {
  frames: Uint8Array[] = [];
  closed = false;
  failWith: UiError | null = null;
  spoilAuth = false;
  constructor(
    private w: PowerzWasm,
    private inner: WebTransport,
  ) {}
  names(): FrameName[] {
    return this.frames.map((f) => classify(this.w, f));
  }
  async transact(frame: Uint8Array, timeoutMs: number): Promise<Uint8Array> {
    this.frames.push(frame.slice());
    if (this.failWith) throw this.failWith;
    const reply = await this.inner.transact(frame, timeoutMs);
    if (this.spoilAuth && classify(this.w, frame) === "auth") {
      const bad = reply.slice();
      for (let i = 4; i < bad.length; i++) bad[i] = (bad[i] ?? 0) ^ 0xa5;
      return bad;
    }
    return reply;
  }
  async read(timeoutMs: number): Promise<Uint8Array> {
    if (this.failWith) throw this.failWith;
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

/** Advance the fake clock in small steps until `p` settles (at most `maxMs`). */
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
    (e) => {
      done = true;
      failed = true;
      error = e;
    },
  );
  for (let t = 0; t < maxMs && !done; t += 10) await vi.advanceTimersByTimeAsync(10);
  if (!done) throw new Error("promise did not settle on the fake clock");
  if (failed) throw error;
  return value as T;
}

interface Run {
  rec: Recorder;
  driver: MeterDriver;
  batches: Sample[][];
  errors: UiError[];
}

/** The WebUSB transport of a fake meter that answers like the real KM003C. */
async function realMeterTransport(scenario: string): Promise<WebTransport> {
  const dev = realMeterDevice(w, { scenario });
  const opened = await settle(
    openUsbOrOfferHid(dev as unknown as Parameters<typeof openUsbOrOfferHid>[0], {
      debug: false,
    }),
  );
  if (!("transport" in opened)) throw new Error("expected the WebUSB transport");
  return opened.transport;
}

async function start(
  scenario = "load-5v-1a",
  rateCode = 2,
  tweak: (rec: Recorder) => void = () => {},
): Promise<Run> {
  const rec = new Recorder(w, await realMeterTransport(scenario));
  tweak(rec);
  const batches: Sample[][] = [];
  const errors: UiError[] = [];
  const options = {
    wasm: w,
    transport: rec,
    rateCode,
    onBatch: (samples: Sample[]) => batches.push(samples),
    onError: (e: UiError) => errors.push(e),
    random: () => 0x1234_5678_9abc_def0n,
  } as unknown as DriverOptions;
  const driver = await settle(MeterDriver.connect(options));
  return { rec, driver, batches, errors };
}

const count = (names: FrameName[], name: FrameName) => names.filter((n) => n === name).length;

describe("opening the meter (AC-QFTCWZ.3)", () => {
  it("returns the WebUSB transport when the vendor interface can be claimed", async () => {
    const dev = new FakeUsbDevice(() => new Uint8Array([0x05, 0x01, 0x00, 0x00]));
    const opened = await settle(
      openUsbOrOfferHid(dev as unknown as Parameters<typeof openUsbOrOfferHid>[0]),
    );
    expect("transport" in opened).toBe(true);
    expect("offerHid" in opened && opened.offerHid).toBeFalsy();
  });

  it("offers WebHID when claiming the vendor interface fails", async () => {
    const dev = new FakeUsbDevice();
    dev.claimError = new DOMException("Unable to claim interface.", "NetworkError");
    const opened = await settle(
      openUsbOrOfferHid(dev as unknown as Parameters<typeof openUsbOrOfferHid>[0]),
    );
    expect("transport" in opened).toBe(false);
    expect(opened).toMatchObject({ offerHid: true, error: { kind: "device_busy" } });
    const error = (opened as { error: UiError }).error;
    expect(error.message.length).toBeGreaterThan(0);
  });
});

describe("session sequence (AC-QFTCWZ.4)", () => {
  it("connects, reads the device info, authenticates and starts the queue at the rate", async () => {
    const { rec, driver } = await start("load-5v-1a", 2);
    const names = rec.names();
    expect(names[0]).toBe("connect");
    const auth = names.indexOf("auth");
    const startQueue = names.indexOf("start_queue:2");
    expect(auth).toBeGreaterThan(0);
    expect(startQueue).toBeGreaterThan(auth);
    // All four info blocks are read, before the auth (it needs the UID).
    for (const addr of [0x0000_0420, 0x0000_4420, 0x0300_0c00, 0x4001_0450]) {
      const at = names.indexOf(`memory_read:${addr}`);
      expect(at, `block ${addr.toString(16)}`).toBeGreaterThan(0);
      expect(at).toBeLessThan(auth);
    }
    expect(driver.info).toMatchObject({
      model: "KM003C",
      firmware_version: "2.0.6",
      serial: "123456",
    });
    expect(driver.authLevel).toBe(1);
    expect(driver.mode).toBe("queue");
    await settle(driver.disconnect());
  });

  it("polls every 100 ms at 50 SPS and delivers the samples in batches", async () => {
    const { rec, driver, batches, errors } = await start("load-5v-1a", 2);
    const before = rec.frames.length;
    await vi.advanceTimersByTimeAsync(1_000);
    const polls = count(rec.names().slice(before), "get_data:adc_queue");
    expect(polls).toBeGreaterThanOrEqual(8);
    expect(polls).toBeLessThanOrEqual(12);
    const samples = batches.flat();
    expect(samples.length).toBeGreaterThanOrEqual(40);
    expect(samples.length).toBeLessThanOrEqual(60);
    expect(batches.length).toBeLessThan(samples.length); // batches, not one call per sample
    expect(batches.some((b) => b.length > 1)).toBe(true);
    expect(samples[0]?.vbus_v).toBeCloseTo(5, 0);
    expect(samples[0]?.ibus_a).toBeCloseTo(1, 0);
    expect(errors).toEqual([]);
    await settle(driver.disconnect());
  });

  it("polls every 20 ms at 1000 SPS", async () => {
    const { rec, driver } = await start("load-5v-1a", 3);
    expect(rec.names()).toContain("start_queue:3");
    const before = rec.frames.length;
    await vi.advanceTimersByTimeAsync(1_000);
    const polls = count(rec.names().slice(before), "get_data:adc_queue");
    expect(polls).toBeGreaterThanOrEqual(40);
    expect(polls).toBeLessThanOrEqual(55);
    await settle(driver.disconnect());
  });

  it("stops the queue on disconnect, closes the transport and sends nothing after", async () => {
    const { rec, driver } = await start();
    await vi.advanceTimersByTimeAsync(300);
    await settle(driver.disconnect());
    expect(rec.names()).toContain("stop_queue");
    expect(rec.closed).toBe(true);
    const after = rec.frames.length;
    await vi.advanceTimersByTimeAsync(1_000);
    expect(rec.frames.length).toBe(after);
  });

  it("falls back to ADC snapshots when authentication fails", async () => {
    const { rec, driver, batches, errors } = await start("load-5v-1a", 2, (r) => {
      r.spoilAuth = true;
    });
    expect(driver.mode).toBe("snapshot");
    expect(driver.authLevel).toBe(0);
    expect(driver.info.model).toBe("KM003C");
    const before = rec.frames.length;
    await vi.advanceTimersByTimeAsync(1_000);
    const names = rec.names();
    expect(names.some((n) => n.startsWith("start_queue"))).toBe(false);
    expect(count(names.slice(before), "get_data:adc")).toBeGreaterThanOrEqual(3);
    const samples = batches.flat();
    expect(samples.length).toBeGreaterThanOrEqual(3);
    expect(samples[0]?.vbus_v).toBeCloseTo(5, 0);
    expect(errors).toEqual([]);
    await settle(driver.disconnect());
  });

  it("captures PD traffic passively and stops the sniffer", async () => {
    const { rec, driver } = await start("pd-9v-2a", 2);
    const rows: { name?: string }[] = [];
    await settle(driver.startPdCapture((r: { name?: string }[]) => rows.push(...r)));
    expect(rec.names()).toContain("start_pd");
    await vi.advanceTimersByTimeAsync(3_000);
    const names = rec.names();
    expect(names.some((n) => n === "pd_poll" || n === "pd_poll_adc")).toBe(true);
    expect(rows.some((r) => r.name === "Source_Capabilities")).toBe(true);
    await settle(driver.stopPdCapture());
    expect(rec.names()).toContain("stop_pd");
    const polled = rec.names().filter((n) => n.startsWith("pd_poll")).length;
    await vi.advanceTimersByTimeAsync(1_000);
    expect(rec.names().filter((n) => n.startsWith("pd_poll")).length).toBe(polled);
    await settle(driver.disconnect());
  });
});

describe("closed command set (AC-QFTCWZ.5)", () => {
  it("every frame sent on every path is a safe read or stream request", async () => {
    const all: FrameName[] = [];

    const normal = await start("pd-9v-2a", 2);
    await vi.advanceTimersByTimeAsync(500);
    await settle(normal.driver.startPdCapture(() => {}));
    await vi.advanceTimersByTimeAsync(1_000);
    await settle(normal.driver.stopPdCapture());
    await settle(normal.driver.disconnect());
    all.push(...normal.rec.names());

    const fast = await start("load-5v-1a", 3);
    await vi.advanceTimersByTimeAsync(200);
    await settle(fast.driver.disconnect());
    all.push(...fast.rec.names());

    const noAuth = await start("load-5v-1a", 1, (r) => {
      r.spoilAuth = true;
    });
    await vi.advanceTimersByTimeAsync(1_000);
    await settle(noAuth.driver.disconnect());
    all.push(...noAuth.rec.names());

    const failing = await start("load-5v-1a", 2);
    failing.rec.failWith = { kind: "disconnected", message: "gone", hint: null };
    await vi.advanceTimersByTimeAsync(500);
    all.push(...failing.rec.names());

    expect(all.length).toBeGreaterThan(20);
    expect(all.filter((n) => n === "unknown")).toEqual([]);
    for (const n of all) {
      expect(n).toMatch(
        /^(connect|auth|get_data:(adc|adc_queue|settings|pd_packet|pd_status)|start_queue:[0-3]|stop_queue|start_pd|stop_pd|pd_poll|pd_poll_adc|memory_read:\d+)$/,
      );
    }
  });

  it("offers no API that sends caller-supplied bytes", () => {
    const rawish = /send|raw|write|transact|exec|command|frame|bytes/i;
    expect(Object.keys(driverModule).filter((k) => rawish.test(k))).toEqual([]);
    const methods = Object.getOwnPropertyNames(MeterDriver.prototype).filter(
      (k) => k !== "constructor",
    );
    expect(methods.length).toBeGreaterThan(0);
    expect(methods.filter((k) => rawish.test(k))).toEqual([]);
    const statics = Object.getOwnPropertyNames(MeterDriver).filter(
      (k) => !["length", "name", "prototype"].includes(k),
    );
    expect(statics.filter((k) => rawish.test(k))).toEqual([]);
  });
});

describe("session errors (AC-QFTCWZ.6)", () => {
  it("a transfer error ends the session as disconnected", async () => {
    const run = await start();
    await vi.advanceTimersByTimeAsync(300);
    run.rec.failWith = { kind: "disconnected", message: "The meter was unplugged.", hint: null };
    await vi.advanceTimersByTimeAsync(300);
    expect(run.errors).toHaveLength(1);
    expect(run.errors[0]).toMatchObject({ kind: "disconnected" });
    expect(typeof run.errors[0]?.message).toBe("string");
    const after = run.rec.frames.length;
    await vi.advanceTimersByTimeAsync(1_000);
    expect(run.rec.frames.length).toBe(after);
    expect(run.errors).toHaveLength(1);
  });

  it("a timeout ends the session as timeout", async () => {
    const run = await start();
    await vi.advanceTimersByTimeAsync(300);
    run.rec.failWith = { kind: "timeout", message: "The meter did not answer.", hint: null };
    await vi.advanceTimersByTimeAsync(300);
    expect(run.errors).toHaveLength(1);
    expect(run.errors[0]).toMatchObject({ kind: "timeout" });
    const after = run.rec.frames.length;
    await vi.advanceTimersByTimeAsync(1_000);
    expect(run.rec.frames.length).toBe(after);
  });

  it("a non-UiError thrown by a transport still reaches the caller as a known kind", async () => {
    const run = await start();
    run.rec.failWith = new DOMException("A transfer error has occurred.", "NetworkError") as never;
    await vi.advanceTimersByTimeAsync(300);
    expect(run.errors).toHaveLength(1);
    expect(["disconnected", "timeout", "protocol", "internal"]).toContain(run.errors[0]?.kind);
  });

  it("a timeout during connect rejects connect with kind timeout", async () => {
    const rec = new Recorder(w, await realMeterTransport("idle"));
    rec.failWith = { kind: "timeout", message: "The meter did not answer.", hint: null };
    const options = {
      wasm: w,
      transport: rec,
      rateCode: 2,
      onBatch: () => {},
      onError: () => {},
    } as unknown as DriverOptions;
    await expect(settle(MeterDriver.connect(options))).rejects.toMatchObject({ kind: "timeout" });
  });
});
