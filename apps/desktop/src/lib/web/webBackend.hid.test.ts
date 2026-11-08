/**
 * FR QFTCWZ, AC-QFTCWZ.3: the web backend's WebHID fallback.
 *
 * - Permitted WebHID meters (`navigator.hid.getDevices()`, vendor 0x5FC9) are listed as
 *   `webhid:N` and connect over HID as ADC-snapshot sessions.
 * - When the WebUSB claim fails with `device_busy`, an already-permitted HID meter is
 *   connected instead; without one, a browser with WebHID gets a `device_busy` error that
 *   says HID is an option, and `requestHidMeter()` opens the HID picker synchronously.
 * - A browser with WebHID but no WebUSB uses the HID picker for `requestMeter()`.
 * - A HID session reports `basic_readings_only: true` (rate locked, no rate), refuses PD
 *   capture and rate changes with no traffic, and sends only GET_DATA 0x001.
 * - `device_busy` stays the error when the HID open also fails or there is no WebHID.
 *
 * Runs on Vitest's fake clock.
 */

import { readFileSync, readdirSync } from "node:fs";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { BackendEvent, Batch, Status, UiError } from "../backend";
import type { PowerzWasm } from "./driver";
import { FakeHidDevice, FakeUsbDevice } from "./fakeDevices.test";
import { loadWasm } from "./testWasm.test";
import { createWebBackend } from "./webBackend";
import {
  FakeHidNavigator,
  FakeUsbNavigator,
  demoHidDevice,
  demoUsbDevice,
  downloadSink,
  hidSentNames,
  settle,
} from "./webBackendFakes.test";
import { HID_BASIC_NOTE } from "./features";

type Options = Parameters<typeof createWebBackend>[0];
type Backend = Awaited<ReturnType<typeof createWebBackend>> & {
  requestHidMeter(): ReturnType<Awaited<ReturnType<typeof createWebBackend>>["requestMeter"]>;
};

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

async function backendWith(
  usb: FakeUsbNavigator | null,
  hid: FakeHidNavigator | null,
): Promise<Backend> {
  return (await settle(
    createWebBackend({
      wasm: w,
      usb,
      hid,
      download: downloadSink().download,
    } as unknown as Options),
  )) as Backend;
}

const statuses = (events: BackendEvent[]): Status[] =>
  events.flatMap((e) => (e.type === "status" ? [e.data] : []));
const batches = (events: BackendEvent[]): Batch[] =>
  events.flatMap((e) => (e.type === "batch" ? [e.data] : []));
const adcOnly = (names: string[]) => names.filter((n) => n !== "get_data:adc");

/** A WebUSB meter whose vendor interface an OS driver holds. */
function busyUsbDevice(): FakeUsbDevice {
  const dev = demoUsbDevice(w, "123456");
  dev.claimError = new DOMException("Unable to claim interface.", "NetworkError");
  return dev;
}

async function rejection(p: Promise<unknown>): Promise<UiError> {
  try {
    await settle(p);
  } catch (e) {
    return e as UiError;
  }
  throw new Error("expected a rejection");
}

describe("HID meters in the device list (AC-QFTCWZ.3)", () => {
  it("lists permitted 0x5FC9 HID meters as webhid:N, next to the demo", async () => {
    const hid = new FakeHidNavigator();
    const other = new FakeHidDevice();
    other.vendorId = 0x046d;
    hid.devices = [demoHidDevice(w), other];
    const backend = await backendWith(null, hid);
    const list = await settle(backend.listDevices());
    const meters = list.filter((d) => d.demo_scenario === null);
    expect(meters).toHaveLength(1);
    expect(meters[0]?.id).toMatch(/^webhid:\d+$/);
    expect(meters[0]?.model).toBe("KM003C");
    expect(list.some((d) => d.demo_scenario !== null)).toBe(true);
  });

  it("lists both WebUSB and WebHID meters under their own id prefixes", async () => {
    const usb = new FakeUsbNavigator();
    usb.devices = [demoUsbDevice(w, "123456")];
    const hid = new FakeHidNavigator();
    hid.devices = [demoHidDevice(w)];
    const backend = await backendWith(usb, hid);
    const ids = (await settle(backend.listDevices())).map((d) => d.id);
    expect(ids.filter((id) => id.startsWith("webusb:"))).toHaveLength(1);
    expect(ids.filter((id) => id.startsWith("webhid:"))).toHaveLength(1);
  });
});

describe("a HID session (AC-QFTCWZ.3)", () => {
  async function connectHid() {
    const hid = new FakeHidNavigator();
    const dev = demoHidDevice(w);
    hid.devices = [dev];
    const backend = await backendWith(null, hid);
    const id = (await settle(backend.listDevices())).find((d) => d.id.startsWith("webhid:"))?.id;
    if (!id) throw new Error("the HID meter is not listed");
    const events: BackendEvent[] = [];
    await settle(backend.connect(id, 50, (e) => events.push(e)));
    return { backend, dev, id, events };
  }

  it("streams ADC snapshots and sends only GET_DATA 0x001", async () => {
    const { backend, dev, events } = await connectHid();
    await vi.advanceTimersByTimeAsync(1_500);
    await settle(backend.disconnect());
    const samples = batches(events).flatMap((b) => b.samples);
    expect(samples.length).toBeGreaterThan(5);
    expect(samples.at(-1)?.vbus_v).toBeCloseTo(5, 0);
    expect(dev.sent.length).toBeGreaterThan(5);
    expect(adcOnly(hidSentNames(w, dev))).toEqual([]);
  });

  it("reports itself as basic readings only, with no queue rate", async () => {
    const { backend, id, events } = await connectHid();
    const connected = statuses(events).find((s) => s.state === "connected");
    expect(connected).toMatchObject({
      state: "connected",
      basic_readings_only: true,
      rate_locked: true,
      rate_sps: null,
    });
    const attached = await settle(backend.attach(() => {}, id));
    expect(attached).toMatchObject({
      basic_readings_only: true,
      rate_locked: true,
      rate_sps: null,
    });
    expect(HID_BASIC_NOTE).toBe("HID: basic readings only");
  });

  it("refuses PD capture and rate changes with no traffic", async () => {
    const { backend, dev, events } = await connectHid();
    await vi.advanceTimersByTimeAsync(250);
    await expect(settle(backend.pdStart())).rejects.toMatchObject({ kind: "usage" });
    await expect(settle(backend.setRate(1000))).rejects.toMatchObject({ kind: "usage" });
    await vi.advanceTimersByTimeAsync(500);
    expect(adcOnly(hidSentNames(w, dev))).toEqual([]);
    expect(events.some((e) => e.type === "pd_state")).toBe(false);
    expect(statuses(events).some((s) => s.state === "rate")).toBe(false);
  });
});

describe("the UI shows the HID limit (AC-QFTCWZ.3)", () => {
  it("a component renders HID_BASIC_NOTE for a basic_readings_only session", () => {
    const dir = new URL("../components/", import.meta.url);
    const sources = [
      readFileSync(new URL("../../App.svelte", import.meta.url), "utf8"),
      ...readdirSync(dir)
        .filter((f) => f.endsWith(".svelte"))
        .map((f) => readFileSync(new URL(f, dir), "utf8")),
    ];
    const showing = sources.filter(
      (src) => /\bHID_BASIC_NOTE\b/.test(src) && /\bbasic_readings_only\b/.test(src),
    );
    expect(showing.length).toBeGreaterThanOrEqual(1);
  });
});

describe("falling back to HID when WebUSB is busy (AC-QFTCWZ.3)", () => {
  it("connects an already-permitted HID meter when the WebUSB claim fails", async () => {
    const usb = new FakeUsbNavigator();
    const usbDev = busyUsbDevice();
    usb.devices = [usbDev];
    const hid = new FakeHidNavigator();
    const hidDev = demoHidDevice(w);
    hid.devices = [hidDev];
    const backend = await backendWith(usb, hid);
    const id = (await settle(backend.listDevices())).find((d) => d.id.startsWith("webusb:"))?.id;
    if (!id) throw new Error("the WebUSB meter is not listed");
    const events: BackendEvent[] = [];
    await settle(backend.connect(id, 50, (e) => events.push(e)));
    await vi.advanceTimersByTimeAsync(500);
    expect(statuses(events).find((s) => s.state === "connected")).toMatchObject({
      basic_readings_only: true,
    });
    expect(usbDev.outTransfers.filter((t) => t.data.length > 0)).toEqual([]);
    expect(hidDev.sent.length).toBeGreaterThan(0);
    expect(adcOnly(hidSentNames(w, hidDev))).toEqual([]);
    expect(batches(events).flatMap((b) => b.samples).length).toBeGreaterThan(0);
    await settle(backend.disconnect());
  });

  it("without a permitted HID meter, a browser with WebHID gets device_busy that names HID", async () => {
    const usb = new FakeUsbNavigator();
    usb.devices = [busyUsbDevice()];
    const hid = new FakeHidNavigator();
    const backend = await backendWith(usb, hid);
    const id = (await settle(backend.listDevices())).find((d) => d.id.startsWith("webusb:"))?.id;
    const error = await rejection(backend.connect(id ?? "", 50, () => {}));
    expect(error.kind).toBe("device_busy");
    expect(`${error.message} ${error.hint ?? ""}`).toMatch(/HID/);
    // Nothing was asked of the browser's HID picker without a click.
    expect(hid.requests).toEqual([]);
  });

  it("without WebHID, the error is a plain device_busy", async () => {
    const usb = new FakeUsbNavigator();
    usb.devices = [busyUsbDevice()];
    const backend = await backendWith(usb, null);
    const id = (await settle(backend.listDevices())).find((d) => d.id.startsWith("webusb:"))?.id;
    const error = await rejection(backend.connect(id ?? "", 50, () => {}));
    expect(error.kind).toBe("device_busy");
    expect(`${error.message} ${error.hint ?? ""}`).not.toMatch(/HID/);
  });

  it("stays device_busy when the HID meter cannot be opened either", async () => {
    const usb = new FakeUsbNavigator();
    usb.devices = [busyUsbDevice()];
    const hid = new FakeHidNavigator();
    const hidDev = demoHidDevice(w);
    hidDev.open = () =>
      Promise.reject(new DOMException("Failed to open the device.", "NotAllowedError"));
    hid.devices = [hidDev];
    const backend = await backendWith(usb, hid);
    const id = (await settle(backend.listDevices())).find((d) => d.id.startsWith("webusb:"))?.id;
    const error = await rejection(backend.connect(id ?? "", 50, () => {}));
    expect(error.kind).toBe("device_busy");
    expect(hidDev.sent).toEqual([]);
  });
});

describe("the HID picker (AC-QFTCWZ.3)", () => {
  it("requestHidMeter opens navigator.hid.requestDevice synchronously, filtered to 0x5FC9", async () => {
    const usb = new FakeUsbNavigator();
    const hid = new FakeHidNavigator();
    const backend = await backendWith(usb, hid);
    hid.pick = demoHidDevice(w);
    const pending = backend.requestHidMeter();
    pending.catch(() => {});
    expect(hid.requests).toEqual([{ filters: [{ vendorId: 0x5fc9 }] }]);
    expect(usb.requests).toEqual([]);
    const picked = await settle(pending);
    expect(picked?.id).toMatch(/^webhid:\d+$/);
    expect(picked?.demo_scenario).toBeNull();
    const ids = (await settle(backend.listDevices())).map((d) => d.id);
    expect(ids).toContain(picked?.id);
  });

  it("a cancelled HID picker resolves null", async () => {
    const hid = new FakeHidNavigator();
    const backend = await backendWith(new FakeUsbNavigator(), hid);
    hid.pick = null;
    await expect(settle(backend.requestHidMeter())).resolves.toBeNull();
  });

  it("requestHidMeter resolves null without WebHID", async () => {
    const backend = await backendWith(new FakeUsbNavigator(), null);
    await expect(settle(backend.requestHidMeter())).resolves.toBeNull();
  });

  it("a browser with WebHID but no WebUSB uses the HID picker for requestMeter", async () => {
    const hid = new FakeHidNavigator();
    const backend = await backendWith(null, hid);
    hid.pick = demoHidDevice(w);
    const pending = backend.requestMeter();
    // Before any await: the click's user activation must still hold.
    expect(hid.requests).toEqual([{ filters: [{ vendorId: 0x5fc9 }] }]);
    const picked = await settle(pending);
    expect(picked?.id).toMatch(/^webhid:\d+$/);
  });

  it("a browser with WebUSB keeps the WebUSB picker for requestMeter", async () => {
    const usb = new FakeUsbNavigator();
    const hid = new FakeHidNavigator();
    const backend = await backendWith(usb, hid);
    usb.pick = demoUsbDevice(w, "654321");
    const pending = backend.requestMeter();
    expect(usb.requests).toEqual([{ filters: [{ vendorId: 0x5fc9 }] }]);
    expect(hid.requests).toEqual([]);
    await settle(pending);
  });
});
