/**
 * FR W8SHX8: the web backend answers the settings read, the record list and the record
 * download in the browser.
 *
 * - AC-W8SHX8.1, .2, .4: over WebUSB (and the built-in demo meter) `meterSettings`,
 *   `recordsList` and `recordDownload` resolve with the desktop shapes, on the backend
 *   and on `meter(id)`; every frame sent is a safe read (no `unknown`).
 * - AC-W8SHX8.3: a download reads only the record's own window.
 * - AC-W8SHX8.6: a HID session refuses all three with the basic-readings wording.
 * - AC-W8SHX8.7: the session answers them itself (SESSION_COMMANDS), not the
 *   desktop-only refusal.
 *
 * Runs on Vitest's fake clock against fake devices answered by the wasm demo meter.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { UiError } from "../backend";
import type { PowerzWasm } from "./driver";
import type { FakeHidDevice, FakeUsbDevice } from "./fakeDevices.test";
import { SESSION_COMMANDS } from "./session";
import { loadWasm } from "./testWasm.test";
import { createWebBackend } from "./webBackend";
import {
  FakeHidNavigator,
  FakeUsbNavigator,
  demoHidDevice,
  demoUsbDevice,
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

async function usbBackend(): Promise<{ backend: Backend; dev: FakeUsbDevice; id: string }> {
  const nav = new FakeUsbNavigator();
  const dev = demoUsbDevice(w, "123456");
  nav.devices = [dev];
  const backend = await settle(
    createWebBackend({
      wasm: w,
      usb: nav,
      hid: null,
      download: downloadSink().download,
    } as unknown as Options),
  );
  const id = (await settle(backend.listDevices())).find((d) => d.serial === "123456")?.id;
  if (!id) throw new Error("the permitted meter is not listed");
  await settle(backend.connect(id, 50, () => {}));
  await vi.advanceTimersByTimeAsync(300);
  return { backend, dev, id };
}

const DEMO_NAMES = ["PHONE_5V", "PD_9V_TEST", "LAPTOP_20V"];

describe("session commands (AC-W8SHX8.7)", () => {
  it("the session answers the three reads itself", () => {
    const names: readonly string[] = SESSION_COMMANDS;
    for (const name of ["meterSettings", "recordsList", "recordDownload"]) {
      expect(names, name).toContain(name);
    }
  });
});

describe("over WebUSB (AC-W8SHX8.1, .2, .3, .4)", () => {
  it("reads the settings, lists the records and downloads one, on the backend and meter(id)", async () => {
    const { backend, dev, id } = await usbBackend();
    for (const m of [backend, backend.meter(id)]) {
      const settings = await settle(m.meterSettings());
      expect(settings.fields.length).toBeGreaterThan(5);
      expect(settings.fields.map((f) => f.key)).toContain("brightness");
      for (const f of settings.fields) {
        expect(Object.keys(f).sort()).toEqual(["key", "label", "tag", "value"]);
      }

      const list = await settle(m.recordsList());
      expect(list.records.map((r) => r.name)).toEqual(DEMO_NAMES);
      expect(list.records.map((r) => r.index)).toEqual([1, 2, 3]);

      const record = await settle(m.recordDownload(3));
      expect(record).toMatchObject({ index: 3, name: "LAPTOP_20V", interval_s: 5 });
      expect(record.samples).toHaveLength(60);
      expect(record.samples[2]?.t_s).toBe(10);
      expect(record.samples[0]?.vbus_v).toBeCloseTo(20, 0);
    }
    const names = sentNames(w, dev);
    expect(names).not.toContain("unknown");
    expect(names).toContain("get_data:settings");
    expect(names).toContain("get_data:record_list");
    // LAPTOP_20V: 60 points × 16 bytes = 960 bytes = 4 reads, twice.
    expect(names.filter((n) => n.startsWith("record_read:"))).toEqual(
      Array(8).fill("record_read:3"),
    );
    await settle(backend.disconnect());
  });

  it("refuses an unlisted record with the desktop wording", async () => {
    const { backend, dev } = await usbBackend();
    const error = await rejection(backend.recordDownload(7));
    expect(error).toMatchObject({ kind: "usage", message: "That record is not on the meter." });
    expect(sentNames(w, dev).some((n) => n.startsWith("record_read:"))).toBe(false);
    await settle(backend.disconnect());
  });
});

describe("the built-in demo meter (AC-W8SHX8.2)", () => {
  it("answers the settings read, the list and a download", async () => {
    const backend = await settle(
      createWebBackend({
        wasm: w,
        usb: null,
        hid: null,
        download: downloadSink().download,
      } as unknown as Options),
    );
    const demo = (await settle(backend.listDevices())).find((d) => d.demo_scenario !== null);
    if (!demo) throw new Error("no demo meter listed");
    await settle(backend.connect(demo.id, 50, () => {}));
    await vi.advanceTimersByTimeAsync(300);
    expect((await settle(backend.meterSettings())).fields.length).toBeGreaterThan(5);
    expect((await settle(backend.recordsList())).records.map((r) => r.name)).toEqual(DEMO_NAMES);
    expect((await settle(backend.recordDownload(1))).samples).toHaveLength(300);
    await settle(backend.disconnect());
  });
});

describe("over WebHID (AC-W8SHX8.6)", () => {
  it("refuses all three with the basic-readings wording and sends only snapshots", async () => {
    const hid = new FakeHidNavigator();
    const dev: FakeHidDevice = demoHidDevice(w);
    hid.devices = [dev];
    const backend = await settle(
      createWebBackend({
        wasm: w,
        usb: null,
        hid,
        download: downloadSink().download,
      } as unknown as Options),
    );
    const meter = (await settle(backend.listDevices())).find((d) => d.id.startsWith("webhid:"));
    if (!meter) throw new Error("the HID meter is not listed");
    await settle(backend.connect(meter.id, 50, () => {}));
    await vi.advanceTimersByTimeAsync(300);
    for (const call of [
      () => backend.meterSettings(),
      () => backend.recordsList(),
      () => backend.recordDownload(1),
    ]) {
      const error = await rejection(call());
      expect(error.kind).toBe("usage");
      expect(error.message).toMatch(/ is not available over HID: it gives basic readings only\.$/);
    }
    expect(hidSentNames(w, dev).filter((n) => n !== "get_data:adc")).toEqual([]);
    await settle(backend.disconnect());
  });
});
