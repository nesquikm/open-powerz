/**
 * FR QFTCX0, AC-QFTCX0.2: the web backend's device list.
 *
 * The list holds the demo meter plus the meters the browser already has permission for;
 * "Connect a meter…" (`requestMeter`) opens the WebUSB picker filtered to vendor 0x5FC9
 * synchronously, inside the click handler, and the picked meter joins the list;
 * plugging and unplugging a permitted meter updates the list.
 */

import { beforeEach, describe, expect, it } from "vitest";
import type { DeviceListing } from "../backend";
import type { PowerzWasm } from "./driver";
import { loadWasm } from "./testWasm.test";
import { createWebBackend } from "./webBackend";
import { FakeUsbNavigator, demoUsbDevice, downloadSink } from "./webBackendFakes.test";

type Options = Parameters<typeof createWebBackend>[0];

let w: PowerzWasm;

beforeEach(async () => {
  w = await loadWasm();
});

function backendWith(usb: FakeUsbNavigator | null) {
  return createWebBackend({
    wasm: w,
    usb,
    download: downloadSink().download,
  } as unknown as Options);
}

const serials = (list: DeviceListing[]) =>
  list.filter((d) => d.demo_scenario === null).map((d) => d.serial);

/** Let the hotplug handlers' `getDevices()` round trips finish. */
async function flush(): Promise<void> {
  for (let i = 0; i < 20; i++) await Promise.resolve();
  await new Promise((r) => setTimeout(r, 0));
}

describe("device list (AC-QFTCX0.2)", () => {
  it("lists the demo meter and the meters the browser already has permission for", async () => {
    const nav = new FakeUsbNavigator();
    nav.devices = [demoUsbDevice(w, "123456")];
    const backend = await backendWith(nav);
    const list = await backend.listDevices();
    const demos = list.filter((d) => d.demo_scenario !== null);
    expect(demos.length).toBeGreaterThanOrEqual(1);
    expect(serials(list)).toEqual(["123456"]);
    const meter = list.find((d) => d.serial === "123456");
    expect(meter?.mode).toBe("application");
    expect(meter?.label).toContain("123456");
    // Every listing has its own id.
    expect(new Set(list.map((d) => d.id)).size).toBe(list.length);
  });

  it("lists only the demo meter when the browser has no WebUSB", async () => {
    const backend = await backendWith(null);
    const list = await backend.listDevices();
    expect(list.length).toBeGreaterThanOrEqual(1);
    expect(list.every((d) => d.demo_scenario !== null)).toBe(true);
  });

  it("opens the picker filtered to 0x5FC9 synchronously and adds the picked meter", async () => {
    const nav = new FakeUsbNavigator();
    const backend = await backendWith(nav);
    nav.pick = demoUsbDevice(w, "654321");
    const pending = backend.requestMeter();
    // No await before requestDevice: the click's user activation must still hold.
    expect(nav.requests).toEqual([{ filters: [{ vendorId: 0x5fc9 }] }]);
    const picked = await pending;
    expect(picked?.serial).toBe("654321");
    expect(picked?.demo_scenario).toBeNull();
    expect(serials(await backend.listDevices())).toContain("654321");
  });

  it("resolves null and leaves the list alone when the picker is cancelled", async () => {
    const nav = new FakeUsbNavigator();
    nav.devices = [demoUsbDevice(w, "123456")];
    const backend = await backendWith(nav);
    const before = await backend.listDevices();
    nav.pick = null;
    await expect(backend.requestMeter()).resolves.toBeNull();
    expect(await backend.listDevices()).toEqual(before);
  });

  it("updates the list when a permitted meter is plugged in and unplugged", async () => {
    const nav = new FakeUsbNavigator();
    const backend = await backendWith(nav);
    const seen: DeviceListing[][] = [];
    const stop = await backend.watchDevices((list) => seen.push(list));
    const dev = demoUsbDevice(w, "424242");

    nav.plug(dev);
    await flush();
    expect(serials(seen.at(-1) ?? [])).toEqual(["424242"]);
    expect((seen.at(-1) ?? []).some((d) => d.demo_scenario !== null)).toBe(true);

    nav.unplug(dev);
    await flush();
    expect(serials(seen.at(-1) ?? [])).toEqual([]);

    stop();
    const count = seen.length;
    nav.plug(dev);
    await flush();
    expect(seen.length).toBe(count);
  });
});
