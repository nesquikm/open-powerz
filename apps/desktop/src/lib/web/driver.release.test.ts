/**
 * FR G30K4R: a failed connect frees the meter.
 *
 * - AC-G30K4R.1: when the sign-in fails at any step (timeout, protocol error, REJECT,
 *   disconnect), the driver closes the transport before it rejects; over WebUSB the
 *   interface is released and the device closed.
 * - AC-G30K4R.4: regression over a fake USBDevice whose meter never answers CONNECT:
 *   after the connect rejects the interface is released and the device closed, and a
 *   second connect on the same fake opens and claims it afresh.
 *
 * Runs on Vitest's fake clock (the 500 ms + recovery + 2000 ms CONNECT waits).
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DemoTransport } from "./demoTransport";
import { MeterDriver, openUsbOrOfferHid, type PowerzWasm } from "./driver";
import { FakeUsbDevice } from "./fakeDevices.test";
import { classify, loadWasm } from "./testWasm.test";
import { disconnectedError, timeoutError, type WebTransport } from "./transport";

type DriverOptions = Parameters<typeof MeterDriver.connect>[0];
type OpenDevice = Parameters<typeof openUsbOrOfferHid>[0];
type OpenOptions = Parameters<typeof openUsbOrOfferHid>[1];

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
async function settle<T>(p: Promise<T>, maxMs = 10_000): Promise<T> {
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

/** How the scripted meter fails the sign-in. */
type Failure = "none" | "timeout" | "reject" | "disconnect" | "protocol" | "memory-timeout";

/** The demo meter behind a script that fails one sign-in step; counts close() calls. */
class Failing implements WebTransport {
  closes = 0;
  private readonly inner: DemoTransport;
  constructor(private readonly failure: Failure) {
    this.inner = new DemoTransport(w, "load-5v-1a", () => Date.now());
  }
  private wait(timeoutMs: number): Promise<never> {
    return new Promise((_, reject) => setTimeout(() => reject(timeoutError()), timeoutMs));
  }
  async transact(frame: Uint8Array, timeoutMs: number): Promise<Uint8Array> {
    const name = classify(w, frame);
    const id = frame[1] ?? 0;
    if (name === "connect") {
      if (this.failure === "timeout") return this.wait(timeoutMs);
      if (this.failure === "reject") return new Uint8Array([0x06, id, 0, 0]);
      if (this.failure === "disconnect") throw disconnectedError();
    }
    if (name.startsWith("memory_read:")) {
      if (this.failure === "memory-timeout") return this.wait(timeoutMs);
      // A reply with the right id that is not a memory-read answer.
      if (this.failure === "protocol") return new Uint8Array([0x7f, id, 0, 0]);
    }
    return this.inner.transact(frame, timeoutMs);
  }
  async read(timeoutMs: number): Promise<Uint8Array> {
    return this.wait(timeoutMs);
  }
  async recover(): Promise<void> {}
  async close(): Promise<void> {
    this.closes += 1;
    await this.inner.close();
  }
}

function connect(transport: WebTransport): Promise<MeterDriver> {
  const options = {
    wasm: w,
    transport,
    rateCode: 2,
    onBatch: () => {},
    onError: () => {},
    random: () => 0x1234_5678_9abc_def0n,
  } as unknown as DriverOptions;
  return MeterDriver.connect(options);
}

/** Connect over `t`; resolves with how many closes had happened when it rejected. */
async function closesAtRejection(t: Failing): Promise<number> {
  let at = -1;
  const p = connect(t).then(
    () => {
      throw new Error("expected the connect to fail");
    },
    () => {
      at = t.closes;
    },
  );
  await settle(p);
  return at;
}

describe("the driver closes the transport on a failed sign-in (AC-G30K4R.1)", () => {
  for (const failure of [
    "timeout",
    "reject",
    "disconnect",
    "protocol",
    "memory-timeout",
  ] as const) {
    it(`closes the transport before it rejects (${failure})`, async () => {
      const t = new Failing(failure);
      expect(await closesAtRejection(t)).toBeGreaterThanOrEqual(1);
    });
  }

  it("a successful connect leaves the transport open until disconnect", async () => {
    const t = new Failing("none");
    const driver = await settle(connect(t));
    expect(t.closes).toBe(0);
    await settle(driver.disconnect());
    expect(t.closes).toBe(1);
  });
});

/** A fake meter on WebUSB: silent (never answers CONNECT) or the demo meter. */
function meterResponder(silent: () => boolean): (frame: Uint8Array) => Uint8Array | null {
  const meter = new w.DemoMeter("load-5v-1a");
  let last = Date.now();
  return (frame) => {
    if (silent()) return null;
    const now = Date.now();
    if (now > last) {
      meter.advance(now - last);
      last = now;
    }
    const reply = meter.transact(frame);
    return reply.length === 0 ? null : reply;
  };
}

async function openUsb(dev: FakeUsbDevice): Promise<WebTransport> {
  const opened = await settle(
    openUsbOrOfferHid(dev as unknown as OpenDevice, { debug: false } as unknown as OpenOptions),
  );
  if (!("transport" in opened)) throw new Error("expected the WebUSB transport");
  return opened.transport;
}

const VENDOR_IFACE = 4;
const claimed = (dev: FakeUsbDevice): boolean =>
  dev.configuration?.interfaces.find((i) => i.interfaceNumber === VENDOR_IFACE)?.claimed === true;

describe("a REJECT over WebUSB frees the meter (AC-G30K4R.1)", () => {
  it("releases the interface and closes the device after a REJECTed CONNECT", async () => {
    const dev = new FakeUsbDevice((frame) =>
      classify(w, frame) === "connect" ? new Uint8Array([0x06, frame[1] ?? 0, 0, 0]) : null,
    );
    const t = await openUsb(dev);
    expect(claimed(dev)).toBe(true);
    await expect(settle(connect(t))).rejects.toHaveProperty("kind");
    expect(dev.calls).toContain(`releaseInterface:${VENDOR_IFACE}`);
    expect(dev.calls[dev.calls.length - 1]).toBe("close");
    expect(claimed(dev)).toBe(false);
    expect(dev.opened).toBe(false);
  });
});

describe("regression: a meter that never answers CONNECT (AC-G30K4R.4)", () => {
  it("ends with the interface released and the device closed after the connect rejects", async () => {
    const dev = new FakeUsbDevice(meterResponder(() => true));
    const t = await openUsb(dev);
    expect(claimed(dev)).toBe(true);
    expect(dev.opened).toBe(true);
    await expect(settle(connect(t))).rejects.toMatchObject({ kind: "timeout" });
    expect(dev.calls).toContain(`releaseInterface:${VENDOR_IFACE}`);
    expect(dev.calls).toContain("close");
    expect(claimed(dev)).toBe(false);
    expect(dev.opened).toBe(false);
    expect(dev.waitingIn).toBe(0);
  });

  it("a second connect on the same fake opens and claims it afresh and signs in", async () => {
    let silent = true;
    const dev = new FakeUsbDevice(meterResponder(() => silent));
    const first = await openUsb(dev);
    await expect(settle(connect(first))).rejects.toMatchObject({ kind: "timeout" });
    const mark = dev.calls.length;

    // The meter answers from now on (say, after a replug).
    silent = false;
    const second = await openUsb(dev);
    const after = dev.calls.slice(mark);
    expect(after[0]).toBe("open");
    expect(after).toContain(`claimInterface:${VENDOR_IFACE}`);
    // The failed session had closed the device before this open.
    expect(dev.calls.slice(0, mark)).toContain("close");
    expect(claimed(dev)).toBe(true);
    expect(dev.opened).toBe(true);

    const driver = await settle(connect(second));
    expect(driver.info).toMatchObject({ model: "KM003C", serial: "123456" });
    await settle(driver.disconnect());
  });
});
