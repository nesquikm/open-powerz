/**
 * FR QFTCWZ, AC-QFTCWZ.1: the demo-meter transport wraps the wasm `DemoMeter` and
 * advances its clock by the wall time that passed between calls.
 */

import { describe, expect, it } from "vitest";
import { DemoTransport } from "./demoTransport";
import { loadWasm } from "./testWasm.test";

describe("demo transport (AC-QFTCWZ.1)", () => {
  it("answers CONNECT with an ACCEPT", async () => {
    const w = await loadWasm();
    const t = new DemoTransport(w, "load-5v-1a", () => 0);
    const reply = await t.transact(w.connectRequest(1), 100);
    expect(reply[0]).toBe(0x05);
    expect(() => w.decodeAccept(reply)).not.toThrow();
    await t.close();
  });

  it("advances the meter by the wall time between calls", async () => {
    const w = await loadWasm();
    let now = 1_000;
    const t = new DemoTransport(w, "load-5v-1a", () => now);
    w.decodeAccept(await t.transact(w.connectRequest(1), 100));
    const uid = w.decodeMemoryRead(
      await t.transact(w.memoryReadRequest(2, 0x4001_0450, 12), 100),
      0x4001_0450,
      12,
    );
    const stamp = 1_790_000_000_000n;
    w.decodeAuthReply(await t.transact(w.authRequest(3, 1, uid, stamp, 1n), 100), uid, stamp, 1n);
    w.decodeAccept(await t.transact(w.startQueueRequest(4, 2), 100));

    // No wall time passed: the queue holds (almost) nothing.
    const none = w.decodeQueue(await t.transact(w.getDataRequest(5, 0x002), 100), 2);
    expect(none.length).toBeLessThanOrEqual(1);

    now += 1_000; // one second at 50 SPS
    const second = w.decodeQueue(await t.transact(w.getDataRequest(6, 0x002), 100), 2);
    expect(second.length).toBeGreaterThanOrEqual(45);
    expect(second.length).toBeLessThanOrEqual(55);
    expect(second[0]?.vbus_v).toBeCloseTo(5, 0);
  });

  it("rejects an unknown scenario", async () => {
    const w = await loadWasm();
    expect(() => new DemoTransport(w, "no-such-scenario", () => 0)).toThrow();
  });

  it("read() with nothing pending is a timeout", async () => {
    const w = await loadWasm();
    const t = new DemoTransport(w, "idle", () => 0);
    await expect(t.read(20)).rejects.toMatchObject({ kind: "timeout" });
  });

  it("after close() a transfer is disconnected", async () => {
    const w = await loadWasm();
    const t = new DemoTransport(w, "idle", () => 0);
    await t.close();
    await expect(t.transact(w.connectRequest(1), 100)).rejects.toMatchObject({
      kind: "disconnected",
    });
  });
});
