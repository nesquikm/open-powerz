/**
 * FR W8SHX9: the CDC pipe a settings write runs over.
 *
 * - AC-W8SHX9.6: `CdcPipe` sends only GET_DATA SETTINGS reads and the type-0x0A
 *   settings write; every other frame (CONNECT, other GET_DATA reads, queue and PD
 *   control, memory reads, and the ADCNEW control frames 0x01..0x03) is refused before
 *   anything is written.
 * - Replies are framed by the wasm core's CDC framing (`cdcFrameLength`): a reply split
 *   over several chunks comes back whole, bytes of a following frame wait for the next
 *   `read`, and a missing reply rejects as `timeout`.
 * - `DemoCdcTransport` is the demo meter's CDC twin behind `WebTransport`: it shares the
 *   demo meter's settings.
 *
 * Runs on Vitest's fake clock against an in-memory link and the wasm demo meter.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { UiError } from "../backend";
import { CdcPipe, DemoCdcTransport } from "./cdcTransport";
import { DemoTransport } from "./demoTransport";
import type { PowerzWasm } from "./driver";
import { demoSettingsReply, demoWriteFrame, withId, writeWasm } from "./fakeSerial.test";
import { loadWasm } from "./testWasm.test";
import { timeoutError } from "./transport";
import { settle } from "./webBackendFakes.test";

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

async function rejection(p: Promise<unknown>): Promise<UiError> {
  try {
    await settle(p);
  } catch (e) {
    return e as UiError;
  }
  throw new Error("the call resolved");
}

/** An in-memory serial link: each write gets the responder's chunks. */
class FakeLink {
  written: Uint8Array[] = [];
  closed = false;
  private pending: Uint8Array[] = [];
  constructor(public respond: (frame: Uint8Array) => Uint8Array[] = () => []) {}
  async write(bytes: Uint8Array): Promise<void> {
    this.written.push(bytes.slice());
    this.pending.push(...this.respond(bytes));
  }
  async read(timeoutMs: number): Promise<Uint8Array> {
    const next = this.pending.shift();
    if (next) return next;
    await new Promise((r) => setTimeout(r, timeoutMs));
    throw timeoutError();
  }
  async close(): Promise<void> {
    this.closed = true;
  }
}

type Link = ConstructorParameters<typeof CdcPipe>[1];
const pipeOver = (link: FakeLink) => new CdcPipe(w, link as unknown as Link);

describe("the frame allowlist (AC-W8SHX9.6)", () => {
  it("refuses every frame but a SETTINGS read and a settings write, sending nothing", async () => {
    const link = new FakeLink((f) => [withId(new Uint8Array([0x05, 0, 0, 0]), f[1] ?? 0)]);
    const pipe = pipeOver(link);
    const refused: [string, Uint8Array][] = [
      ["connect", w.connectRequest(1)],
      ["GET_DATA ADC", w.getDataRequest(2, 0x001)],
      ["GET_DATA queue", w.getDataRequest(3, 0x002)],
      ["GET_DATA record list", w.getDataRequest(4, 0x200)],
      ["start queue", w.startQueueRequest(5, 2)],
      ["stop queue", w.stopQueueRequest(6)],
      ["start PD", w.startPdSnifferRequest(7)],
      ["PD poll", w.pdPollRequest(8, true)],
      ["memory read", w.memoryReadRequest(9, 0x4001_0450, 12)],
      ["ADCNEW 0x01", new Uint8Array([0x01])],
      ["ADCNEW 0x02", new Uint8Array([0x02, 0x01])],
      ["ADCNEW 0x03", new Uint8Array([0x03])],
      ["delete records (0x48)", new Uint8Array([0x48, 0x0a, 0x28, 0x00])],
    ];
    for (const [what, frame] of refused) {
      const e = await rejection(pipe.transact(frame, 500));
      expect(e.kind, what).toBe("usage");
      expect(e.message, what).toMatch(/not safe/i);
    }
    expect(link.written).toEqual([]);
  });

  it("sends a SETTINGS read and a settings write", async () => {
    const settings = demoSettingsReply(w);
    const link = new FakeLink((f) =>
      f[0] === 0x0a ? [new Uint8Array([0x07, f[1] ?? 0, 0, 0])] : [withId(settings, f[1] ?? 0)],
    );
    const pipe = pipeOver(link);
    const read = w.settingsRequest(3);
    expect([...(await settle(pipe.transact(read, 1_000)))]).toEqual([...withId(settings, 3)]);
    const write = demoWriteFrame(w, { brightness: 80 }, 4);
    expect(write[0]).toBe(0x0a);
    expect([...(await settle(pipe.transact(write, 1_000)))]).toEqual([0x07, 4, 0, 0]);
    expect(link.written).toEqual([read, write]);
  });
});

describe("CDC framing", () => {
  it("joins a reply split over several chunks", async () => {
    const settings = demoSettingsReply(w);
    const link = new FakeLink((f) => {
      const r = withId(settings, f[1] ?? 0);
      return [r.slice(0, 1), r.slice(1, 9), r.slice(9, 50), r.slice(50)];
    });
    const reply = await settle(pipeOver(link).transact(w.settingsRequest(5), 1_000));
    expect(reply.length).toBe(settings.length);
    expect(writeWasm(w).cdcFrameLength(reply)).toBe(settings.length);
    expect(w.decodeSettings(reply).fields).toEqual(w.decodeSettings(settings).fields);
  });

  it("keeps the bytes of a following frame for the next read", async () => {
    const settings = demoSettingsReply(w);
    const link = new FakeLink((f) => {
      const r = withId(settings, f[1] ?? 0);
      const joined = new Uint8Array(r.length + 4);
      joined.set(r);
      joined.set([0x05, 0x77, 0x00, 0x00], r.length);
      return [joined];
    });
    const pipe = pipeOver(link);
    const first = await settle(pipe.transact(w.settingsRequest(6), 1_000));
    expect(first.length).toBe(settings.length);
    expect([...(await settle(pipe.read(1_000)))]).toEqual([0x05, 0x77, 0x00, 0x00]);
  });

  it("an incomplete reply times out as `timeout`", async () => {
    const settings = demoSettingsReply(w);
    const link = new FakeLink((f) => [withId(settings, f[1] ?? 0).slice(0, 40)]);
    const e = await rejection(pipeOver(link).transact(w.settingsRequest(7), 1_000));
    expect(e.kind).toBe("timeout");
  });

  it("no reply at all times out as `timeout`", async () => {
    const e = await rejection(pipeOver(new FakeLink()).transact(w.settingsRequest(8), 1_000));
    expect(e.kind).toBe("timeout");
  });

  it("close closes the link", async () => {
    const link = new FakeLink();
    await settle(pipeOver(link).close());
    expect(link.closed).toBe(true);
  });
});

describe("the demo meter's CDC twin", () => {
  it("a write over the twin is what the demo meter serves afterwards", async () => {
    const demo = new DemoTransport(w, "load-5v-1a", () => Date.now());
    const twin = new DemoCdcTransport(demo, () => Date.now());
    const sw = new (writeWasm(w).SettingsWrite)({ brightness: 42 });
    sw.takeRead(await settle(twin.transact(sw.readRequest(1), 1_000)));
    sw.takeRead(await settle(twin.transact(sw.readRequest(2), 1_000)));
    sw.takeWriteReply(
      await settle(twin.transact(sw.writeRequest(3), 1_000)).catch(() => undefined),
    );
    sw.takeRead(await settle(twin.transact(sw.readRequest(4), 1_000)));
    const written = sw.result();
    sw.free();
    const served = w.decodeSettings(await settle(demo.transact(w.settingsRequest(5), 1_000)));
    expect(served.fields).toEqual(written.fields);
    expect(served.fields.find((f) => f.key === "brightness")?.value).toContain("42");
    await settle(twin.close());
    await settle(demo.close());
  });

  it("the twin refuses the same frames as the serial pipe", async () => {
    const demo = new DemoTransport(w, "load-5v-1a", () => Date.now());
    const twin = new DemoCdcTransport(demo, () => Date.now());
    const e = await rejection(twin.transact(w.startQueueRequest(1, 2), 500));
    expect(e.kind).toBe("usage");
    await settle(twin.close());
    await settle(demo.close());
  });
});

describe("stale bytes (FR W8SHX9 review)", () => {
  it("drops a reply left from a timed-out request before sending the next frame", async () => {
    const stale = withId(demoSettingsReply(w), 7);
    const fresh = withId(demoSettingsReply(w), 8);
    let held: Uint8Array[] = [stale];
    const link = {
      written: [] as Uint8Array[],
      async write(bytes: Uint8Array) {
        this.written.push(bytes);
        held.push(fresh);
      },
      async read(): Promise<Uint8Array> {
        const next = held.shift();
        if (!next) throw timeoutError();
        return next;
      },
      async close() {},
      drain() {
        held = [];
      },
    };
    const pipe = new CdcPipe(writeWasm(w), link);
    const request = new Uint8Array([0x0c, 8, 0x10, 0x00]);
    const reply = await settle(pipe.transact(request, 1_000));
    expect(reply[1]).toBe(8);
  });
});
