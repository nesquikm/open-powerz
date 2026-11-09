/**
 * FR W8SHXC: the pure parts of the browser trigger bridge.
 *
 * The trigger engine (wasm) runs in a dedicated worker with a blocking port; the page
 * owns the Web Serial port and moves the bytes through shared memory:
 *
 * - `ByteRing`: a byte FIFO over a `SharedArrayBuffer`, one writer and one reader on
 *   either side of the worker boundary. `write` takes what fits and says how much;
 *   `read` takes what is there (empty when nothing); `readBlocking(timeoutMs)` waits
 *   with `Atomics.wait` until bytes arrive or the timeout passes (empty then), and
 *   returns at once once the ring is closed. A second `ByteRing` over the same buffer
 *   (the other side) sees the same bytes.
 * - `VbusCell`: the session's latest VBUS reading in shared memory, set by the page from
 *   each stream batch and read by the worker's VBUS watch (AC-W8SHXC.5).
 * - `RpcClient` / `answerRpc`: the page's promise calls `{ id, method, args }` and the
 *   worker's `{ id, ok }` / `{ id, error }` answers; an engine error keeps its
 *   `{ kind, message, hint }` (AC-W8SHXC.1: the same errors).
 *
 * `SharedArrayBuffer` and `Atomics` exist in Node, so this runs without a worker.
 */

import { describe, expect, it } from "vitest";
import { ByteRing, RpcClient, VbusCell, answerRpc, type RpcCall } from "./triggerBridge";

const bytes = (...b: number[]) => Uint8Array.from(b);
const text = (s: string) => new TextEncoder().encode(s);

describe("ByteRing", () => {
  it("lives in a SharedArrayBuffer", () => {
    const ring = ByteRing.create(64);
    expect(ring.buffer).toBeInstanceOf(SharedArrayBuffer);
    expect(ring.capacity).toBe(64);
    expect(ring.size).toBe(0);
  });

  it("gives back what was written, in order", () => {
    const ring = ByteRing.create(64);
    expect(ring.write(text("pdm open\r\n"))).toBe(10);
    expect(ring.size).toBe(10);
    expect(new TextDecoder().decode(ring.read())).toBe("pdm open\r\n");
    expect(ring.size).toBe(0);
    expect(ring.read()).toEqual(new Uint8Array(0));
  });

  it("keeps order across the wrap-around", () => {
    const ring = ByteRing.create(8);
    expect(ring.write(bytes(1, 2, 3, 4, 5, 6))).toBe(6);
    expect(ring.read(4)).toEqual(bytes(1, 2, 3, 4));
    expect(ring.write(bytes(7, 8, 9, 10, 11, 12))).toBe(6);
    expect(ring.read()).toEqual(bytes(5, 6, 7, 8, 9, 10, 11, 12));
  });

  it("takes only what fits when full", () => {
    const ring = ByteRing.create(8);
    expect(ring.write(bytes(1, 2, 3, 4, 5))).toBe(5);
    expect(ring.write(bytes(6, 7, 8, 9, 10))).toBe(3);
    expect(ring.read()).toEqual(bytes(1, 2, 3, 4, 5, 6, 7, 8));
  });

  it("is shared: the other side's ring over the same buffer reads the writes", () => {
    const page = ByteRing.create(32);
    const worker = new ByteRing(page.buffer);
    page.write(text("ok\r\n"));
    expect(worker.size).toBe(4);
    expect(worker.read()).toEqual(text("ok\r\n"));
    expect(page.size).toBe(0);
  });

  it("readBlocking returns at once when bytes are there", () => {
    const ring = ByteRing.create(32);
    ring.write(bytes(9, 8, 7));
    const start = performance.now();
    expect(ring.readBlocking(1_000)).toEqual(bytes(9, 8, 7));
    expect(performance.now() - start).toBeLessThan(200);
  });

  it("readBlocking waits out its timeout and gives nothing on a quiet port", () => {
    const ring = ByteRing.create(32);
    const start = performance.now();
    expect(ring.readBlocking(40)).toEqual(new Uint8Array(0));
    const waited = performance.now() - start;
    expect(waited).toBeGreaterThanOrEqual(30);
    expect(waited).toBeLessThan(1_000);
  });

  it("a closed ring wakes its reader with nothing", () => {
    const ring = ByteRing.create(32);
    ring.close();
    expect(ring.closed).toBe(true);
    expect(new ByteRing(ring.buffer).closed).toBe(true);
    const start = performance.now();
    expect(ring.readBlocking(5_000)).toEqual(new Uint8Array(0));
    expect(performance.now() - start).toBeLessThan(500);
  });
});

describe("VbusCell (AC-W8SHXC.5)", () => {
  it("holds nothing until the page sets a reading", () => {
    expect(VbusCell.create().get()).toBeNull();
  });

  it("the worker side reads the page's latest reading", () => {
    const page = VbusCell.create();
    expect(page.buffer).toBeInstanceOf(SharedArrayBuffer);
    const worker = new VbusCell(page.buffer);
    page.set({ volts: 9.01, cc1_v: 1.62, cc2_v: 0.01, time_ms: 1_234, period_ms: 20 });
    page.set({ volts: 0.42, cc1_v: 1.6, cc2_v: 0.02, time_ms: 1_254, period_ms: 20 });
    const r = worker.get();
    expect(r).not.toBeNull();
    expect(r?.volts).toBeCloseTo(0.42, 6);
    expect(r?.cc1_v).toBeCloseTo(1.6, 6);
    expect(r?.cc2_v).toBeCloseTo(0.02, 6);
    expect(r?.time_ms).toBe(1_254);
    expect(r?.period_ms).toBe(20);
  });
});

describe("the RPC between page and worker", () => {
  it("answers each call by its id", async () => {
    const posted: { id: number; method: string; args: unknown[] }[] = [];
    const client = new RpcClient((m) => posted.push(m));
    const a = client.call("list", ["pd"]);
    const b = client.call("risk", [{ protocol: "qc", volts: 9 }]);
    expect(posted.map((m) => m.method)).toEqual(["list", "risk"]);
    expect(posted[0]?.args).toEqual(["pd"]);
    expect(posted[0]?.id).not.toBe(posted[1]?.id);
    client.receive({ id: posted[1]?.id ?? -1, ok: "above_5v" });
    client.receive({ id: posted[0]?.id ?? -1, ok: { protocol: "pd", supported: true } });
    await expect(a).resolves.toEqual({ protocol: "pd", supported: true });
    await expect(b).resolves.toBe("above_5v");
  });

  it("an engine error comes back with its kind, message and hint", async () => {
    const posted: { id: number }[] = [];
    const client = new RpcClient((m) => posted.push(m));
    const p = client.call("request", [{ protocol: "pd", position: 2 }, false, false]);
    client.receive({
      id: posted[0]?.id ?? -1,
      error: { kind: "timeout", message: "The meter did not answer in time.", hint: "Check." },
    });
    await expect(p).rejects.toEqual({
      kind: "timeout",
      message: "The meter did not answer in time.",
      hint: "Check.",
    });
  });

  it("failAll rejects every pending call (the worker went away)", async () => {
    const client = new RpcClient(() => {});
    const a = client.call("detect", []);
    const b = client.call("armed", []);
    client.failAll({ kind: "disconnected", message: "gone", hint: null });
    await expect(a).rejects.toMatchObject({ kind: "disconnected" });
    await expect(b).rejects.toMatchObject({ kind: "disconnected" });
  });

  it("the worker side calls the engine method with the args", () => {
    const calls: unknown[][] = [];
    const engine = {
      request: (...args: unknown[]) => {
        calls.push(args);
        return { command: "pd req=2" };
      },
    };
    const reply = answerRpc(engine, {
      id: 7,
      method: "request",
      args: [{ protocol: "pd", position: 2 }, true, true],
    });
    expect(calls).toEqual([[{ protocol: "pd", position: 2 }, true, true]]);
    expect(reply).toEqual({ id: 7, ok: { command: "pd req=2" } });
  });

  it("the worker side answers a thrown binding error as that error", () => {
    const engine = {
      risk: () => {
        throw { kind: "usage", message: "The charger went away. Press Detect.", hint: null };
      },
    };
    expect(answerRpc(engine, { id: 3, method: "risk", args: [] })).toEqual({
      id: 3,
      error: { kind: "usage", message: "The charger went away. Press Detect.", hint: null },
    });
  });

  it("the worker side refuses a method the engine does not have", () => {
    const reply = answerRpc({}, { id: 4, method: "flashFirmware", args: [] });
    expect(reply.id).toBe(4);
    expect(reply.error?.kind).toBe("internal");
    expect("ok" in reply).toBe(false);
  });
});

/**
 * FR K4DWCF: a real `Worker.postMessage` structured-clones its message and throws a
 * `DataCloneError` on a proxy, and the panel's values come from Svelte's deep `$state`
 * (proxies). The client posts a plain deep copy of the call's arguments; a value that
 * cannot be copied at all fails that one call and leaves the client usable.
 */
describe("the RPC client posts plain copies of reactive values (FR K4DWCF)", () => {
  /** A post that clones as `Worker.postMessage` does. */
  const cloningPost = (posted: RpcCall[]) => (m: RpcCall) => {
    posted.push(structuredClone(m));
  };

  const PD_9V = { protocol: "pd", position: 2, voltage_mv: null, current_ma: null };

  it("AC-K4DWCF.1: a call holding a reactive-state proxy is posted and answered", async () => {
    const posted: RpcCall[] = [];
    const client = new RpcClient(cloningPost(posted));
    const reactive = new Proxy({ ...PD_9V }, {});
    const p = client.call("request", [reactive, true, false]);
    p.catch(() => {}); // awaited below; keeps an early failure from going unhandled
    expect(posted).toHaveLength(1);
    expect(posted[0]?.method).toBe("request");
    client.receive({ id: posted[0]?.id ?? -1, ok: { command: "pd req=2" } });
    await expect(p).resolves.toEqual({ command: "pd req=2" });
  });

  it("AC-K4DWCF.2: the posted args are plain values equal to the ones passed", async () => {
    const posted: RpcCall[] = [];
    const client = new RpcClient((m) => posted.push(m));
    const request = Proxy.revocable({ ...PD_9V }, {});
    const message = Proxy.revocable(
      { sop: "sop", msg_type: 2, data_objects: Proxy.revocable([0x1234, 0x5678], {}).proxy },
      {},
    );
    void client.call("request", [request.proxy, true, false]);
    void client.call("sendMessage", [message.proxy, true]);
    void client.call("consoleSend", ["pd req=2", true]);
    // Once the reactive state is gone, what was posted must still read as plain values.
    request.revoke();
    message.revoke();
    expect(posted.map((m) => m.args)).toEqual([
      [PD_9V, true, false],
      [{ sop: "sop", msg_type: 2, data_objects: [0x1234, 0x5678] }, true],
      ["pd req=2", true],
    ]);
    // And they clone as a real worker's postMessage needs.
    expect(structuredClone(posted)).toEqual(posted);
  });

  it("AC-K4DWCF.2: shared memory in a call still reaches the worker as shared memory", () => {
    const posted: RpcCall[] = [];
    const client = new RpcClient((m) => posted.push(m));
    const portIn = new SharedArrayBuffer(32);
    void client.call("open", [{ portIn }]);
    const sent = (posted[0]?.args[0] as { portIn: SharedArrayBuffer }).portIn;
    expect(sent).toBeInstanceOf(SharedArrayBuffer);
    new Uint8Array(sent)[0] = 7;
    expect(new Uint8Array(portIn)[0]).toBe(7);
  });

  it("AC-K4DWCF.3: a value that cannot be copied fails that call; the client still works", async () => {
    const posted: RpcCall[] = [];
    const client = new RpcClient(cloningPost(posted));
    const bad = client.call("request", [{ ...PD_9V, onDone: () => {} }, true, false]);
    const e = await bad.then(
      () => null,
      (err: unknown) => err as { kind: string; message: string },
    );
    expect(e).not.toBeNull();
    expect(typeof e?.kind).toBe("string");
    expect(typeof e?.message).toBe("string");
    expect(posted).toEqual([]);
    const next = client.call("armed", []);
    expect(posted).toHaveLength(1);
    client.receive({ id: posted[0]?.id ?? -1, ok: true });
    await expect(next).resolves.toBe(true);
  });
});
