/**
 * FR W8SHXC: protocol triggering in the browser.
 *
 * - AC-W8SHXC.1: the trigger commands answer as on the desktop. The demo meter runs
 *   the wasm demo engine in the page (its port is simulated and synchronous, so no
 *   worker); a real meter's commands go to the engine's worker as RPC calls with the
 *   desktop arguments, and the engine's errors come back with their kind, message and
 *   hint.
 * - AC-W8SHXC.3: above 5 V is refused unless confirmed or covered by "don't ask
 *   again"; console lines always ask (the demo engine).
 * - AC-W8SHXC.4: leaving the view (`triggerBackTo5v(true)`), closing the session,
 *   disconnecting the meter and the page's `pagehide` each send the way back to 5 V
 *   and `pdm close` through the engine, and the serial port is closed after.
 * - AC-W8SHXC.5: the stream's VBUS readings reach the worker's VBUS cell.
 * - AC-W8SHXC.6: the serial port is picked as for the settings change (the granted port
 *   reused, a plain message on cancel, busy port, no Web Serial) and shared with it: a
 *   settings change while a trigger session holds the port is refused with the
 *   desktop's busy-port wording and sends nothing; after the close it works again on
 *   the same granted port.
 * - Cross-origin isolation: a real meter's trigger needs `crossOriginIsolated`
 *   (SharedArrayBuffer); without it `triggerOpen` refuses with a usage error whose hint
 *   asks for a reload, before any picker or port.
 * - AC-W8SHXC.7: the web build has the trigger feature on; a HID session can trigger.
 *
 * The backend options these tests use: `crossOriginIsolated` (defaults to the page's),
 * `triggerWorker` (starts the engine's dedicated worker; defaults to the bundled
 * `triggerWorker.ts`) and `pageEvents` (where `pagehide` is listened for; defaults to
 * `window`). The worker protocol: the page posts `{ id, method, args }` and the worker
 * answers `{ id, ok }` or `{ id, error }`; the first call is `open` with
 * `[{ portIn, portOut, vbus }]` — the `SharedArrayBuffer`s of the bytes read from the
 * port (page → worker), the bytes for the port (worker → page) and the VBUS cell. The
 * method names are the wasm `TriggerEngine`'s (`detect`, `list`, `risk`, `request`,
 * `armed`, `disarm`, `chargerGone`, `pdEntered`, `backTo5v`, `leaveTo5v`,
 * `messageRisk`, `sendMessage`, `cableRead`, `consoleRisk`, `consoleSend`, `close`).
 *
 * Runs on Vitest's fake clock against fake WebUSB / WebHID / Web Serial objects and a
 * fake worker; the demo engine is the real wasm module.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { PdMessageRequest, TriggerRequest, UiError } from "../backend";
import type { PowerzWasm } from "./driver";
import { FakeUsbDevice } from "./fakeDevices.test";
import {
  FakeSerial,
  FakeSerialPort,
  demoCdcResponder,
  demoMeterWithTwin,
  type SerialResponder,
} from "./fakeSerial.test";
import { WEB_FEATURES } from "./features";
import { classify, loadWasm } from "./testWasm.test";
import { ByteRing, VbusCell } from "./triggerBridge";
import { createWebBackend } from "./webBackend";
import {
  FakeHidNavigator,
  FakeUsbNavigator,
  demoHidDevice,
  downloadSink,
  settle,
} from "./webBackendFakes.test";

type Options = Parameters<typeof createWebBackend>[0];
type Backend = Awaited<ReturnType<typeof createWebBackend>>;

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
    await settle(p, 20_000);
  } catch (e) {
    return e as UiError;
  }
  throw new Error("expected a rejection");
}

const NEEDS_CONFIRMATION = "A request above 5 V needs your confirmation.";
const PORT_HELD = "The meter's serial port is in use by another program.";

const pd = (position: number): TriggerRequest =>
  ({ protocol: "pd", position, voltage_mv: null, current_ma: null }) as TriggerRequest;

const softReset: PdMessageRequest = { sop: "sop", msg_type: 13, data_objects: [] };

// ---- the fake worker ----

type Answer = { ok: unknown } | { error: UiError };
type Posted = { id: number; method: string; args: unknown[] };
type MessageListener = (e: { data: unknown }) => void;

class FakeTriggerWorker {
  posted: Posted[] = [];
  terminated = false;
  answers: Record<string, (args: unknown[]) => Answer> = {};
  onmessage: MessageListener | null = null;
  private listeners = new Set<MessageListener>();
  portIn: ByteRing | null = null;
  portOut: ByteRing | null = null;
  vbus: VbusCell | null = null;

  postMessage(data: unknown): void {
    const m = data as Posted;
    this.posted.push(m);
    if (m.method === "open") {
      const ch = m.args[0] as {
        portIn: SharedArrayBuffer;
        portOut: SharedArrayBuffer;
        vbus: SharedArrayBuffer;
      };
      this.portIn = new ByteRing(ch.portIn);
      this.portOut = new ByteRing(ch.portOut);
      this.vbus = new VbusCell(ch.vbus);
      // What the engine's handshake writes first.
      this.portOut.write(new TextEncoder().encode("pdm open\r\n"));
    }
    const answer = this.answers[m.method]?.(m.args) ?? { ok: null };
    void Promise.resolve().then(() => this.emit({ data: { id: m.id, ...answer } }));
  }

  private emit(e: { data: unknown }): void {
    this.onmessage?.(e);
    for (const l of this.listeners) l(e);
  }

  addEventListener(type: string, fn: MessageListener): void {
    if (type === "message") this.listeners.add(fn);
  }

  removeEventListener(type: string, fn: MessageListener): void {
    if (type === "message") this.listeners.delete(fn);
  }

  terminate(): void {
    this.terminated = true;
  }

  methods(): string[] {
    return this.posted.map((p) => p.method);
  }
}

// ---- backends ----

/** Text lines the trigger writes are no settings frames: the CDC twin ignores them. */
function settingsOnly(responder: SerialResponder): SerialResponder {
  return (frame) => (frame[0] !== undefined && frame[0] >= 0x61 ? [] : responder(frame));
}

interface RealRun {
  backend: Backend;
  id: string;
  port: FakeSerialPort;
  serial: FakeSerial;
  workers: FakeTriggerWorker[];
  page: EventTarget;
}

interface RealOptions {
  serial?: FakeSerial | null;
  isolated?: boolean;
  hid?: boolean;
}

async function realBackend(o: RealOptions = {}): Promise<RealRun> {
  const meter = demoMeterWithTwin(w);
  let last = Date.now();
  const advance = () => {
    const now = Date.now();
    if (now > last) {
      meter.advance(now - last);
      last = now;
    }
  };
  const port = new FakeSerialPort(settingsOnly(demoCdcResponder(meter.cdcTwin())));
  const serial = o.serial === undefined ? new FakeSerial() : o.serial;
  if (serial) serial.pick = port;
  const workers: FakeTriggerWorker[] = [];
  const page = new EventTarget();
  let usb: FakeUsbNavigator | null = null;
  let hid: FakeHidNavigator | null = null;
  if (o.hid) {
    hid = new FakeHidNavigator();
    hid.devices = [demoHidDevice(w)];
  } else {
    usb = new FakeUsbNavigator();
    const dev = new FakeUsbDevice((frame) => {
      advance();
      const reply = meter.transact(frame);
      return reply.length ? reply : null;
    });
    dev.serialNumber = "123456";
    (dev as unknown as { productName: string }).productName = "KM003C";
    usb.devices = [dev];
  }
  const backend = await settle(
    createWebBackend({
      wasm: w,
      usb,
      hid,
      serial,
      download: downloadSink().download,
      crossOriginIsolated: o.isolated ?? true,
      pageEvents: page,
      triggerWorker: () => {
        const worker = new FakeTriggerWorker();
        workers.push(worker);
        return worker;
      },
    } as unknown as Options),
  );
  const listing = await settle(backend.listDevices());
  const id = listing.find((d) => (o.hid ? d.id.startsWith("webhid:") : d.serial === "123456"))?.id;
  if (!id) throw new Error("the meter is not listed");
  await settle(backend.connect(id, 50, () => {}));
  await vi.advanceTimersByTimeAsync(300);
  return { backend, id, port, serial: serial ?? new FakeSerial(), workers, page };
}

async function demoBackend(isolated = false): Promise<{ backend: Backend; workers: unknown[] }> {
  const workers: unknown[] = [];
  const backend = await settle(
    createWebBackend({
      wasm: w,
      usb: new FakeUsbNavigator(),
      hid: null,
      serial: null,
      download: downloadSink().download,
      crossOriginIsolated: isolated,
      pageEvents: new EventTarget(),
      triggerWorker: () => {
        const worker = new FakeTriggerWorker();
        workers.push(worker);
        return worker;
      },
    } as unknown as Options),
  );
  const id = (await settle(backend.listDevices())).find((d) => d.demo_scenario)?.id;
  if (!id) throw new Error("no demo meter listed");
  await settle(backend.connect(id, 50, () => {}));
  await vi.advanceTimersByTimeAsync(300);
  return { backend, workers };
}

const settingsFrames = (port: FakeSerialPort) =>
  port.frames().filter((f) => classify(w, f) === "get_data:settings" || f[0] === 0x0a);

// ---- AC-W8SHXC.7 ----

describe("the feature map (AC-W8SHXC.7)", () => {
  it("switches triggering on in the web build", async () => {
    expect(WEB_FEATURES.trigger).toBe(true);
    const { backend } = await demoBackend();
    expect(backend.features.trigger).toBe(true);
    await settle(backend.disconnect());
  });
});

// ---- AC-W8SHXC.1, .3: the demo engine in the page ----

describe("the demo meter's trigger (AC-W8SHXC.1, .3)", () => {
  it("runs the demo engine in the page, with no worker and no isolation", async () => {
    const { backend, workers } = await demoBackend(false);
    await settle(backend.triggerOpen());
    const section = await settle(backend.triggerList("pd"));
    expect(section.supported).toBe(true);
    expect(section.offers.length).toBeGreaterThan(1);
    expect(workers).toEqual([]);
    await settle(backend.triggerClose());
    await settle(backend.disconnect());
  });

  it("asks before 9 V, sends it confirmed and arms only when remembered", async () => {
    const { backend } = await demoBackend();
    await settle(backend.triggerOpen());
    await settle(backend.triggerList("pd"));
    expect(await settle(backend.triggerRisk(pd(1)))).toBe("at_most_5v");
    expect(await settle(backend.triggerRisk(pd(2)))).toBe("above_5v_rememberable");
    const refused = await rejection(backend.triggerRequest(pd(2), false));
    expect(refused).toEqual({ kind: "usage", message: NEEDS_CONFIRMATION, hint: null });
    const reply = await settle(backend.triggerRequest(pd(2), true));
    expect(reply.command).toBe("pd req=2");
    expect(await settle(backend.triggerArmed())).toBe(false);
    expect(await settle(backend.triggerPdEntered())).toBe(true);
    await settle(backend.triggerRequest(pd(2), true, true));
    expect(await settle(backend.triggerArmed())).toBe(true);
    expect(await settle(backend.triggerRisk(pd(2)))).toBe("armed");
    // Covered: no confirmation needed.
    await settle(backend.triggerRequest(pd(2), false));
    // The console always asks.
    expect(await settle(backend.triggerConsoleRisk("pd req=3"))).toBe("above_5v");
    const line = await rejection(backend.triggerConsoleSend("pd req=3", false));
    expect(line.kind).toBe("usage");
    expect(line.message).toMatch(/needs your confirmation\.$/);
    expect(await settle(backend.triggerArmed())).toBe(true);
    await settle(backend.triggerDisarm());
    expect(await settle(backend.triggerArmed())).toBe(false);
    expect(await settle(backend.triggerChargerGone())).toBe(false);
    await settle(backend.triggerBackTo5v());
    await settle(backend.triggerClose());
    await settle(backend.disconnect());
  });

  it("detects, sends built messages, reads the cable and refuses GoodCRC", async () => {
    const { backend } = await demoBackend();
    await settle(backend.triggerOpen());
    const detection = await settle(backend.triggerDetect(), 30_000);
    expect(Array.isArray(detection.entries)).toBe(true);
    expect(detection.entries.length).toBeGreaterThan(0);
    const notPd = await rejection(backend.triggerCableRead());
    expect(notPd.kind).toBe("usage");
    await settle(backend.triggerList("pd"));
    const cable = await settle(backend.triggerCableRead());
    expect(cable.kind).toBe("found");
    expect(await settle(backend.triggerMessageRisk(softReset))).toBe("at_most_5v");
    const sent = await settle(backend.triggerMessageSend(softReset, false));
    expect(sent.command).toMatch(/^pd /);
    const goodCrc = await rejection(backend.triggerConsoleSend("pd cmd=1", true));
    expect(goodCrc.kind).toBe("usage");
    const lines = await settle(backend.triggerConsoleSend("pdm parm", false));
    expect(lines.some((l) => l.direction === "tx")).toBe(true);
    await settle(backend.triggerClose());
    await settle(backend.disconnect());
  });

  it("answers the desktop's session refusals", async () => {
    const { backend } = await demoBackend();
    const none = await rejection(backend.triggerDetect());
    expect(none).toMatchObject({ kind: "usage", message: "No trigger session is open." });
    await settle(backend.triggerOpen());
    const twice = await rejection(backend.triggerOpen());
    expect(twice).toMatchObject({ kind: "usage", message: "A trigger session is already open." });
    await settle(backend.triggerClose());
    const closed = await rejection(backend.triggerList("pd"));
    expect(closed).toMatchObject({ kind: "usage", message: "No trigger session is open." });
    await settle(backend.disconnect());
  });
});

// ---- AC-W8SHXC.1, .4, .5, .6: a real meter, the engine in its worker ----

describe("a real meter's trigger goes to the engine's worker (AC-W8SHXC.1)", () => {
  it("opens the picked port, starts the worker and moves the bytes both ways", async () => {
    const { backend, port, serial, workers } = await realBackend();
    await settle(backend.triggerOpen());
    expect(serial.requests).toHaveLength(1);
    expect(port.isOpen).toBe(true);
    expect(workers).toHaveLength(1);
    const worker = workers[0] as FakeTriggerWorker;
    expect(worker.methods()[0]).toBe("open");
    // The worker's handshake bytes reached the port.
    await vi.advanceTimersByTimeAsync(50);
    const written = new TextDecoder().decode(Uint8Array.from(port.frames().flatMap((f) => [...f])));
    expect(written).toContain("pdm open\r\n");
    // The port's bytes reach the worker.
    port.emit(new TextEncoder().encode("pdm mode entry\r\n"));
    await vi.advanceTimersByTimeAsync(50);
    expect(new TextDecoder().decode(worker.portIn?.read())).toContain("pdm mode entry");
    await settle(backend.triggerClose());
    await settle(backend.disconnect());
  });

  it("forwards each command with the desktop arguments and returns the answer", async () => {
    const { backend, workers } = await realBackend();
    await settle(backend.triggerOpen());
    const worker = workers[0] as FakeTriggerWorker;
    worker.answers.list = () => ({ ok: { protocol: "pd", supported: true, offers: [] } });
    worker.answers.risk = () => ({ ok: "above_5v_rememberable" });
    worker.answers.armed = () => ({ ok: true });
    const section = await settle(backend.triggerList("pd"));
    expect(section).toEqual({ protocol: "pd", supported: true, offers: [] });
    expect(await settle(backend.triggerRisk(pd(2)))).toBe("above_5v_rememberable");
    await settle(backend.triggerRequest(pd(2), true, true));
    await settle(backend.triggerRequest(pd(3), true));
    expect(await settle(backend.triggerArmed())).toBe(true);
    await settle(backend.triggerMessageSend(softReset, false));
    await settle(backend.triggerConsoleSend("pdm parm", true));
    const byMethod = (m: string) => worker.posted.filter((p) => p.method === m);
    expect(byMethod("list")[0]?.args).toEqual(["pd"]);
    expect(byMethod("request").map((p) => p.args)).toEqual([
      [pd(2), true, true],
      [pd(3), true, false],
    ]);
    expect(byMethod("sendMessage")[0]?.args).toEqual([softReset, false]);
    expect(byMethod("consoleSend")[0]?.args).toEqual(["pdm parm", true]);
    await settle(backend.triggerClose());
    await settle(backend.disconnect());
  });

  it("an engine error keeps its kind, message and hint", async () => {
    const { backend, workers } = await realBackend();
    await settle(backend.triggerOpen());
    const worker = workers[0] as FakeTriggerWorker;
    const timeout: UiError = {
      kind: "timeout",
      message: "The meter did not answer in time.",
      hint: "Check the cable, then try again.",
    };
    worker.answers.detect = () => ({ error: timeout });
    expect(await rejection(backend.triggerDetect())).toEqual(timeout);
    const busy: UiError = { kind: "device_busy", message: PORT_HELD, hint: "x" };
    worker.answers.request = () => ({ error: busy });
    expect(await rejection(backend.triggerRequest(pd(2), true))).toEqual(busy);
    await settle(backend.triggerClose());
    await settle(backend.disconnect());
  });
});

describe("leaving, closing, disconnecting and pagehide go back to 5 V (AC-W8SHXC.4)", () => {
  it("leaving the view sends the leave way back; the plain one the plain way", async () => {
    const { backend, workers } = await realBackend();
    await settle(backend.triggerOpen());
    const worker = workers[0] as FakeTriggerWorker;
    await settle(backend.triggerBackTo5v(true));
    await settle(backend.triggerBackTo5v());
    expect(worker.methods().slice(-2)).toEqual(["leaveTo5v", "backTo5v"]);
    await settle(backend.triggerClose());
    await settle(backend.disconnect());
  });

  it("closing sends the engine's close and frees the serial port", async () => {
    const { backend, port, workers } = await realBackend();
    await settle(backend.triggerOpen());
    const worker = workers[0] as FakeTriggerWorker;
    await settle(backend.triggerClose());
    expect(worker.methods()).toContain("close");
    await vi.advanceTimersByTimeAsync(500);
    expect(port.isOpen).toBe(false);
    await settle(backend.disconnect());
  });

  it("disconnecting the meter closes the trigger session", async () => {
    const { backend, port, workers } = await realBackend();
    await settle(backend.triggerOpen());
    const worker = workers[0] as FakeTriggerWorker;
    await settle(backend.disconnect());
    await vi.advanceTimersByTimeAsync(500);
    expect(worker.methods()).toContain("close");
    expect(port.isOpen).toBe(false);
  });

  it("the page's pagehide sends the leave way back, then close", async () => {
    const { backend, page, workers } = await realBackend();
    await settle(backend.triggerOpen());
    const worker = workers[0] as FakeTriggerWorker;
    const before = worker.posted.length;
    page.dispatchEvent(new Event("pagehide"));
    await vi.advanceTimersByTimeAsync(100);
    expect(worker.methods().slice(before)).toEqual(["leaveTo5v", "close"]);
    await settle(backend.disconnect());
  });
});

describe("the VBUS watch gets the stream's readings (AC-W8SHXC.5)", () => {
  it("each stream batch updates the worker's VBUS cell", async () => {
    const { backend, workers } = await realBackend();
    await settle(backend.triggerOpen());
    const worker = workers[0] as FakeTriggerWorker;
    await vi.advanceTimersByTimeAsync(500);
    const first = worker.vbus?.get();
    expect(first).not.toBeNull();
    expect(first?.volts).toBeGreaterThan(4);
    expect(first?.volts).toBeLessThan(6);
    expect(first?.period_ms).toBeGreaterThan(0);
    await vi.advanceTimersByTimeAsync(500);
    const later = worker.vbus?.get();
    expect(later?.time_ms).toBeGreaterThan(first?.time_ms ?? Infinity);
    await settle(backend.triggerClose());
    await settle(backend.disconnect());
  });
});

describe("the serial port, picked and shared as for settings (AC-W8SHXC.6)", () => {
  it("a cancelled picker ends the open with a plain message and starts nothing", async () => {
    const run = await realBackend();
    run.serial.pick = null;
    const e = await rejection(run.backend.triggerOpen());
    expect(e).toMatchObject({
      kind: "usage",
      message: "No serial port was chosen, so nothing was sent.",
    });
    expect(run.workers).toEqual([]);
    expect(run.port.opens).toEqual([]);
    await settle(run.backend.disconnect());
  });

  it("a busy port is refused with the desktop wording", async () => {
    const run = await realBackend();
    run.port.openError = new DOMException("busy", "InvalidStateError");
    const e = await rejection(run.backend.triggerOpen());
    expect(e).toMatchObject({ kind: "device_busy", message: PORT_HELD });
    expect(run.workers).toEqual([]);
    await settle(run.backend.disconnect());
  });

  it("no Web Serial gives a plain message", async () => {
    const run = await realBackend({ serial: null });
    const e = await rejection(run.backend.triggerOpen());
    expect(e.kind).toBe("usage");
    expect(e.message).toMatch(/Web Serial/);
    expect(run.workers).toEqual([]);
    await settle(run.backend.disconnect());
  });

  it("reuses the granted port without asking again", async () => {
    const run = await realBackend();
    await settle(run.backend.triggerOpen());
    await settle(run.backend.triggerClose());
    await vi.advanceTimersByTimeAsync(500);
    await settle(run.backend.triggerOpen());
    expect(run.serial.requests).toHaveLength(1);
    expect(run.port.opens).toHaveLength(2);
    await settle(run.backend.triggerClose());
    await settle(run.backend.disconnect());
  });

  it("a settings change while the trigger holds the port is refused and sends nothing", async () => {
    const run = await realBackend();
    await settle(run.backend.triggerOpen());
    const e = await rejection(run.backend.meterSettingsWrite({ brightness: 80 }));
    expect(e).toMatchObject({ kind: "device_busy", message: PORT_HELD });
    expect(settingsFrames(run.port)).toEqual([]);
    await settle(run.backend.triggerClose());
    await vi.advanceTimersByTimeAsync(500);
    // The port is free again: the change goes out on the same granted port.
    await settle(run.backend.meterSettingsWrite({ brightness: 80 }), 20_000);
    expect(settingsFrames(run.port).length).toBeGreaterThan(0);
    expect(run.serial.requests).toHaveLength(1);
    await settle(run.backend.disconnect());
  });
});

describe("cross-origin isolation", () => {
  it("a real meter's trigger refuses without it, before any picker or port", async () => {
    const run = await realBackend({ isolated: false });
    const e = await rejection(run.backend.triggerOpen());
    expect(e.kind).toBe("usage");
    expect(e.hint ?? "").toMatch(/reload/i);
    expect(run.serial.requests).toEqual([]);
    expect(run.port.opens).toEqual([]);
    expect(run.workers).toEqual([]);
    await settle(run.backend.disconnect());
  });
});

describe("a HID session can trigger (AC-W8SHXC.7)", () => {
  it("opens the serial port and the engine worker", async () => {
    const run = await realBackend({ hid: true });
    await settle(run.backend.triggerOpen());
    expect(run.port.opens).toHaveLength(1);
    expect(run.workers).toHaveLength(1);
    const worker = run.workers[0] as FakeTriggerWorker;
    worker.answers.list = () => ({ ok: { protocol: "qc", supported: true } });
    expect(await settle(run.backend.triggerList("qc"))).toEqual({
      protocol: "qc",
      supported: true,
    });
    await settle(run.backend.triggerClose());
    await settle(run.backend.disconnect());
  });
});
