/**
 * FR W8SHXA: the web backend deletes offline records in the browser.
 *
 * - AC-W8SHXA.2: each backup is a CSV download (through the backend's `download`
 *   option, type text/csv) named as the desktop app names its backup files, made
 *   before the table write.
 * - AC-W8SHXA.5: `recordsDelete` answers `{ records, backup_dir, backups }` with the
 *   records after the delete and the backup file names, on the backend and on
 *   `meter(id)`, over WebUSB and the built-in demo meter.
 * - AC-W8SHXA.6: the session answers `recordsDelete` itself (SESSION_COMMANDS), not
 *   the desktop-only refusal; every frame sent is a safe read or the record-table
 *   write; a HID session refuses it.
 *
 * Runs on Vitest's fake clock against fake devices answered by the wasm demo meter.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { UiError } from "../backend";
import type { PowerzWasm } from "./driver";
import type { FakeUsbDevice } from "./fakeDevices.test";
import { WEB_FEATURES } from "./features";
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
  type Download,
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

const BACKUP_DIR = "your browser's downloads";

async function usbBackend(): Promise<{
  backend: Backend;
  dev: FakeUsbDevice;
  id: string;
  downloads: Download[];
}> {
  const nav = new FakeUsbNavigator();
  const dev = demoUsbDevice(w, "123456");
  nav.devices = [dev];
  const { downloads, download } = downloadSink();
  const backend = await settle(
    createWebBackend({ wasm: w, usb: nav, hid: null, download } as unknown as Options),
  );
  const id = (await settle(backend.listDevices())).find((d) => d.serial === "123456")?.id;
  if (!id) throw new Error("the permitted meter is not listed");
  await settle(backend.connect(id, 50, () => {}));
  await vi.advanceTimersByTimeAsync(300);
  return { backend, dev, id, downloads };
}

describe("session command (AC-W8SHXA.6)", () => {
  it("the session answers recordsDelete itself and the web build switches it on", () => {
    const names: readonly string[] = SESSION_COMMANDS;
    expect(names).toContain("recordsDelete");
    expect(WEB_FEATURES.recordDelete).toBe(true);
  });
});

describe("over WebUSB (AC-W8SHXA.2, .5, .6)", () => {
  it("deletes one record after a CSV backup download, on the backend", async () => {
    const { backend, dev, downloads } = await usbBackend();
    const result = await settle(backend.recordsDelete(1, "PHONE_5V"), 20_000);
    expect(result).toEqual({
      records: expect.any(Array) as unknown,
      backup_dir: BACKUP_DIR,
      backups: ["01_PHONE_5V.csv"],
    });
    expect(result.records.map((r) => [r.index, r.name])).toEqual([
      [1, "PD_9V_TEST"],
      [2, "LAPTOP_20V"],
    ]);
    expect(downloads.map((d) => d.name)).toEqual(["01_PHONE_5V.csv"]);
    const [backup] = downloads;
    if (!backup) throw new Error("no backup download");
    expect(backup.blob.type).toBe("text/csv");
    const csv = await backup.blob.text();
    // A header line plus one line per sample (PHONE_5V has 300).
    expect(csv.trimEnd().split("\n")).toHaveLength(301);

    const names = sentNames(w, dev);
    expect(names).not.toContain("unknown");
    expect(names.filter((n) => n === "record_table_write")).toHaveLength(1);
    expect((await settle(backend.recordsList())).records.map((r) => r.name)).toEqual([
      "PD_9V_TEST",
      "LAPTOP_20V",
    ]);
    await settle(backend.disconnect());
  });

  it("deletes all of them on meter(id), one download per record", async () => {
    const { backend, dev, id, downloads } = await usbBackend();
    const result = await settle(backend.meter(id).recordsDelete("all", "all"), 20_000);
    expect(result).toEqual({
      records: [],
      backup_dir: BACKUP_DIR,
      backups: ["01_PHONE_5V.csv", "02_PD_9V_TEST.csv", "03_LAPTOP_20V.csv"],
    });
    expect(downloads.map((d) => d.name)).toEqual(result.backups);
    for (const d of downloads) expect(d.blob.type).toBe("text/csv");
    expect(sentNames(w, dev)).not.toContain("unknown");
    await settle(backend.disconnect());
  });

  it("a wrong confirmation downloads nothing and writes nothing", async () => {
    const { backend, dev, downloads } = await usbBackend();
    const error = await rejection(backend.recordsDelete(1, "DELETE 1"));
    expect(error).toMatchObject({
      kind: "usage",
      message: "The typed confirmation does not match; nothing was deleted.",
    });
    expect(error.message).not.toMatch(/desktop app/i);
    expect(downloads).toEqual([]);
    expect(sentNames(w, dev)).not.toContain("record_table_write");
    await settle(backend.disconnect());
  });
});

describe("the built-in demo meter (AC-W8SHXA.5)", () => {
  it("deletes a record and lists the rest", async () => {
    const { downloads, download } = downloadSink();
    const backend = await settle(
      createWebBackend({ wasm: w, usb: null, hid: null, download } as unknown as Options),
    );
    const demo = (await settle(backend.listDevices())).find((d) => d.demo_scenario !== null);
    if (!demo) throw new Error("no demo meter listed");
    await settle(backend.connect(demo.id, 50, () => {}));
    await vi.advanceTimersByTimeAsync(300);
    const result = await settle(backend.recordsDelete(2, "PD_9V_TEST"), 20_000);
    expect(result.records.map((r) => r.name)).toEqual(["PHONE_5V", "LAPTOP_20V"]);
    expect(result.backups).toEqual(["02_PD_9V_TEST.csv"]);
    expect(downloads.map((d) => d.name)).toEqual(["02_PD_9V_TEST.csv"]);
    await settle(backend.disconnect());
  });
});

describe("over WebHID (AC-W8SHXA.6)", () => {
  it("refuses with the basic-readings wording and sends only snapshots", async () => {
    const hid = new FakeHidNavigator();
    const dev = demoHidDevice(w);
    hid.devices = [dev];
    const { downloads, download } = downloadSink();
    const backend = await settle(
      createWebBackend({ wasm: w, usb: null, hid, download } as unknown as Options),
    );
    const meter = (await settle(backend.listDevices())).find((d) => d.id.startsWith("webhid:"));
    if (!meter) throw new Error("the HID meter is not listed");
    await settle(backend.connect(meter.id, 50, () => {}));
    await vi.advanceTimersByTimeAsync(300);
    const error = await rejection(backend.recordsDelete("all", "all"));
    expect(error.kind).toBe("usage");
    expect(error.message).toBe(
      "Deleting offline records is not available over HID: it gives basic readings only.",
    );
    expect(hidSentNames(w, dev).filter((n) => n !== "get_data:adc")).toEqual([]);
    expect(downloads).toEqual([]);
    await settle(backend.disconnect());
  });
});
