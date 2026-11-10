/**
 * FR EHY1JX: the web device menu lists one meter twice.
 *
 * A meter the browser has permission for over both WebUSB and WebHID is one meter:
 * the device list groups HID meters by product id and leaves out as many as there are
 * WebUSB meters of that product, keeping HID meters with a live session first. The
 * WebUSB entry still falls back to the permitted HID meter when its interface is busy.
 *
 * Runs on Vitest's fake clock (the HID sessions poll on timers).
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { BackendEvent, DeviceListing, Status } from "../backend";
import { deviceOptionLabel } from "../format";
import type { PowerzWasm } from "./driver";
import type { FakeHidDevice, FakeUsbDevice } from "./fakeDevices.test";
import { loadWasm } from "./testWasm.test";
import { createWebBackend } from "./webBackend";
import {
  FakeHidNavigator,
  FakeUsbNavigator,
  demoHidDevice,
  demoUsbDevice,
  downloadSink,
  settle,
} from "./webBackendFakes.test";

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

const meters = (list: DeviceListing[]) => list.filter((d) => d.demo_scenario === null);
const usbIds = (list: DeviceListing[]) =>
  meters(list)
    .map((d) => d.id)
    .filter((id) => id.startsWith("webusb:"));
const hidIds = (list: DeviceListing[]) =>
  meters(list)
    .map((d) => d.id)
    .filter((id) => id.startsWith("webhid:"));
const statuses = (events: BackendEvent[]): Status[] =>
  events.flatMap((e) => (e.type === "status" ? [e.data] : []));

function usbMeter(serial = "123456", productId = 0x0063): FakeUsbDevice {
  const dev = demoUsbDevice(w, serial);
  dev.productId = productId;
  return dev;
}

function hidMeter(productId = 0x0063): FakeHidDevice {
  const dev = demoHidDevice(w);
  dev.productId = productId;
  return dev;
}

describe("one meter granted over USB and HID is listed once (AC-EHY1JX.1)", () => {
  it("lists one meter entry, the WebUSB one, for a WebUSB and a WebHID meter of one product", async () => {
    const usb = new FakeUsbNavigator();
    usb.devices = [usbMeter("123456")];
    const hid = new FakeHidNavigator();
    hid.devices = [hidMeter()];
    const backend = await backendWith(usb, hid);
    const list = await settle(backend.listDevices());
    expect(meters(list)).toHaveLength(1);
    expect(meters(list)[0]?.id).toMatch(/^webusb:\d+$/);
    expect(meters(list)[0]?.serial).toBe("123456");
    expect(hidIds(list)).toEqual([]);
    // The demo meter is still there.
    expect(list.some((d) => d.demo_scenario !== null)).toBe(true);
  });

  it("keeps the list at one meter when the hotplug watcher reports it", async () => {
    const usb = new FakeUsbNavigator();
    const hid = new FakeHidNavigator();
    const backend = await backendWith(usb, hid);
    const seen: DeviceListing[][] = [];
    const stop = await settle(backend.watchDevices((list) => seen.push(list)));
    hid.devices = [hidMeter()];
    usb.plug(usbMeter("123456"));
    await vi.advanceTimersByTimeAsync(50);
    const last = seen.at(-1) ?? [];
    expect(usbIds(last)).toHaveLength(1);
    expect(hidIds(last)).toEqual([]);
    stop();
  });
});

describe("HID meters beyond the WebUSB ones are listed and say HID (AC-EHY1JX.2)", () => {
  it("lists a HID-only meter", async () => {
    const hid = new FakeHidNavigator();
    hid.devices = [hidMeter()];
    const backend = await backendWith(new FakeUsbNavigator(), hid);
    const list = await settle(backend.listDevices());
    expect(usbIds(list)).toEqual([]);
    expect(hidIds(list)).toHaveLength(1);
  });

  it("lists one of two HID meters next to one WebUSB meter of the same product", async () => {
    const usb = new FakeUsbNavigator();
    usb.devices = [usbMeter("123456")];
    const hid = new FakeHidNavigator();
    hid.devices = [hidMeter(), hidMeter()];
    const backend = await backendWith(usb, hid);
    const list = await settle(backend.listDevices());
    expect(usbIds(list)).toHaveLength(1);
    expect(hidIds(list)).toHaveLength(1);
    expect(meters(list)).toHaveLength(2);
  });

  it("counts WebUSB meters per product: a HID meter of another product stays listed", async () => {
    const usb = new FakeUsbNavigator();
    usb.devices = [usbMeter("123456", 0x0063)];
    const hid = new FakeHidNavigator();
    hid.devices = [hidMeter(0x0064), hidMeter(0x0063)];
    const backend = await backendWith(usb, hid);
    const list = await settle(backend.listDevices());
    expect(usbIds(list)).toHaveLength(1);
    expect(hidIds(list)).toHaveLength(1);
  });

  it("reads the model followed by (HID) in the device menu, serials shown or hidden", async () => {
    const usb = new FakeUsbNavigator();
    usb.devices = [usbMeter("123456")];
    const hid = new FakeHidNavigator();
    hid.devices = [hidMeter(), hidMeter()];
    const backend = await backendWith(usb, hid);
    const list = await settle(backend.listDevices());
    const hidEntry = meters(list).find((d) => d.id.startsWith("webhid:"));
    if (!hidEntry) throw new Error("the extra HID meter is not listed");
    expect(deviceOptionLabel(hidEntry, false, list)).toBe("KM003C (HID)");
    expect(deviceOptionLabel(hidEntry, true, list)).toMatch(/^KM003C\b.*\(HID\)$/);
    // The WebUSB entry does not say HID.
    const usbEntry = meters(list).find((d) => d.id.startsWith("webusb:"));
    if (!usbEntry) throw new Error("the WebUSB meter is not listed");
    expect(deviceOptionLabel(usbEntry, false, list)).toBe("KM003C — 123456");
    expect(deviceOptionLabel(usbEntry, true, list)).not.toContain("HID");
  });
});

describe("a HID meter with a live session stays listed (AC-EHY1JX.3)", () => {
  it("lists the connected HID meter next to a WebUSB meter of the same product", async () => {
    const usb = new FakeUsbNavigator();
    usb.devices = [usbMeter("123456")];
    const hid = new FakeHidNavigator();
    const hidDev = hidMeter();
    hid.devices = [hidDev];
    hid.pick = hidDev;
    const backend = await backendWith(usb, hid);
    const picked = await settle(backend.requestHidMeter());
    if (!picked) throw new Error("the HID picker returned no meter");
    expect(picked.id).toMatch(/^webhid:\d+$/);
    await settle(backend.connect(picked.id, 50, () => {}));
    await vi.advanceTimersByTimeAsync(200);

    const list = await settle(backend.listDevices());
    expect(hidIds(list)).toEqual([picked.id]);
    expect(usbIds(list)).toHaveLength(1);

    await settle(backend.disconnect());
    const after = await settle(backend.listDevices());
    expect(hidIds(after)).toEqual([]);
    expect(usbIds(after)).toHaveLength(1);
  });

  it("keeps the connected one of two HID meters listed, not the other", async () => {
    const usb = new FakeUsbNavigator();
    usb.devices = [usbMeter("123456")];
    const hid = new FakeHidNavigator();
    const first = hidMeter();
    const second = hidMeter();
    hid.devices = [first, second];
    hid.pick = second;
    const backend = await backendWith(usb, hid);
    const picked = await settle(backend.requestHidMeter());
    if (!picked) throw new Error("the HID picker returned no meter");
    await settle(backend.connect(picked.id, 50, () => {}));
    await vi.advanceTimersByTimeAsync(200);
    expect(second.sent.length).toBeGreaterThan(0);

    const list = await settle(backend.listDevices());
    expect(hidIds(list)).toEqual([picked.id]);
    expect(usbIds(list)).toHaveLength(1);
    await settle(backend.disconnect());
  });
});

describe("the WebUSB entry still falls back to HID when busy (AC-EHY1JX.4)", () => {
  it("connects the permitted HID meter when another program holds the WebUSB interface", async () => {
    const usb = new FakeUsbNavigator();
    const usbDev = usbMeter("123456");
    usbDev.claimError = new DOMException("Unable to claim interface.", "NetworkError");
    usb.devices = [usbDev];
    const hid = new FakeHidNavigator();
    const hidDev = hidMeter();
    hid.devices = [hidDev];
    const backend = await backendWith(usb, hid);
    const list = await settle(backend.listDevices());
    const id = usbIds(list)[0];
    if (!id) throw new Error("the WebUSB meter is not listed");
    const events: BackendEvent[] = [];
    await settle(backend.connect(id, 50, (e) => events.push(e)));
    await vi.advanceTimersByTimeAsync(500);
    expect(statuses(events).find((s) => s.state === "connected")).toMatchObject({
      basic_readings_only: true,
    });
    expect(usbDev.outTransfers.filter((t) => t.data.length > 0)).toEqual([]);
    expect(hidDev.sent.length).toBeGreaterThan(0);
    await settle(backend.disconnect());
  });
});
