/**
 * FR W8SHXC: a real meter's trigger session over its lifetime (`Triggers`), against a
 * fake Web Serial port and a fake engine worker.
 *
 * - The byte pump: the page drains the worker's out-ring when the worker posts a
 *   message (it posts `{ wrote: true }` after each write), with no timer needed; a slow
 *   fallback timer still drains it without a message.
 * - A dead worker (`error`, `messageerror`, or a reported `{ failed }`) rejects every
 *   pending and later call with a `disconnected` error, asks the engine to go back to
 *   5 V and close (best effort), and lets the serial port go.
 * - The open waits at most `OPEN_WAIT_MS` for the worker: past it the worker is
 *   terminated, the port let go and `triggerOpen` refused; the port is not held after.
 * - `closeFor` keeps the port counted as held until the close has finished.
 * - An open whose meter went away meanwhile closes the new session and refuses.
 * - While the last accepted contract is above 5 V, `beforeunload` asks before the tab
 *   closes; back at 5 V or closed, it does not.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { TriggerRequest, UiError } from "../backend";
import { FakeSerial, FakeSerialPort } from "./fakeSerial.test";
import { ByteRing } from "./triggerBridge";
import { OPEN_WAIT_MS, Triggers, type TriggerStream } from "./webTrigger";
import { settle } from "./webBackendFakes.test";

beforeEach(() => {
  vi.useFakeTimers({
    toFake: ["setTimeout", "clearTimeout", "setInterval", "clearInterval", "Date"],
  });
  vi.setSystemTime(1_790_000_000_000);
});

afterEach(() => {
  vi.useRealTimers();
});

type Posted = { id: number; method: string; args: unknown[] };
type Listener = (e: { data?: unknown }) => void;
type Answer = { ok: unknown } | { error: UiError } | "hold";

class LifecycleWorker {
  posted: Posted[] = [];
  terminated = false;
  answers: Record<string, (args: unknown[]) => Answer> = {};
  portOut: ByteRing | null = null;
  private held: (() => void)[] = [];
  private listeners: Record<string, Set<Listener>> = {};

  postMessage(data: unknown): void {
    const m = data as Posted;
    this.posted.push(m);
    if (m.method === "open") {
      const ch = m.args[0] as { portOut: SharedArrayBuffer };
      this.portOut = new ByteRing(ch.portOut);
    }
    const answer = this.answers[m.method]?.(m.args) ?? { ok: null };
    const send = () => this.emit("message", { id: m.id, ...(answer as object) });
    if (answer === "hold") this.held.push(() => this.emit("message", { id: m.id, ok: null }));
    else void Promise.resolve().then(send);
  }

  /** Answer every held call `ok`. */
  release(): void {
    for (const a of this.held.splice(0)) a();
  }

  emit(type: string, data?: unknown): void {
    for (const l of this.listeners[type] ?? []) l({ data });
  }

  addEventListener(type: string, fn: Listener): void {
    (this.listeners[type] ??= new Set()).add(fn);
  }

  terminate(): void {
    this.terminated = true;
  }

  methods(): string[] {
    return this.posted.map((p) => p.method);
  }
}

const ID = "webusb:1";

interface Run {
  triggers: Triggers;
  commands: ReturnType<Triggers["commands"]>;
  port: FakeSerialPort;
  workers: LifecycleWorker[];
  page: EventTarget;
  /** Whether the meter's measurement session is there. */
  meter: { present: boolean };
}

function run(o: { answers?: Record<string, (args: unknown[]) => Answer> } = {}): Run {
  const port = new FakeSerialPort();
  const serial = new FakeSerial();
  serial.pick = port;
  const workers: LifecycleWorker[] = [];
  const page = new EventTarget();
  const meter = { present: true };
  const stream: TriggerStream = {
    rateSps: 50,
    pdStopForTrigger: async () => {},
    setRate: async () => {},
    holdPolling: () => {},
  };
  const triggers = new Triggers({
    module: () => Promise.reject(new Error("no demo here")),
    webSerial: serial as never,
    isolated: true,
    startWorker: () => {
      const w = new LifecycleWorker();
      Object.assign(w.answers, o.answers ?? {});
      workers.push(w);
      return w;
    },
    pageEvents: page,
    stream: () => (meter.present ? stream : null),
  });
  return { triggers, commands: triggers.commands(() => ID), port, workers, page, meter };
}

async function rejection(p: Promise<unknown>, maxMs = 20_000): Promise<UiError> {
  try {
    await settle(p, maxMs);
  } catch (e) {
    return e as UiError;
  }
  throw new Error("expected a rejection");
}

/** Let promise callbacks run without moving the clock. */
async function flush(): Promise<void> {
  for (let i = 0; i < 50; i++) await Promise.resolve();
}

const text = (port: FakeSerialPort) =>
  new TextDecoder().decode(Uint8Array.from(port.frames().flatMap((f) => [...f])));

const pd = (position: number): TriggerRequest =>
  ({ protocol: "pd", position, voltage_mv: null, current_ma: null }) as TriggerRequest;

const reply = (mv: number | null) => ({
  ok: {
    command: "pd req=2",
    requested_mv: mv,
    requested_ma: 3000,
    delivered_mv: null,
    delivered_ma: null,
    reply: null,
  },
});

describe("the byte pump", () => {
  it("drains the out-ring on a worker message, with no timer", async () => {
    const r = run();
    await settle(r.commands.triggerOpen());
    const worker = r.workers[0] as LifecycleWorker;
    await vi.advanceTimersByTimeAsync(100);
    worker.portOut?.write(new TextEncoder().encode("pd req=2\r\n"));
    worker.emit("message", { wrote: true });
    await flush();
    expect(text(r.port)).toContain("pd req=2\r\n");
    await settle(r.commands.triggerClose());
  });

  it("a slow fallback timer drains it without a message", async () => {
    const r = run();
    await settle(r.commands.triggerOpen());
    const worker = r.workers[0] as LifecycleWorker;
    await vi.advanceTimersByTimeAsync(100);
    worker.portOut?.write(new TextEncoder().encode("pd req=3\r\n"));
    await flush();
    await vi.advanceTimersByTimeAsync(100);
    expect(text(r.port)).toContain("pd req=3\r\n");
    await settle(r.commands.triggerClose());
  });
});

describe("a dead worker", () => {
  for (const [what, kill] of [
    ["an error event", (w: LifecycleWorker) => w.emit("error")],
    ["a messageerror event", (w: LifecycleWorker) => w.emit("messageerror")],
    [
      "a reported failure",
      (w: LifecycleWorker) =>
        w.emit("message", {
          failed: { kind: "internal", message: "wasm trap", hint: null },
        }),
    ],
  ] as const) {
    it(`after ${what}: calls refuse, the engine is asked back to 5 V, the port is let go`, async () => {
      const r = run({ answers: { detect: () => "hold" } });
      await settle(r.commands.triggerOpen());
      const worker = r.workers[0] as LifecycleWorker;
      const pending = r.commands.triggerDetect();
      await flush();
      kill(worker);
      const first = await rejection(pending);
      expect(first.kind).toBe("disconnected");
      const later = await rejection(r.commands.triggerRisk(pd(2)));
      expect(later).toEqual(first);
      expect(worker.methods()).toEqual(expect.arrayContaining(["leaveTo5v", "close"]));
      expect(worker.methods().indexOf("leaveTo5v")).toBeLessThan(worker.methods().indexOf("close"));
      // No risk call reached the dead worker.
      expect(worker.methods()).not.toContain("risk");
      await vi.advanceTimersByTimeAsync(20_000);
      expect(r.port.isOpen).toBe(false);
      expect(worker.terminated).toBe(true);
      // Closing the dead session is quiet, and the port is free again after.
      await settle(r.commands.triggerClose());
      expect(r.triggers.holds(ID)).toBe(false);
    });
  }

  it("a reported disconnect keeps its own wording", async () => {
    const r = run();
    await settle(r.commands.triggerOpen());
    const worker = r.workers[0] as LifecycleWorker;
    const lost: UiError = {
      kind: "disconnected",
      message: "The meter's serial port went away.",
      hint: "Plug the meter back in.",
    };
    worker.emit("message", { failed: lost });
    expect(await rejection(r.commands.triggerDetect())).toEqual(lost);
    await settle(r.commands.triggerClose(), 20_000);
  });
});

describe("the open", () => {
  it("past OPEN_WAIT_MS stops the worker, lets the port go and refuses", async () => {
    const r = run({ answers: { open: () => "hold" } });
    const opening = r.commands.triggerOpen();
    await vi.advanceTimersByTimeAsync(OPEN_WAIT_MS - 500);
    expect(r.triggers.holds(ID)).toBe(true);
    const e = await rejection(opening);
    expect(e.kind).toBe("timeout");
    const worker = r.workers[0] as LifecycleWorker;
    expect(worker.terminated).toBe(true);
    expect(r.port.isOpen).toBe(false);
    expect(r.triggers.holds(ID)).toBe(false);
    // No close was asked of an engine that never opened.
    expect(worker.methods()).toEqual(["open"]);
  });

  it("a meter gone while opening closes the new session and refuses", async () => {
    const r = run();
    const opening = r.commands.triggerOpen();
    await flush();
    r.meter.present = false;
    const e = await rejection(opening);
    expect(e.kind).toBe("usage");
    const worker = r.workers[0] as LifecycleWorker;
    expect(worker.methods()).toContain("close");
    await vi.advanceTimersByTimeAsync(1_000);
    expect(r.port.isOpen).toBe(false);
    expect(r.triggers.holds(ID)).toBe(false);
  });
});

describe("closeFor", () => {
  it("keeps the port held until the close has finished", async () => {
    const r = run({ answers: { close: () => "hold" } });
    await settle(r.commands.triggerOpen());
    const worker = r.workers[0] as LifecycleWorker;
    const closing = r.triggers.closeFor(ID);
    await vi.advanceTimersByTimeAsync(1_000);
    expect(r.triggers.holds(ID)).toBe(true);
    worker.release();
    await settle(closing);
    expect(r.triggers.holds(ID)).toBe(false);
  });
});

describe("beforeunload above 5 V", () => {
  const asks = (page: EventTarget) => {
    const e = new Event("beforeunload", { cancelable: true });
    page.dispatchEvent(e);
    return e.defaultPrevented;
  };

  it("asks while the last contract is above 5 V, not at 5 V or after the close", async () => {
    let mv = 9_000;
    const r = run({ answers: { request: () => reply(mv) } });
    await settle(r.commands.triggerOpen());
    expect(asks(r.page)).toBe(false);
    await settle(r.commands.triggerRequest(pd(2), true));
    expect(asks(r.page)).toBe(true);
    await settle(r.commands.triggerBackTo5v());
    expect(asks(r.page)).toBe(false);
    await settle(r.commands.triggerRequest(pd(2), true));
    expect(asks(r.page)).toBe(true);
    mv = 5_000;
    await settle(r.commands.triggerRequest(pd(1), false));
    expect(asks(r.page)).toBe(false);
    mv = 15_000;
    await settle(r.commands.triggerRequest(pd(3), true));
    expect(asks(r.page)).toBe(true);
    await settle(r.commands.triggerClose());
    expect(asks(r.page)).toBe(false);
  });

  it("a failed request leaves the guard as it was", async () => {
    const r = run({
      answers: {
        request: () => ({ error: { kind: "usage", message: "no", hint: null } }),
      },
    });
    await settle(r.commands.triggerOpen());
    await rejection(r.commands.triggerRequest(pd(2), true));
    expect(asks(r.page)).toBe(false);
    await settle(r.commands.triggerClose());
  });
});
