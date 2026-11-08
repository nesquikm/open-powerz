/**
 * FR QFTCX0: shared fakes for the web backend tests (no mocking library).
 *
 * - `FakeUsbNavigator`: a fake `navigator.usb` with `getDevices`, `requestDevice` (the
 *   picker) and the "connect" / "disconnect" hotplug events.
 * - `demoUsbDevice`: a `FakeUsbDevice` answered by the wasm demo meter, its clock driven
 *   by `Date.now()` (Vitest's fake clock in the session tests).
 * - `settle`: advance the fake clock until a promise settles.
 * - `sentNames`: the names of the frames a fake device received, via the safe-set
 *   classifier (`unknown` for anything outside it).
 *
 * Exported for the web backend tests; the test at the bottom checks the fakes.
 */

import { describe, expect, it, vi } from "vitest";
import type { PowerzWasm } from "./driver";
import { FakeUsbDevice } from "./fakeDevices.test";
import { classify, loadWasm, type FrameName } from "./testWasm.test";

type Listener = (event: { device: unknown }) => void;

/** A fake `navigator.usb`: permitted devices, the picker and hotplug events. */
export class FakeUsbNavigator {
  /** Devices the page already has permission for (what `getDevices()` answers). */
  devices: FakeUsbDevice[] = [];
  /** Every `requestDevice` call's options, in order. */
  requests: unknown[] = [];
  /** What the picker returns next; `null`: the user cancels it. */
  pick: FakeUsbDevice | null = null;
  private listeners: Record<string, Listener[]> = { connect: [], disconnect: [] };

  async getDevices(): Promise<FakeUsbDevice[]> {
    return [...this.devices];
  }

  /** Records the call synchronously (the user-gesture check), then answers. */
  requestDevice(options: unknown): Promise<FakeUsbDevice> {
    this.requests.push(options);
    const picked = this.pick;
    if (!picked) {
      return Promise.reject(new DOMException("No device selected.", "NotFoundError"));
    }
    if (!this.devices.includes(picked)) this.devices.push(picked);
    return Promise.resolve(picked);
  }

  addEventListener(type: string, fn: Listener): void {
    (this.listeners[type] ??= []).push(fn);
  }

  removeEventListener(type: string, fn: Listener): void {
    this.listeners[type] = (this.listeners[type] ?? []).filter((l) => l !== fn);
  }

  /** A permitted meter is plugged in. */
  plug(device: FakeUsbDevice): void {
    if (!this.devices.includes(device)) this.devices.push(device);
    for (const l of [...(this.listeners.connect ?? [])]) l({ device });
  }

  /** A meter is unplugged. */
  unplug(device: FakeUsbDevice): void {
    this.devices = this.devices.filter((d) => d !== device);
    for (const l of [...(this.listeners.disconnect ?? [])]) l({ device });
  }

  listenerCount(type: string): number {
    return (this.listeners[type] ?? []).length;
  }
}

/** A fake WebUSB meter with serial `serial`, answered by the wasm demo meter running
 * `scenario`; the demo clock follows `Date.now()`. */
export function demoUsbDevice(
  w: PowerzWasm,
  serial = "123456",
  scenario = "load-5v-1a",
): FakeUsbDevice {
  const meter = new w.DemoMeter(scenario);
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
  dev.serialNumber = serial;
  (dev as unknown as { productName: string }).productName = "KM003C";
  return dev;
}

/** The frames `dev` received (zero-length packets left out), by safe-set name. */
export function sentNames(w: PowerzWasm, dev: FakeUsbDevice): FrameName[] {
  return dev.outTransfers.filter((t) => t.data.length > 0).map((t) => classify(w, t.data));
}

/** One captured download: its file name and contents. */
export interface Download {
  name: string;
  blob: Blob;
}

/** A download sink for the backend's `download` option. */
export function downloadSink(): {
  downloads: Download[];
  download: (name: string, blob: Blob) => void;
} {
  const downloads: Download[] = [];
  return { downloads, download: (name, blob) => downloads.push({ name, blob }) };
}

/** Advance the fake clock in small steps until `p` settles (at most `maxMs`). */
export async function settle<T>(p: Promise<T>, maxMs = 5_000): Promise<T> {
  let done = false;
  let value: T | undefined;
  let error: unknown;
  let failed = false;
  p.then(
    (v) => {
      done = true;
      value = v;
    },
    (e: unknown) => {
      done = true;
      failed = true;
      error = e;
    },
  );
  for (let i = 0; i < 5 && !done; i++) await Promise.resolve();
  for (let t = 0; t < maxMs && !done; t += 10) await vi.advanceTimersByTimeAsync(10);
  if (!done) throw new Error("promise did not settle on the fake clock");
  if (failed) throw error;
  return value as T;
}

describe("web backend fakes", () => {
  it("the picker records its options synchronously and adds the picked device", async () => {
    const nav = new FakeUsbNavigator();
    const w = await loadWasm();
    const dev = demoUsbDevice(w, "777");
    nav.pick = dev;
    const p = nav.requestDevice({ filters: [{ vendorId: 0x5fc9 }] });
    expect(nav.requests).toEqual([{ filters: [{ vendorId: 0x5fc9 }] }]);
    expect(await p).toBe(dev);
    expect(await nav.getDevices()).toEqual([dev]);
  });

  it("the demo USB device answers CONNECT", async () => {
    const w = await loadWasm();
    const dev = demoUsbDevice(w);
    await dev.transferOut(1, new Uint8Array(w.connectRequest(1)));
    const reply = await dev.transferIn(1, 64);
    expect(reply.data.byteLength).toBeGreaterThan(0);
    expect(sentNames(w, dev)).toEqual(["connect"]);
  });
});
