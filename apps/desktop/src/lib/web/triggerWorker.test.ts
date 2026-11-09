/**
 * FR W8SHXC: the trigger engine's worker (`runTriggerWorker`), against a fake scope and
 * a fake engine module.
 *
 * - The `close` is answered even when stopping the engine throws.
 * - A second `open` is refused while an engine is live (or still opening): one engine
 *   per worker.
 * - The engine's clock is monotonic: `performance.timeOrigin + performance.now()`, not
 *   the settable `Date.now()`.
 * - Each port write is followed by a `{ wrote: true }` message, so the page drains the
 *   out-ring at once.
 * - A VBUS-watch error other than the closed session is reported as `{ failed }`; the
 *   closed session's refusal is not.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { UiError } from "../backend";
import { ByteRing, VbusCell, sessionClosed } from "./triggerBridge";
import {
  VBUS_CHECK_MS,
  monotonicNowMs,
  runTriggerWorker,
  type WorkerClock,
  type WorkerEngine,
  type WorkerGlue,
  type WorkerPort,
} from "./triggerWorker";

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["setInterval", "clearInterval", "Date"] });
});

afterEach(() => {
  vi.useRealTimers();
});

class FakeEngine implements WorkerEngine {
  freeError: unknown = null;
  vbusError: unknown = null;
  freed = 0;
  closes = 0;
  pushes = 0;
  constructor(
    readonly port: WorkerPort,
    readonly clock: WorkerClock,
  ) {}
  pushVbus(): void {
    this.pushes++;
    if (this.vbusError) throw this.vbusError;
  }
  close(): void {
    this.closes++;
  }
  free(): void {
    this.freed++;
    if (this.freeError) throw this.freeError;
  }
  detect(): unknown {
    return { entries: [] };
  }
}

interface Run {
  posted: unknown[];
  send(call: { id: number; method: string; args?: unknown[] }): void;
  engines: FakeEngine[];
  /** Lets the pending glue load finish. */
  loaded: () => void;
}

function run(o: { holdLoad?: boolean } = {}): Run {
  const posted: unknown[] = [];
  let listener: ((e: { data: unknown }) => void) | null = null;
  const engines: FakeEngine[] = [];
  const glue: WorkerGlue = {
    TriggerEngine: {
      open: (port, clock) => {
        const e = new FakeEngine(port, clock);
        engines.push(e);
        return e;
      },
    },
  };
  let loaded: () => void = () => {};
  const load = () =>
    o.holdLoad
      ? new Promise<WorkerGlue>((resolve) => {
          loaded = () => resolve(glue);
        })
      : Promise.resolve(glue);
  runTriggerWorker(
    {
      postMessage: (m) => posted.push(m),
      addEventListener: (_type, fn) => {
        listener = fn;
      },
    },
    load,
  );
  return {
    posted,
    engines,
    send: (call) => listener?.({ data: { args: [], ...call } }),
    loaded: () => loaded(),
  };
}

const channels = () => ({
  portIn: ByteRing.create(256).buffer,
  portOut: ByteRing.create(256).buffer,
  vbus: VbusCell.create().buffer,
});

async function opened(r: Run): Promise<FakeEngine> {
  r.send({ id: 1, method: "open", args: [channels()] });
  await vi.waitFor(() => expect(r.posted).toContainEqual({ id: 1, ok: null }));
  const engine = r.engines.at(-1);
  if (!engine) throw new Error("no engine");
  return engine;
}

describe("the worker's close", () => {
  it("is answered even when freeing the engine throws", async () => {
    const r = run();
    const engine = await opened(r);
    engine.freeError = new Error("already freed");
    r.send({ id: 2, method: "close" });
    expect(r.posted).toContainEqual({ id: 2, ok: null });
    expect(engine.closes).toBe(1);
    // Every later call says the session is closed.
    r.send({ id: 3, method: "detect" });
    expect(r.posted).toContainEqual({ id: 3, error: sessionClosed() });
  });
});

describe("a second open", () => {
  it("is refused while an engine is live, which keeps running", async () => {
    const r = run();
    const engine = await opened(r);
    r.send({ id: 2, method: "open", args: [channels()] });
    const answer = r.posted.find((m) => (m as { id?: number }).id === 2) as { error: UiError };
    expect(answer.error.kind).toBe("usage");
    expect(r.engines).toHaveLength(1);
    expect(engine.freed).toBe(0);
    r.send({ id: 3, method: "detect" });
    expect(r.posted).toContainEqual({ id: 3, ok: { entries: [] } });
  });

  it("is refused while the first is still opening", async () => {
    const r = run({ holdLoad: true });
    r.send({ id: 1, method: "open", args: [channels()] });
    r.send({ id: 2, method: "open", args: [channels()] });
    const second = r.posted.find((m) => (m as { id?: number }).id === 2) as { error: UiError };
    expect(second.error.kind).toBe("usage");
    r.loaded();
    await vi.waitFor(() => expect(r.posted).toContainEqual({ id: 1, ok: null }));
    expect(r.engines).toHaveLength(1);
  });
});

describe("the engine's clock", () => {
  it("is performance.timeOrigin + performance.now(), not Date.now()", async () => {
    // The system clock is set far back: a Date.now() clock would jump with it.
    vi.setSystemTime(946_684_800_000);
    const r = run();
    const engine = await opened(r);
    const before = performance.timeOrigin + performance.now();
    const now = engine.clock.nowMs();
    const after = monotonicNowMs();
    expect(now).toBeGreaterThanOrEqual(before);
    expect(now).toBeLessThanOrEqual(after);
    expect(Math.abs(now - Date.now())).toBeGreaterThan(1_000_000);
  });
});

describe("port writes", () => {
  it("each write posts { wrote: true } after the bytes are in the out-ring", async () => {
    const r = run();
    const ch = channels();
    r.send({ id: 1, method: "open", args: [ch] });
    await vi.waitFor(() => expect(r.posted).toContainEqual({ id: 1, ok: null }));
    const engine = r.engines[0] as FakeEngine;
    const out = new ByteRing(ch.portOut);
    const before = r.posted.length;
    engine.port.write(new TextEncoder().encode("pd req=2\r\n"));
    expect(r.posted.slice(before)).toEqual([{ wrote: true }]);
    expect(new TextDecoder().decode(out.read())).toBe("pd req=2\r\n");
  });
});

describe("the VBUS watch", () => {
  it("reports a failure other than the closed session, once", async () => {
    const r = run();
    const engine = await opened(r);
    const lost: UiError = {
      kind: "disconnected",
      message: "The meter's serial port went away.",
      hint: "Plug the meter back in.",
    };
    engine.vbusError = lost;
    vi.advanceTimersByTime(VBUS_CHECK_MS * 3);
    const failed = r.posted.filter((m) => "failed" in (m as object));
    expect(failed).toEqual([{ failed: lost }]);
    // The watch stopped: no further pushes.
    expect(engine.pushes).toBe(1);
  });

  it("stays quiet about the closed session's refusal", async () => {
    const r = run();
    const engine = await opened(r);
    engine.vbusError = sessionClosed();
    vi.advanceTimersByTime(VBUS_CHECK_MS * 3);
    expect(r.posted.some((m) => "failed" in (m as object))).toBe(false);
    expect(engine.pushes).toBe(3);
  });
});
