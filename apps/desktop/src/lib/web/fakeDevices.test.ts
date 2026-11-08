/**
 * FR QFTCWZ: hand-written fake WebUSB and WebHID devices (no mocking library).
 *
 * The shapes follow the WebUSB / WebHID specs as far as the transports use them:
 * `USBDevice.open / selectConfiguration / claimInterface / transferOut / transferIn /
 * close`, the `navigator.usb` "disconnect" event, and `HIDDevice.open / sendReport /
 * "inputreport" event / close`. Exported for the transport and driver tests; the one
 * test below checks the fakes themselves.
 */

import { describe, expect, it } from "vitest";

export interface FakeEndpoint {
  endpointNumber: number;
  direction: "in" | "out";
  type: "bulk" | "interrupt" | "isochronous";
  packetSize: number;
}

export interface FakeAlternate {
  alternateSetting: number;
  interfaceClass: number;
  interfaceSubclass: number;
  interfaceProtocol: number;
  endpoints: FakeEndpoint[];
}

export interface FakeInterface {
  interfaceNumber: number;
  alternate: FakeAlternate;
  alternates: FakeAlternate[];
  claimed: boolean;
}

export interface FakeConfiguration {
  configurationValue: number;
  interfaces: FakeInterface[];
}

function iface(number: number, cls: number, endpoints: FakeEndpoint[]): FakeInterface {
  const alt: FakeAlternate = {
    alternateSetting: 0,
    interfaceClass: cls,
    interfaceSubclass: 0,
    interfaceProtocol: 0,
    endpoints,
  };
  return { interfaceNumber: number, alternate: alt, alternates: [alt], claimed: false };
}

/** The meter's interface layout with the vendor interface deliberately not first, and a
 * decoy class-0xFF interface that has no bulk pair. */
export function meterConfiguration(): FakeConfiguration {
  return {
    configurationValue: 1,
    interfaces: [
      iface(0, 0x03, [
        { endpointNumber: 1, direction: "in", type: "interrupt", packetSize: 64 },
        { endpointNumber: 1, direction: "out", type: "interrupt", packetSize: 64 },
      ]),
      iface(1, 0xff, [{ endpointNumber: 2, direction: "in", type: "interrupt", packetSize: 64 }]),
      iface(2, 0x02, [{ endpointNumber: 3, direction: "in", type: "interrupt", packetSize: 8 }]),
      iface(3, 0x0a, [
        { endpointNumber: 4, direction: "in", type: "bulk", packetSize: 64 },
        { endpointNumber: 4, direction: "out", type: "bulk", packetSize: 64 },
      ]),
      iface(4, 0xff, [
        { endpointNumber: 5, direction: "out", type: "bulk", packetSize: 64 },
        { endpointNumber: 6, direction: "in", type: "bulk", packetSize: 64 },
      ]),
    ],
  };
}

export type Responder = (frame: Uint8Array) => Uint8Array | null;

/** A fake `USBDevice`. `respond` turns each OUT transfer into the next IN transfer
 * (null: nothing arrives). `inError` makes the next transferIn reject with it. */
export class FakeUsbDevice {
  vendorId = 0x5fc9;
  productId = 0x0063;
  serialNumber = "123456";
  opened = false;
  configuration: FakeConfiguration | null = null;
  configurations: FakeConfiguration[];
  calls: string[] = [];
  outTransfers: { endpoint: number; data: Uint8Array }[] = [];
  inTransfers: { endpoint: number; length: number }[] = [];
  claimError: Error | null = null;
  inError: Error | null = null;
  private pending: Uint8Array[] = [];

  constructor(
    public respond: Responder = () => null,
    configuration: FakeConfiguration = meterConfiguration(),
  ) {
    this.configurations = [configuration];
  }

  async open(): Promise<void> {
    this.calls.push("open");
    this.opened = true;
  }
  async close(): Promise<void> {
    this.calls.push("close");
    this.opened = false;
  }
  async selectConfiguration(value: number): Promise<void> {
    this.calls.push(`selectConfiguration:${value}`);
    this.configuration = this.configurations.find((c) => c.configurationValue === value) ?? null;
  }
  async claimInterface(number: number): Promise<void> {
    this.calls.push(`claimInterface:${number}`);
    if (this.claimError) throw this.claimError;
    const found = this.configuration?.interfaces.find((i) => i.interfaceNumber === number);
    if (!found) throw new DOMException("no such interface", "NotFoundError");
    found.claimed = true;
  }
  async releaseInterface(number: number): Promise<void> {
    this.calls.push(`releaseInterface:${number}`);
  }
  async transferOut(
    endpoint: number,
    data: BufferSource,
  ): Promise<{
    status: "ok";
    bytesWritten: number;
  }> {
    const bytes = new Uint8Array(
      data instanceof ArrayBuffer
        ? data
        : (data as ArrayBufferView).buffer.slice(
            (data as ArrayBufferView).byteOffset,
            (data as ArrayBufferView).byteOffset + (data as ArrayBufferView).byteLength,
          ),
    );
    this.outTransfers.push({ endpoint, data: bytes });
    if (bytes.length > 0) {
      const reply = this.respond(bytes);
      if (reply) this.pending.push(reply);
    }
    return { status: "ok", bytesWritten: bytes.length };
  }
  transferIn(endpoint: number, length: number): Promise<{ status: "ok"; data: DataView }> {
    this.inTransfers.push({ endpoint, length });
    if (this.inError) {
      const e = this.inError;
      return Promise.reject(e);
    }
    const next = this.pending.shift();
    // Nothing to read: a real transferIn just stays pending.
    if (!next) return new Promise(() => {});
    return Promise.resolve({ status: "ok", data: new DataView(next.buffer.slice(0)) });
  }
}

type Listener = (event: { device: unknown }) => void;

/** A fake `navigator.usb` that only carries the "disconnect" event. */
export class FakeUsb {
  private listeners: Listener[] = [];
  addEventListener(type: string, fn: Listener): void {
    if (type === "disconnect") this.listeners.push(fn);
  }
  removeEventListener(type: string, fn: Listener): void {
    if (type === "disconnect") this.listeners = this.listeners.filter((l) => l !== fn);
  }
  unplug(device: unknown): void {
    for (const l of [...this.listeners]) l({ device });
  }
}

type ReportListener = (event: { reportId: number; data: DataView; device: unknown }) => void;

/** A fake `HIDDevice`. `respond` turns each output report into the input reports that
 * follow it (each one 64 bytes), delivered as "inputreport" events. */
export class FakeHidDevice {
  vendorId = 0x5fc9;
  productId = 0x0063;
  productName = "KM003C";
  opened = false;
  collections = [{ usagePage: 0xff00, usage: 0x01, inputReports: [], outputReports: [] }];
  sent: { reportId: number; data: Uint8Array }[] = [];
  oninputreport: ReportListener | null = null;
  private listeners: ReportListener[] = [];

  constructor(public respond: (report: Uint8Array) => Uint8Array[] = () => []) {}

  async open(): Promise<void> {
    this.opened = true;
  }
  async close(): Promise<void> {
    this.opened = false;
  }
  addEventListener(type: string, fn: ReportListener): void {
    if (type === "inputreport") this.listeners.push(fn);
  }
  removeEventListener(type: string, fn: ReportListener): void {
    if (type === "inputreport") this.listeners = this.listeners.filter((l) => l !== fn);
  }
  async sendReport(reportId: number, data: BufferSource): Promise<void> {
    const view = data as ArrayBufferView;
    const bytes =
      data instanceof ArrayBuffer
        ? new Uint8Array(data.slice(0))
        : new Uint8Array(view.buffer.slice(view.byteOffset, view.byteOffset + view.byteLength));
    this.sent.push({ reportId, data: bytes });
    const reports = this.respond(bytes);
    // Reports arrive one by one after the write, like the real device. Each report's
    // timer is armed only after the previous one fired, so they always arrive in order
    // (separate timers of different delays may fire out of order under load).
    const next = (i: number): void => {
      const r = reports[i];
      if (!r) return;
      setTimeout(() => {
        this.emit(r);
        next(i + 1);
      }, 1);
    };
    next(0);
  }
  emit(report: Uint8Array): void {
    const event = { reportId: 0, data: new DataView(report.buffer.slice(0)), device: this };
    for (const l of [...this.listeners]) l(event);
    this.oninputreport?.(event);
  }
}

/** `bytes` cut into 64-byte input reports, the last one zero-padded. */
export function toReports(bytes: Uint8Array): Uint8Array[] {
  const out: Uint8Array[] = [];
  for (let at = 0; at < bytes.length; at += 64) {
    const r = new Uint8Array(64);
    r.set(bytes.subarray(at, Math.min(at + 64, bytes.length)));
    out.push(r);
  }
  return out;
}

describe("fake devices", () => {
  it("cut a reply into zero-padded 64-byte reports", () => {
    const reports = toReports(new Uint8Array(130).fill(7));
    expect(reports.map((r) => r.length)).toEqual([64, 64, 64]);
    expect(reports[2]?.slice(0, 2)).toEqual(new Uint8Array([7, 7]));
    expect(reports[2]?.[2]).toBe(0);
  });
});
