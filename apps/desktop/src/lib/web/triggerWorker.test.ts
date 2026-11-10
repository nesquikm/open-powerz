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
 * - AC-AHTZW1.4: the worker opens the engine with a log callback as its fourth argument
 *   (`TriggerEngine.open(port, clock, vbus, log)`) and posts each line to the page as
 *   `{ log: line }`, the lines the engine logs while it opens (`pdm open`) included; the
 *   real wasm engine hands its session log to that callback.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { UiError } from "../backend";
import { loadWasm } from "./testWasm.test";
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
    readonly log: unknown = undefined,
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
      open: (...args: unknown[]) => {
        const [port, clock, , log] = args as [WorkerPort, WorkerClock, unknown, unknown];
        // The engine logs `pdm open` while it opens, before it is returned.
        if (typeof log === "function") (log as (line: string) => void)("trigger sent: pdm open");
        const e = new FakeEngine(port, clock, log);
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

describe("the session log (AC-AHTZW1.4)", () => {
  it("opens the engine with a log callback and posts every line to the page", async () => {
    const r = run();
    const engine = await opened(r);
    expect(typeof engine.log).toBe("function");
    const log = engine.log as (line: string) => void;
    const lines = [
      "trigger sent: entry pd",
      "pd offers: 5V 3A, 9V 3A, 15V 3A, 20V 3.25A",
      "trigger armed ended: back to 5 V",
    ];
    for (const line of lines) log(line);
    const logged = r.posted
      .filter((m) => typeof m === "object" && m !== null && "log" in m)
      .map((m) => (m as { log: unknown }).log);
    // The line logged while the engine opened comes first, then each in order.
    expect(logged).toEqual(["trigger sent: pdm open", ...lines]);
  });

  it("the real wasm engine hands its session log to the callback", async () => {
    const wasm = (await loadWasm()) as unknown as {
      TriggerEngine: {
        open(
          port: WorkerPort,
          clock: WorkerClock,
          vbus: null,
          log: (line: string) => void,
        ): { list(protocol: string): unknown; free(): void };
      };
    };
    let t = 1_790_000_000_000;
    let pending: Uint8Array = new Uint8Array(0);
    const written: string[] = [];
    const replies: Record<string, string> = {
      "pdm open": "pdm mode entry\nver1.0\n",
      "entry qc": "ok\nready\n",
    };
    const port: WorkerPort = {
      read: (timeoutMs) => {
        if (pending.length === 0) {
          t += Math.max(1, timeoutMs);
          return new Uint8Array(0);
        }
        const out = pending;
        pending = new Uint8Array(0);
        return out;
      },
      write: (bytes) => {
        const cmd = new TextDecoder().decode(bytes).trim();
        written.push(cmd);
        const reply = replies[cmd];
        if (reply) pending = new TextEncoder().encode(reply);
      },
    };
    const clock: WorkerClock = {
      nowMs: () => t,
      sleep: (ms) => {
        t += ms;
      },
    };
    const lines: string[] = [];
    const engine = wasm.TriggerEngine.open(port, clock, null, (line) => lines.push(line));
    try {
      expect(written[0]).toBe("pdm open");
      engine.list("qc");
      expect(lines.some((l) => l.includes("pdm open"))).toBe(true);
      expect(lines.some((l) => l.includes("entry qc"))).toBe(true);
    } finally {
      engine.free();
    }
  });
});
