/**
 * FR W8SHX9: the Web Serial link to the meter's virtual serial port.
 *
 * - AC-W8SHX9.4: `pickMeterPort` reuses the one granted POWER-Z port without asking;
 *   otherwise it opens the picker filtered to POWER-Z ports (`usbVendorId` 0x5FC9). A
 *   cancelled picker is a usage error that says nothing was sent. `openSerialLink`
 *   opens the port at 115200 baud; a busy port (`InvalidStateError` / `NetworkError`)
 *   is the desktop app's "in use by another program" error.
 * - The link waits 200 ms after opening before its first frame (native
 *   FIRST_FRAME_DELAY), writes frames, reads what arrives, times out as `timeout`, and
 *   on close releases both streams and closes the port.
 *
 * Runs on Vitest's fake clock against `FakeSerial` / `FakeSerialPort`.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { UiError } from "../backend";
import { FakeSerial, FakeSerialPort, POWERZ_USB_VENDOR } from "./fakeSerial.test";
import { POWERZ_SERIAL_FILTER, openSerialLink, pickMeterPort } from "./serial";
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

async function rejection(p: Promise<unknown>): Promise<UiError> {
  try {
    await settle(p);
  } catch (e) {
    return e as UiError;
  }
  throw new Error("the call resolved");
}

type Serial = Parameters<typeof pickMeterPort>[0];
type Port = Parameters<typeof openSerialLink>[0];

const asSerial = (s: FakeSerial) => s as unknown as Serial;
const asPort = (p: FakeSerialPort) => p as unknown as Port;

describe("picking the meter's port (AC-W8SHX9.4)", () => {
  it("filters the picker to POWER-Z ports", () => {
    expect(POWERZ_SERIAL_FILTER).toEqual({ usbVendorId: POWERZ_USB_VENDOR });
  });

  it("asks the browser with the POWER-Z filter when no port is granted", async () => {
    const serial = new FakeSerial();
    const port = new FakeSerialPort();
    serial.pick = port;
    const picked = await settle(pickMeterPort(asSerial(serial)));
    expect(picked).toBe(port);
    expect(serial.requests).toEqual([{ filters: [{ usbVendorId: POWERZ_USB_VENDOR }] }]);
  });

  it("reuses the one granted POWER-Z port without asking", async () => {
    const serial = new FakeSerial();
    const port = new FakeSerialPort();
    serial.ports = [port];
    serial.pick = new FakeSerialPort();
    expect(await settle(pickMeterPort(asSerial(serial)))).toBe(port);
    expect(serial.requests).toEqual([]);
  });

  it("ignores granted ports of other vendors", async () => {
    const serial = new FakeSerial();
    const other = new FakeSerialPort();
    other.info = { usbVendorId: 0x1234, usbProductId: 1 };
    const meter = new FakeSerialPort();
    serial.ports = [other];
    serial.pick = meter;
    expect(await settle(pickMeterPort(asSerial(serial)))).toBe(meter);
    expect(serial.requests).toHaveLength(1);
  });

  it("asks when two POWER-Z ports are granted (it cannot tell which meter)", async () => {
    const serial = new FakeSerial();
    const a = new FakeSerialPort();
    const b = new FakeSerialPort();
    serial.ports = [a, b];
    serial.pick = b;
    expect(await settle(pickMeterPort(asSerial(serial)))).toBe(b);
    expect(serial.requests).toHaveLength(1);
  });

  it("a cancelled picker is a plain usage error", async () => {
    const serial = new FakeSerial();
    serial.pick = null;
    const e = await rejection(pickMeterPort(asSerial(serial)));
    expect(e).toEqual({
      kind: "usage",
      message: "No serial port was chosen, so nothing was sent.",
      hint: null,
    });
  });
});

describe("the serial link", () => {
  it("opens the port at 115200 baud", async () => {
    const port = new FakeSerialPort();
    const link = await settle(openSerialLink(asPort(port)));
    expect(port.opens).toEqual([{ baudRate: 115200 }]);
    expect(port.isOpen).toBe(true);
    await settle(link.close());
  });

  for (const name of ["InvalidStateError", "NetworkError"]) {
    it(`a busy port (${name}) is the desktop app's port-held error (AC-W8SHX9.4)`, async () => {
      const port = new FakeSerialPort();
      port.openError = new DOMException("Failed to open serial port.", name);
      const e = await rejection(openSerialLink(asPort(port)));
      expect(e).toEqual({
        kind: "device_busy",
        message: "The meter's serial port is in use by another program.",
        hint: "Close that program, then try again.",
      });
      expect(port.written).toEqual([]);
    });
  }

  it("sends nothing in the first 200 ms after opening", async () => {
    const port = new FakeSerialPort();
    const opened = Date.now();
    const link = await settle(openSerialLink(asPort(port)));
    await settle(link.write(new Uint8Array([0x0c, 0x01, 0x10, 0x00])));
    expect(port.written).toHaveLength(1);
    expect((port.written[0]?.at ?? 0) - opened).toBeGreaterThanOrEqual(200);
    await settle(link.close());
  });

  it("writes frames and reads back what the meter sends", async () => {
    const port = new FakeSerialPort((f) => [f.slice(0, 3), f.slice(3)]);
    const link = await settle(openSerialLink(asPort(port)));
    const frame = new Uint8Array([0x0c, 0x05, 0x10, 0x00, 0xaa]);
    await settle(link.write(frame));
    const got: number[] = [];
    while (got.length < frame.length) got.push(...(await settle(link.read(1_000))));
    expect(got).toEqual([...frame]);
    expect(port.frames()).toEqual([frame]);
    await settle(link.close());
  });

  it("a read with nothing arriving times out as `timeout`", async () => {
    const port = new FakeSerialPort();
    const link = await settle(openSerialLink(asPort(port)));
    const e = await rejection(link.read(300));
    expect(e.kind).toBe("timeout");
    await settle(link.close());
  });

  it("close releases both streams and closes the port", async () => {
    const port = new FakeSerialPort((f) => [f]);
    const link = await settle(openSerialLink(asPort(port)));
    await settle(link.write(new Uint8Array([0x05, 0x01, 0x00, 0x00])));
    await settle(link.close());
    expect(port.isOpen).toBe(false);
    expect(port.closes).toBe(1);
  });
});

describe("hardening (FR W8SHX9 review)", () => {
  it("a picker the browser refuses (no user activation) says so, not that none was chosen", async () => {
    const serial = new FakeSerial();
    serial.requestPort = () =>
      Promise.reject(Object.assign(new Error("no gesture"), { name: "SecurityError" }));
    const e = await rejection(pickMeterPort(serial));
    expect(e.kind).toBe("usage");
    expect(e.message).toMatch(/did not open the serial port picker/);
  });

  it("a port that floods more than the buffer cap fails the read as protocol", async () => {
    const port = new FakeSerialPort();
    const link = await settle(openSerialLink(port));
    for (let i = 0; i < 70; i++) port.emit(new Uint8Array(1024));
    await vi.advanceTimersByTimeAsync(10);
    // Everything below the cap is still handed out, then the overflow surfaces.
    let error: UiError | null = null;
    for (let i = 0; i < 80 && !error; i++) {
      try {
        await settle(link.read(10));
      } catch (e) {
        error = e as UiError;
      }
    }
    expect(error?.kind).toBe("protocol");
    await settle(link.close());
  });
});
