/**
 * FR W8SHX9: the web backend changes the meter's settings over Web Serial.
 *
 * - AC-W8SHX9.1: over WebUSB, `meterSettingsWrite` reads SETTINGS twice, writes once and
 *   reads again, all on the meter's serial port; none of it goes over WebUSB, and the
 *   settings read over WebUSB afterwards shows the change.
 * - AC-W8SHX9.4: the first change opens the Web Serial picker filtered to POWER-Z ports;
 *   the granted port is reused without asking. A cancelled picker, a busy port or a
 *   browser without Web Serial ends the change with a plain message and sends nothing.
 * - AC-W8SHX9.5: after a change that took, the queue restarts at the session's rate.
 * - AC-W8SHX9.7: a HID session refuses the change with the basic-readings wording; the
 *   web build has the settings write feature on and the session answers
 *   `meterSettingsWrite` itself.
 * - The demo meter changes its settings over its own CDC twin, with no Web Serial.
 *
 * Runs on Vitest's fake clock against fake WebUSB / WebHID / Web Serial objects
 * answered by the wasm demo meter and its CDC twin.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MeterSettings, UiError } from "../backend";
import type { PowerzWasm } from "./driver";
import { FakeUsbDevice } from "./fakeDevices.test";
import {
  FakeSerial,
  FakeSerialPort,
  POWERZ_USB_VENDOR,
  demoCdcResponder,
  demoMeterWithTwin,
  foreignAdcReply,
} from "./fakeSerial.test";
import { WEB_FEATURES, hidOnlyBasicText } from "./features";
import { SESSION_COMMANDS } from "./session";
import { classify, loadWasm } from "./testWasm.test";
import { createWebBackend } from "./webBackend";
import {
  FakeHidNavigator,
  FakeUsbNavigator,
  demoHidDevice,
  downloadSink,
  hidSentNames,
  sentNames,
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
    await settle(p);
  } catch (e) {
    return e as UiError;
  }
  throw new Error("expected a rejection");
}

/** A WebUSB meter and its serial port, both answered by one demo meter (the port by
 * its CDC twin, so they share the stored settings). */
function meterWithPort(): { dev: FakeUsbDevice; port: FakeSerialPort } {
  const meter = demoMeterWithTwin(w);
  let last = Date.now();
  const dev = new FakeUsbDevice((frame) => {
    const now = Date.now();
    if (now > last) {
      meter.advance(now - last);
      last = now;
    }
    const reply = meter.transact(frame);
    return reply.length ? reply : null;
  });
  dev.serialNumber = "123456";
  (dev as unknown as { productName: string }).productName = "KM003C";
  const port = new FakeSerialPort(demoCdcResponder(meter.cdcTwin()));
  return { dev, port };
}

interface UsbRun {
  backend: Backend;
  dev: FakeUsbDevice;
  port: FakeSerialPort;
  serial: FakeSerial;
  id: string;
}

async function usbBackend(o: { serial?: FakeSerial | null } = {}): Promise<UsbRun> {
  const nav = new FakeUsbNavigator();
  const { dev, port } = meterWithPort();
  nav.devices = [dev];
  const serial = o.serial === undefined ? new FakeSerial() : o.serial;
  if (serial) serial.pick = port;
  const backend = await settle(
    createWebBackend({
      wasm: w,
      usb: nav,
      hid: null,
      serial,
      download: downloadSink().download,
    } as unknown as Options),
  );
  const id = (await settle(backend.listDevices())).find((d) => d.serial === "123456")?.id;
  if (!id) throw new Error("the permitted meter is not listed");
  await settle(backend.connect(id, 50, () => {}));
  await vi.advanceTimersByTimeAsync(300);
  return { backend, dev, port, serial: serial ?? new FakeSerial(), id };
}

const valueOf = (s: MeterSettings, key: string) => s.fields.find((f) => f.key === key)?.value;
const POLLS = ["get_data:adc", "get_data:adc_queue"];
const usbNamesFrom = (dev: FakeUsbDevice, from: number) =>
  sentNames(w, dev)
    .slice(from)
    .filter((n) => !POLLS.includes(n));
const portKinds = (port: FakeSerialPort) =>
  port
    .frames()
    .map((f) =>
      classify(w, f) === "get_data:settings" ? "read" : f[0] === 0x0a ? "write" : "other",
    );

describe("the feature map and the session (AC-W8SHX9.7)", () => {
  it("switches the settings write on in the web build", async () => {
    expect(WEB_FEATURES.settingsWrite).toBe(true);
    const { backend } = await usbBackend();
    expect(backend.features.settingsWrite).toBe(true);
    await settle(backend.disconnect());
  });

  it("the session answers meterSettingsWrite itself", () => {
    expect(SESSION_COMMANDS as readonly string[]).toContain("meterSettingsWrite");
  });
});

describe("over WebUSB and Web Serial (AC-W8SHX9.1, .4, .5)", () => {
  it("the first change asks for the port, then reads, writes and reads it back there", async () => {
    const { backend, dev, port, serial } = await usbBackend();
    const from = sentNames(w, dev).length;
    const after = await settle(backend.meterSettingsWrite({ brightness: 80 }));
    expect(valueOf(after, "brightness")).toContain("80");
    expect(serial.requests).toEqual([{ filters: [{ usbVendorId: POWERZ_USB_VENDOR }] }]);
    expect(port.opens).toEqual([{ baudRate: 115200 }]);
    expect(portKinds(port)).toEqual(["read", "read", "write", "read"]);
    // The port is released after the change.
    expect(port.isOpen).toBe(false);
    // Over WebUSB only the queue restart, and never the write (AC-W8SHX9.5).
    expect(usbNamesFrom(dev, from)).toEqual(["stop_queue", "start_queue:2"]);
    expect(sentNames(w, dev)).not.toContain("unknown");
    // The WebUSB read shows what the serial write changed.
    expect(valueOf(await settle(backend.meterSettings()), "brightness")).toContain("80");
    await settle(backend.disconnect());
  });

  it("a granted port is reused without asking, on the backend and on meter(id)", async () => {
    const { backend, port, serial, id } = await usbBackend();
    await settle(backend.meterSettingsWrite({ brightness: 60 }));
    expect(serial.requests).toHaveLength(1);
    serial.pick = null;
    const after = await settle(backend.meter(id).meterSettingsWrite({ brightness: 40 }));
    expect(valueOf(after, "brightness")).toContain("40");
    expect(serial.requests).toHaveLength(1);
    expect(port.opens).toHaveLength(2);
    await settle(backend.disconnect());
  });

  it("a cancelled picker ends the change with a plain message and sends nothing", async () => {
    const { backend, dev, serial } = await usbBackend();
    serial.pick = null;
    const from = sentNames(w, dev).length;
    const e = await rejection(backend.meterSettingsWrite({ brightness: 80 }));
    expect(e).toEqual({
      kind: "usage",
      message: "No serial port was chosen, so nothing was sent.",
      hint: null,
    });
    expect(usbNamesFrom(dev, from)).toEqual([]);
    await settle(backend.disconnect());
  });

  it("a busy port ends the change with the port-held error and sends nothing", async () => {
    const { backend, dev, port } = await usbBackend();
    port.openError = new DOMException("Failed to open serial port.", "InvalidStateError");
    const from = sentNames(w, dev).length;
    const e = await rejection(backend.meterSettingsWrite({ brightness: 80 }));
    expect(e).toEqual({
      kind: "device_busy",
      message: "The meter's serial port is in use by another program.",
      hint: "Close that program, then try again.",
    });
    expect(port.written).toEqual([]);
    expect(usbNamesFrom(dev, from)).toEqual([]);
    await settle(backend.disconnect());
  });

  it("a browser without Web Serial ends the change with a plain message", async () => {
    const { backend, dev } = await usbBackend({ serial: null });
    const from = sentNames(w, dev).length;
    const e = await rejection(backend.meterSettingsWrite({ brightness: 80 }));
    expect(e).toEqual({
      kind: "usage",
      message:
        "This browser has no Web Serial: changing settings needs Chrome or Edge on a desktop computer.",
      hint: null,
    });
    expect(usbNamesFrom(dev, from)).toEqual([]);
    await settle(backend.disconnect());
  });

  it("a bad change is refused before the picker opens (AC-W8SHX9.2)", async () => {
    const { backend, dev, port, serial } = await usbBackend();
    const from = sentNames(w, dev).length;
    const e = await rejection(backend.meterSettingsWrite({ brightness: 101 }));
    expect(e).toEqual({
      kind: "usage",
      message: "brightness = 101 is out of range (1..=100).",
      hint: null,
    });
    expect(serial.requests).toEqual([]);
    expect(port.opens).toEqual([]);
    expect(usbNamesFrom(dev, from)).toEqual([]);
    await settle(backend.disconnect());
  });
});

describe("the demo meter", () => {
  it("changes its settings over its own CDC twin, with no Web Serial", async () => {
    const serial = new FakeSerial();
    const backend = await settle(
      createWebBackend({
        wasm: w,
        usb: null,
        hid: null,
        serial,
        download: downloadSink().download,
      } as unknown as Options),
    );
    const demo = (await settle(backend.listDevices())).find((d) => d.demo_scenario !== null);
    if (!demo) throw new Error("no demo device");
    await settle(backend.connect(demo.id, 50, () => {}));
    const after = await settle(backend.meterSettingsWrite({ brightness: 42 }));
    expect(valueOf(after, "brightness")).toContain("42");
    expect(valueOf(await settle(backend.meterSettings()), "brightness")).toContain("42");
    expect(serial.requests).toEqual([]);
    await settle(backend.disconnect());
  });
});

describe("a HID session (AC-W8SHX9.7)", () => {
  it("refuses the change with the basic-readings wording and opens no port", async () => {
    const hid = new FakeHidNavigator();
    const dev = demoHidDevice(w);
    hid.devices = [dev];
    const serial = new FakeSerial();
    serial.pick = new FakeSerialPort();
    const backend = await settle(
      createWebBackend({
        wasm: w,
        usb: null,
        hid,
        serial,
        download: downloadSink().download,
      } as unknown as Options),
    );
    const id = (await settle(backend.listDevices())).find((d) => d.id.startsWith("webhid:"))?.id;
    if (!id) throw new Error("the HID meter is not listed");
    await settle(backend.connect(id, 50, () => {}));
    const e = await rejection(backend.meterSettingsWrite({ brightness: 80 }));
    expect(e.kind).toBe("usage");
    expect(e.message).toBe(hidOnlyBasicText("Changing the meter's settings"));
    expect(serial.requests).toEqual([]);
    expect(hidSentNames(w, dev).every((n) => n === "get_data:adc")).toBe(true);
    await settle(backend.disconnect());
  });
});

/** FR CJ3QC9: the in-use refusal (`portHeldError`). */
const IN_USE: UiError = {
  kind: "device_busy",
  message: "The meter's serial port is in use by another program.",
  hint: "Close that program, then try again.",
};

/** Another program polling ADC on `port`: while `on()`, each frame we write is
 * answered first by that program's ADC reply (`sameId`: carrying our request's id). */
function sharePort(port: FakeSerialPort, on: () => boolean, sameId = false): void {
  const meter = port.respond;
  port.respond = (frame) =>
    on() ? [foreignAdcReply(w, sameId ? (frame[1] ?? 0) : 0x5a), ...meter(frame)] : meter(frame);
}

/** Another program's ADC replies already waiting on `port` when it opens (first open only). */
function repliesWaitingOnOpen(port: FakeSerialPort): void {
  const open = port.open.bind(port);
  let first = true;
  port.open = async (options) => {
    await open(options);
    if (!first) return;
    first = false;
    port.emit(foreignAdcReply(w, 0x21));
    port.emit(foreignAdcReply(w, 0x22));
  };
}

const writes = (port: FakeSerialPort) => portKinds(port).filter((k) => k === "write");

describe("another program on the serial port (FR CJ3QC9)", () => {
  it("a reply the change never asked for stops it with the in-use message, no write (AC-CJ3QC9.1)", async () => {
    const { backend, dev, port } = await usbBackend();
    sharePort(port, () => true);
    const from = sentNames(w, dev).length;
    const e = await rejection(backend.meterSettingsWrite({ brightness: 80 }));
    expect(e).toEqual(IN_USE);
    expect(writes(port)).toEqual([]);
    expect(usbNamesFrom(dev, from)).toEqual([]);
    expect(valueOf(await settle(backend.meterSettings()), "brightness")).not.toContain("80");
    await settle(backend.disconnect());
  });

  it("a foreign ADC reply that happens to carry our request's id counts the same (AC-CJ3QC9.1)", async () => {
    const { backend, port } = await usbBackend();
    sharePort(port, () => true, true);
    const e = await rejection(backend.meterSettingsWrite({ brightness: 80 }));
    expect(e).toEqual(IN_USE);
    expect(writes(port)).toEqual([]);
    await settle(backend.disconnect());
  });

  it("another program's replies waiting on the port before the first request: in-use, no write (AC-CJ3QC9.2)", async () => {
    const { backend, dev, port } = await usbBackend();
    repliesWaitingOnOpen(port);
    const from = sentNames(w, dev).length;
    const e = await rejection(backend.meterSettingsWrite({ brightness: 80 }));
    expect(e).toEqual(IN_USE);
    expect(writes(port)).toEqual([]);
    expect(usbNamesFrom(dev, from)).toEqual([]);
    await settle(backend.disconnect());
  });

  it("the refusal releases the port and a following change on a clean port takes (AC-CJ3QC9.3)", async () => {
    const { backend, dev, port, serial } = await usbBackend();
    let otherProgram = true;
    sharePort(port, () => otherProgram);
    expect(await rejection(backend.meterSettingsWrite({ brightness: 80 }))).toEqual(IN_USE);
    expect(port.isOpen).toBe(false);
    expect(port.closes).toBe(1);
    // The session goes on streaming over WebUSB.
    const polls = sentNames(w, dev).length;
    await vi.advanceTimersByTimeAsync(500);
    expect(sentNames(w, dev).length).toBeGreaterThan(polls);
    // The other program has quit: the next try reuses the granted port and takes.
    otherProgram = false;
    const before = port.frames().length;
    const after = await settle(backend.meterSettingsWrite({ brightness: 80 }));
    expect(valueOf(after, "brightness")).toContain("80");
    expect(
      port
        .frames()
        .slice(before)
        .map((f) =>
          classify(w, f) === "get_data:settings" ? "read" : f[0] === 0x0a ? "write" : "other",
        ),
    ).toEqual(["read", "read", "write", "read"]);
    expect(serial.requests).toHaveLength(1);
    expect(port.isOpen).toBe(false);
    await settle(backend.disconnect());
  });

  it("the refusal after replies waiting on open also releases the port for another try (AC-CJ3QC9.3)", async () => {
    const { backend, port } = await usbBackend();
    repliesWaitingOnOpen(port);
    expect(await rejection(backend.meterSettingsWrite({ brightness: 80 }))).toEqual(IN_USE);
    expect(port.isOpen).toBe(false);
    const after = await settle(backend.meterSettingsWrite({ brightness: 80 }));
    expect(valueOf(after, "brightness")).toContain("80");
    await settle(backend.disconnect());
  });

  it("with nobody else on the port the change works as before (AC-CJ3QC9.4)", async () => {
    const { backend, dev, port } = await usbBackend();
    const from = sentNames(w, dev).length;
    const after = await settle(backend.meterSettingsWrite({ brightness: 80 }));
    expect(valueOf(after, "brightness")).toContain("80");
    expect(after.warnings ?? []).toEqual([]);
    expect(portKinds(port)).toEqual(["read", "read", "write", "read"]);
    expect(port.isOpen).toBe(false);
    expect(usbNamesFrom(dev, from)).toEqual(["stop_queue", "start_queue:2"]);
    await settle(backend.disconnect());
  });
});
