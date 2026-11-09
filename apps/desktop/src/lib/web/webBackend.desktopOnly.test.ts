/**
 * FR QFTCX0: what the web backend refuses.
 *
 * - AC-QFTCX0.6: firmware update, logo upload and protocol triggering are
 *   desktop-only (settings writes run in the browser, AC-W8SHX9.7, and so does the
 *   record delete, AC-W8SHXA.6): each web backend function for them rejects
 *   with a "desktop app" usage error and sends nothing to the meter; the backend's
 *   `features` map says so.
 * - AC-QFTCX0.7: settings read, the offline record list and record download work only
 *   over the safe read set; otherwise they are desktop-only too.
 *
 * Runs on Vitest's fake clock against a fake WebUSB meter answered by the demo meter.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { UiError } from "../backend";
import type { PowerzWasm } from "./driver";
import type { FakeUsbDevice } from "./fakeDevices.test";
import { WEB_FEATURES } from "./features";
import { loadWasm } from "./testWasm.test";
import { createWebBackend } from "./webBackend";
import {
  FakeUsbNavigator,
  demoUsbDevice,
  downloadSink,
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

async function connected(): Promise<{ backend: Backend; dev: FakeUsbDevice; id: string }> {
  const nav = new FakeUsbNavigator();
  const dev = demoUsbDevice(w, "123456");
  nav.devices = [dev];
  const backend = await settle(
    createWebBackend({
      wasm: w,
      usb: nav,
      download: downloadSink().download,
    } as unknown as Options),
  );
  const id = (await settle(backend.listDevices())).find((d) => d.serial === "123456")?.id;
  if (!id) throw new Error("the permitted meter is not listed");
  await settle(backend.connect(id, 50, () => {}));
  await vi.advanceTimersByTimeAsync(300);
  return { backend, dev, id };
}

/** Frames the meter received so far (zero-length packets left out). */
const frameCount = (dev: FakeUsbDevice) => dev.outTransfers.filter((t) => t.data.length > 0).length;

async function expectDesktopOnly(call: () => Promise<unknown>, dev: FakeUsbDevice): Promise<void> {
  const before = frameCount(dev);
  // No timer runs here, so the stream's own polls add no frames meanwhile.
  const error = await call().then(
    () => null,
    (e: unknown) => e as UiError,
  );
  expect(error, "the call resolved").not.toBeNull();
  expect(error?.kind).toBe("usage");
  expect(error?.message).toMatch(/desktop app/i);
  expect(frameCount(dev)).toBe(before);
}

describe("desktop-only meter writes (AC-QFTCX0.6)", () => {
  it("the backend's features map is the web one", async () => {
    const { backend } = await connected();
    expect(backend.features).toEqual(WEB_FEATURES);
    await settle(backend.disconnect());
  });

  it("firmware update and logo writes reject without sending anything", async () => {
    const { backend, dev, id } = await connected();
    await expectDesktopOnly(() => backend.firmwareFlash(id, "fw.bin", "FLASH", () => {}), dev);
    await expectDesktopOnly(() => backend.logoUpload(id, "logo.png", true, () => {}), dev);
    await expectDesktopOnly(() => backend.logoUploadBuiltin(id, "surf", () => {}), dev);
    await expectDesktopOnly(() => backend.logoRemove(id, () => {}), dev);
    await settle(backend.disconnect());
  });

  // Settings writes run in the browser since FR W8SHX9 (AC-W8SHX9.7), record deletes
  // since FR W8SHXA (AC-W8SHXA.6).
  it("triggering rejects without sending anything", async () => {
    const { backend, dev, id } = await connected();
    for (const m of [backend, backend.meter(id)]) {
      await expectDesktopOnly(() => m.triggerOpen(), dev);
      await expectDesktopOnly(() => m.triggerDetect(), dev);
      await expectDesktopOnly(() => m.triggerRequest({ protocol: "qc", volts: 9 }, true), dev);
      await expectDesktopOnly(
        () => m.triggerMessageSend({ sop: "sop", msg_type: 7, data_objects: [] }, true),
        dev,
      );
      await expectDesktopOnly(() => m.triggerCableRead(), dev);
      await expectDesktopOnly(() => m.triggerConsoleSend("pdm open", true), dev);
    }
    // Over the whole session, every frame was a safe read or stream request.
    expect(sentNames(w, dev)).not.toContain("unknown");
    await settle(backend.disconnect());
  });
});

describe("record delete runs in the browser (AC-W8SHXA.6)", () => {
  it("is not the desktop-only refusal, and a wrong confirmation writes nothing", async () => {
    const { backend, dev, id } = await connected();
    for (const m of [backend, backend.meter(id)]) {
      const error = await settle(m.recordsDelete(1, "DELETE 1"), 20_000).then(
        () => null,
        (e: unknown) => e as UiError,
      );
      expect(error?.kind).toBe("usage");
      expect(error?.message).not.toMatch(/desktop app/i);
      expect(error?.message).toBe("The typed confirmation does not match; nothing was deleted.");
    }
    expect(sentNames(w, dev)).not.toContain("unknown");
    expect(sentNames(w, dev)).not.toContain("record_table_write");
    await settle(backend.disconnect());
  });
});

describe("meter reads (AC-QFTCX0.7)", () => {
  const reads = [
    ["settingsRead", (b: Backend): Promise<unknown> => b.meterSettings()],
    ["recordsList", (b: Backend): Promise<unknown> => b.recordsList()],
    ["recordDownload", (b: Backend): Promise<unknown> => b.recordDownload(1)],
  ] as const;

  for (const [feature, call] of reads) {
    it(`${feature} uses only the safe read set, or is desktop-only`, async () => {
      const { backend, dev } = await connected();
      if (WEB_FEATURES[feature]) {
        await settle(call(backend)).catch(() => {});
        expect(sentNames(w, dev)).not.toContain("unknown");
      } else {
        await expectDesktopOnly(() => call(backend), dev);
      }
      await settle(backend.disconnect());
    });
  }
});
