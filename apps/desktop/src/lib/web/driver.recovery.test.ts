/**
 * FR VG37JA: the browser driver follows the native session.
 *
 * - AC-VG37JA.2: the first CONNECT waits 500 ms; without a matching ACCEPT (a timeout,
 *   or only replies with another id) the driver recovers the transport once and sends
 *   one more CONNECT that waits 2000 ms. A REJECT or a disconnect fails at once.
 * - AC-VG37JA.3: a memory read takes the 20-byte confirmation from the first reply and
 *   reads further IN transfers until it holds the encrypted data (rounded up to 16
 *   bytes); a reply that already carries the data (the demo meter) still decodes.
 * - AC-VG37JA.6: regression over a fake USBDevice that replays the real meter (a stale
 *   DISCONNECT waiting at open, a silent first CONNECT, memory-read data in its own
 *   transfer after the confirmation): it connects, reads the device info and starts
 *   the queue.
 *
 * Runs on Vitest's fake clock, so the 500 ms / 2000 ms waits are measured, not slept.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { UiError } from "../backend";
import { DemoTransport } from "./demoTransport";
import { MeterDriver, openUsbOrOfferHid, type PowerzWasm } from "./driver";
import { FakeUsbDevice } from "./fakeDevices.test";
import { classify, loadWasm, type FrameName } from "./testWasm.test";
import { disconnectedError, timeoutError, type WebTransport } from "./transport";
import { sentNames } from "./webBackendFakes.test";

type DriverOptions = Parameters<typeof MeterDriver.connect>[0];

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

type ConnectAnswer = "ok" | "timeout" | "wrong-id" | "reject" | "disconnect";

type Event =
  | { op: "transact"; name: FrameName; timeoutMs: number }
  | { op: "read"; timeoutMs: number }
  | { op: "recover" };

/** The demo meter behind a script: how each CONNECT is answered, and whether memory
 * reads arrive split (confirmation, then the data in `chunk`-byte IN transfers). */
class Scripted implements WebTransport {
  events: Event[] = [];
  private queued: Uint8Array[] = [];
  private readonly inner: DemoTransport;
  constructor(
    private readonly connects: ConnectAnswer[],
    private readonly chunk: number | null = null,
  ) {
    this.inner = new DemoTransport(w, "load-5v-1a", () => Date.now());
  }
  private wait(timeoutMs: number): Promise<never> {
    return new Promise((_, reject) => setTimeout(() => reject(timeoutError()), timeoutMs));
  }
  async transact(frame: Uint8Array, timeoutMs: number): Promise<Uint8Array> {
    const name = classify(w, frame);
    this.events.push({ op: "transact", name, timeoutMs });
    if (name === "connect") {
      const answer = this.connects.shift() ?? "ok";
      const id = frame[1] ?? 0;
      if (answer === "timeout") return this.wait(timeoutMs);
      if (answer === "wrong-id") return new Uint8Array([0x05, (id + 0x40) & 0xff, 0, 0]);
      if (answer === "reject") return new Uint8Array([0x06, id, 0, 0]);
      if (answer === "disconnect") throw disconnectedError();
    }
    const reply = await this.inner.transact(frame, timeoutMs);
    if (this.chunk !== null && name.startsWith("memory_read:")) {
      for (let at = 20; at < reply.length; at += this.chunk) {
        this.queued.push(reply.slice(at, Math.min(at + this.chunk, reply.length)));
      }
      return reply.slice(0, 20);
    }
    return reply;
  }
  async read(timeoutMs: number): Promise<Uint8Array> {
    this.events.push({ op: "read", timeoutMs });
    const next = this.queued.shift();
    if (next) return next;
    return this.wait(timeoutMs);
  }
  async recover(): Promise<void> {
    this.events.push({ op: "recover" });
    this.queued = [];
  }
  async close(): Promise<void> {
    await this.inner.close();
  }
  connectTimeouts(): number[] {
    return this.events.flatMap((e) =>
      e.op === "transact" && e.name === "connect" ? [e.timeoutMs] : [],
    );
  }
  recovers(): number {
    return this.events.filter((e) => e.op === "recover").length;
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

const DEMO_INFO = { model: "KM003C", firmware_version: "2.0.6", serial: "123456" };

describe("connect recovery (AC-VG37JA.2)", () => {
  it("a first CONNECT answered at once waits 500 ms and needs no recovery", async () => {
    const t = new Scripted(["ok"]);
    const driver = await settle(connect(t));
    expect(t.connectTimeouts()).toEqual([500]);
    expect(t.recovers()).toBe(0);
    await settle(driver.disconnect());
  });

  it("a silent first CONNECT: recover once, then one more CONNECT that waits 2000 ms", async () => {
    const t = new Scripted(["timeout", "ok"]);
    const driver = await settle(connect(t));
    expect(t.connectTimeouts()).toEqual([500, 2_000]);
    expect(t.recovers()).toBe(1);
    // The recovery sits between the two CONNECTs.
    const kinds = t.events.map((e) => (e.op === "transact" ? e.name : e.op));
    const first = kinds.indexOf("connect");
    const rec = kinds.indexOf("recover");
    expect(rec).toBeGreaterThan(first);
    expect(kinds.indexOf("connect", first + 1)).toBeGreaterThan(rec);
    expect(driver.info).toMatchObject(DEMO_INFO);
    expect(driver.mode).toBe("queue");
    await settle(driver.disconnect());
  });

  it("a first CONNECT that only gets replies with another id is retried after recovery", async () => {
    const t = new Scripted(["wrong-id", "ok"]);
    const driver = await settle(connect(t));
    expect(t.connectTimeouts()).toEqual([500, 2_000]);
    expect(t.recovers()).toBe(1);
    expect(driver.info).toMatchObject(DEMO_INFO);
    await settle(driver.disconnect());
  });

  it("retries only once: two silent CONNECTs fail as a timeout", async () => {
    const t = new Scripted(["timeout", "timeout", "ok"]);
    await expect(settle(connect(t))).rejects.toMatchObject({ kind: "timeout" });
    expect(t.connectTimeouts()).toEqual([500, 2_000]);
    expect(t.recovers()).toBe(1);
  });

  it("a REJECT fails at once with no recovery and no second CONNECT", async () => {
    const t = new Scripted(["reject", "ok"]);
    const failure = (await settle(connect(t)).catch((e: unknown) => e)) as UiError;
    expect(failure).toHaveProperty("kind");
    expect(failure.kind).not.toBe("timeout");
    expect(t.connectTimeouts()).toEqual([500]);
    expect(t.recovers()).toBe(0);
  });

  it("a disconnect fails at once with no recovery and no second CONNECT", async () => {
    const t = new Scripted(["disconnect", "ok"]);
    await expect(settle(connect(t))).rejects.toMatchObject({ kind: "disconnected" });
    expect(t.connectTimeouts()).toEqual([500]);
    expect(t.recovers()).toBe(0);
  });
});

describe("memory read in several transfers (AC-VG37JA.3)", () => {
  it("reads the data after the 20-byte confirmation from further IN transfers", async () => {
    const t = new Scripted(["ok"], 64);
    const driver = await settle(connect(t));
    expect(driver.info).toMatchObject(DEMO_INFO);
    expect(driver.info.uid.length).toBeGreaterThan(0);
    // Each memory read was followed by at least one read() of its data.
    const reads = t.events.filter((e) => e.op === "read").length;
    expect(reads).toBeGreaterThanOrEqual(4);
    expect(driver.mode).toBe("queue");
    await settle(driver.disconnect());
  });

  it("keeps reading until it holds the whole rounded-up block (data in 16-byte pieces)", async () => {
    const t = new Scripted(["ok"], 16);
    const driver = await settle(connect(t));
    expect(driver.info).toMatchObject(DEMO_INFO);
    // Three 64-byte blocks in four pieces each, the 12-byte UID (16 rounded) in one.
    expect(t.events.filter((e) => e.op === "read").length).toBeGreaterThanOrEqual(13);
    await settle(driver.disconnect());
  });

  it("a confirmation that already carries the data in the same reply still decodes", async () => {
    const t = new Scripted(["ok"], null);
    const driver = await settle(connect(t));
    expect(driver.info).toMatchObject(DEMO_INFO);
    expect(driver.mode).toBe("queue");
    await settle(driver.disconnect());
  });
});

describe("regression: a fake USB meter that behaves like the real one (AC-VG37JA.6)", () => {
  /** The demo meter behind a FakeUsbDevice with the real meter's habits. */
  function realisticMeter(): FakeUsbDevice {
    const meter = new w.DemoMeter("load-5v-1a");
    let last = Date.now();
    let connects = 0;
    const dev = new FakeUsbDevice((frame) => {
      const now = Date.now();
      if (now > last) {
        meter.advance(now - last);
        last = now;
      }
      const name = classify(w, frame);
      // The first CONNECT after plug-in gets no reply.
      if (name === "connect" && connects++ === 0) return null;
      const reply = meter.transact(frame);
      if (reply.length === 0) return null;
      // Memory-read data comes in its own IN transfer after the confirmation.
      if (name.startsWith("memory_read:")) return [reply.slice(0, 20), reply.slice(20)];
      return reply;
    });
    // A DISCONNECT from before this session waits on bulk IN.
    dev.queueIn(new Uint8Array([0x03, 0x2a, 0x00, 0x00]));
    return dev;
  }

  it("connects, reads the device info and starts the queue", async () => {
    const dev = realisticMeter();
    const opened = await settle(
      openUsbOrOfferHid(
        dev as unknown as Parameters<typeof openUsbOrOfferHid>[0],
        {
          debug: false,
        } as unknown as Parameters<typeof openUsbOrOfferHid>[1],
      ),
    );
    if (!("transport" in opened)) throw new Error("expected the WebUSB transport");
    const driver = await settle(connect(opened.transport));
    expect(driver.info).toMatchObject(DEMO_INFO);
    expect(driver.authLevel).toBe(1);
    expect(driver.mode).toBe("queue");
    const names = sentNames(w, dev);
    expect(names.filter((n) => n === "connect")).toHaveLength(2);
    expect(names).toContain("start_queue:2");
    expect(dev.calls).toContain("clearHalt:out:5");
    expect(dev.calls).toContain("clearHalt:in:6");
    await settle(driver.disconnect());
  });
});

describe("regression: recovery cancels the pending read (AC-7T1YGJ.3)", () => {
  /** The 68-byte stale frame that answered the first CONNECT on web HIL run
   * WEB.hil2-20261108-2223 (W2 try 1); its id byte (0xd6) matches no request. */
  const STALE_68 = (() => {
    const f = new Uint8Array(68).fill(0x33);
    f.set([0x67, 0xd6, 0xa4, 0x86, 0xf5, 0x28, 0xd5, 0x8f]);
    return f;
  })();

  /** The demo meter behind a FakeUsbDevice: the first CONNECT gets only the stale
   * 68-byte frame, and clearing the IN halt cancels the transferIn still waiting. */
  function cancellingMeter(): FakeUsbDevice {
    const meter = new w.DemoMeter("load-5v-1a");
    let last = Date.now();
    let connects = 0;
    const dev = new FakeUsbDevice((frame) => {
      const now = Date.now();
      if (now > last) {
        meter.advance(now - last);
        last = now;
      }
      const name = classify(w, frame);
      if (name === "connect" && connects++ === 0) return STALE_68;
      const reply = meter.transact(frame);
      if (reply.length === 0) return null;
      if (name.startsWith("memory_read:")) return [reply.slice(0, 20), reply.slice(20)];
      return reply;
    });
    dev.clearHaltCancelsIn = true;
    return dev;
  }

  async function openAndConnect(dev: FakeUsbDevice): Promise<MeterDriver> {
    const opened = await settle(
      openUsbOrOfferHid(
        dev as unknown as Parameters<typeof openUsbOrOfferHid>[0],
        { debug: false } as unknown as Parameters<typeof openUsbOrOfferHid>[1],
      ),
    );
    if (!("transport" in opened)) throw new Error("expected the WebUSB transport");
    return settle(connect(opened.transport));
  }

  it("sends the second CONNECT after the recovery and signs in", async () => {
    const dev = cancellingMeter();
    const driver = await openAndConnect(dev);
    const names = sentNames(w, dev);
    expect(names.filter((n) => n === "connect")).toHaveLength(2);
    expect(dev.calls).toContain("clearHalt:in:6");
    // The second CONNECT went out after the IN halt was cleared.
    const cleared = dev.calls.indexOf("clearHalt:in:6");
    const outs = dev.calls.flatMap((c, i) => (c === "transferOut:5" ? [i] : []));
    expect(outs[1] ?? -1).toBeGreaterThan(cleared);
    expect(driver.info).toMatchObject(DEMO_INFO);
    expect(driver.authLevel).toBe(1);
    expect(driver.mode).toBe("queue");
    await settle(driver.disconnect());
  });

  it("still signs in when the halt clears themselves fail", async () => {
    const dev = cancellingMeter();
    dev.clearHaltError = new DOMException("The clear halt failed.", "InvalidStateError");
    const driver = await openAndConnect(dev);
    expect(sentNames(w, dev).filter((n) => n === "connect")).toHaveLength(2);
    expect(driver.info).toMatchObject(DEMO_INFO);
    expect(driver.authLevel).toBe(1);
    expect(driver.mode).toBe("queue");
    await settle(driver.disconnect());
  });
});
