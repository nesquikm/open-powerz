/**
 * FR W8SHXB: the web backend runs the startup logo flow in the browser.
 *
 * - AC-W8SHXB.1: `logoChoose` opens the injected file picker and answers a token that
 *   `logoPreview` / `logoUpload` take in place of a path; the preview is the wasm core's
 *   (the desktop app's) for the chosen bytes, a `.bin` name reads a raw logo file, and
 *   `logoPreviewBuiltin` is the core's built-in preview. A cancelled picker answers
 *   `null`; an unknown token is a usage error and sends nothing.
 * - AC-W8SHXB.2/.3: `logoUpload`, `logoUploadBuiltin` and `logoRemove` run over WebUSB
 *   and the built-in demo meter, after the settings were read; every frame is a safe
 *   read, a logo chunk inside the region, the duration write or a logo-window read.
 * - AC-W8SHXB.5/.6: `logoRead` and `logoSettingsCheck` answer in the browser.
 * - AC-W8SHXB.7: a HID session refuses every logo command with the basic-readings
 *   wording; the web build switches the logo feature on (firmware update stays off;
 *   triggering is on since AC-W8SHXC.7).
 *
 * Runs on Vitest's fake clock against fake devices answered by the wasm demo meter.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  LogoPreview,
  LogoProgress,
  LogoRead,
  LogoSettingsCheck,
  LogoWritten,
  UiError,
} from "../backend";
import type { PowerzWasm } from "./driver";
import type { FakeUsbDevice } from "./fakeDevices.test";
import { WEB_FEATURES } from "./features";
import type { PickLogoFile } from "./logoFiles";
import { bmp, loadWasm, logoWasm } from "./testWasm.test";
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

/** The backend with the FR W8SHXB commands. */
type Backend = Awaited<ReturnType<typeof createWebBackend>> & {
  logoChoose(): Promise<string | null>;
  logoPreview(path: string, alpha: boolean): Promise<LogoPreview>;
  logoPreviewBuiltin(name: string): Promise<LogoPreview>;
  logoRead(deviceId: string, full: boolean): Promise<LogoRead>;
  logoUpload(
    deviceId: string,
    path: string,
    alpha: boolean,
    onProgress: (p: LogoProgress) => void,
  ): Promise<LogoWritten>;
  logoUploadBuiltin(
    deviceId: string,
    name: string,
    onProgress: (p: LogoProgress) => void,
  ): Promise<LogoWritten>;
  logoRemove(deviceId: string, onProgress: (p: LogoProgress) => void): Promise<LogoWritten>;
  logoSettingsCheck(deviceId: string): Promise<LogoSettingsCheck>;
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

async function rejection(p: Promise<unknown>): Promise<UiError> {
  try {
    await settle(p, 30_000);
  } catch (e) {
    return e as UiError;
  }
  throw new Error("expected a rejection");
}

/** A picker that answers `files` in turn (then cancels). */
function picker(files: { name: string; bytes: Uint8Array }[]) {
  const calls: number[] = [];
  let i = 0;
  const pick: PickLogoFile = async () => {
    calls.push(i);
    return files[i++] ?? null;
  };
  return { pick, calls };
}

const PICTURE = bmp(100, 60);

async function usbBackend(files: { name: string; bytes: Uint8Array }[] = []): Promise<{
  backend: Backend;
  dev: FakeUsbDevice;
  id: string;
  calls: number[];
}> {
  const nav = new FakeUsbNavigator();
  const dev = demoUsbDevice(w, "123456");
  nav.devices = [dev];
  const { pick, calls } = picker(files);
  const backend = (await settle(
    createWebBackend({
      wasm: w,
      usb: nav,
      hid: null,
      download: downloadSink().download,
      pickLogoFile: pick,
    } as unknown as Options),
  )) as Backend;
  const id = (await settle(backend.listDevices())).find((d) => d.serial === "123456")?.id;
  if (!id) throw new Error("the permitted meter is not listed");
  await settle(backend.connect(id, 50, () => {}));
  await vi.advanceTimersByTimeAsync(300);
  return { backend, dev, id, calls };
}

const frameCount = (dev: FakeUsbDevice) => dev.outTransfers.filter((t) => t.data.length > 0).length;

describe("the feature map (AC-W8SHXB.7)", () => {
  it("the web build switches the logo feature on; firmware update stays off", () => {
    expect(WEB_FEATURES.logoUpload).toBe(true);
    expect(WEB_FEATURES.firmwareUpdate).toBe(false);
  });
});

describe("choose and preview (AC-W8SHXB.1)", () => {
  it("previews the chosen picture as the core (the desktop app) does", async () => {
    const { backend, dev, calls } = await usbBackend([{ name: "logo.bmp", bytes: PICTURE }]);
    const token = await settle(backend.logoChoose());
    expect(calls).toHaveLength(1);
    expect(token).toMatch(/^web-file:\d+:logo\.bmp$/);
    const before = frameCount(dev);
    for (const alpha of [false, true]) {
      const preview = await settle(backend.logoPreview(token as string, alpha));
      expect(preview).toEqual(logoWasm(w).logoPreview(PICTURE, false, alpha));
    }
    expect(frameCount(dev)).toBe(before);
    await settle(backend.disconnect());
  });

  it("a .bin name reads the bytes as a raw logo file", async () => {
    const { backend } = await usbBackend([
      { name: "logo.bin", bytes: PICTURE },
      { name: "logo.BMP", bytes: PICTURE },
    ]);
    const bin = await settle(backend.logoChoose());
    const pic = await settle(backend.logoChoose());
    // A BMP is no raw logo file.
    expect((await rejection(backend.logoPreview(bin as string, false))).kind).toBe("usage");
    expect((await settle(backend.logoPreview(pic as string, false))).chunks).toBe(47);
    await settle(backend.disconnect());
  });

  it("a cancelled picker answers null", async () => {
    const { backend } = await usbBackend([]);
    expect(await settle(backend.logoChoose())).toBeNull();
    await settle(backend.disconnect());
  });

  it("an unknown token is a usage error and sends nothing", async () => {
    const { backend, dev, id } = await usbBackend([]);
    await settle(backend.meterSettings());
    const before = frameCount(dev);
    for (const call of [
      () => backend.logoPreview("/tmp/logo.png", false),
      () => backend.logoPreview("web-file:41:logo.png", false),
      () => backend.logoUpload(id, "/tmp/logo.png", false, () => {}),
    ]) {
      const error = await rejection(call());
      expect(error.kind).toBe("usage");
      expect(error.message).not.toMatch(/desktop app/i);
    }
    expect(frameCount(dev)).toBe(before);
    await settle(backend.disconnect());
  });

  it("the built-in logos preview as the core does", async () => {
    const { backend } = await usbBackend();
    for (const name of ["surf", "surf-wordmark"]) {
      const preview = await settle(backend.logoPreviewBuiltin(name));
      expect(preview).toEqual(logoWasm(w).logoPreviewBuiltin(name));
      expect(preview.mode).toBe("alpha");
    }
    await settle(backend.disconnect());
  });
});

describe("over WebUSB (AC-W8SHXB.2, .3, .5, .6)", () => {
  it("uploads the chosen picture, reads it back and checks the settings", async () => {
    const { backend, dev, id } = await usbBackend([{ name: "logo.bmp", bytes: PICTURE }]);
    const token = (await settle(backend.logoChoose())) as string;
    await settle(backend.meterSettings());
    const progress: LogoProgress[] = [];
    const written = await settle(
      backend.logoUpload(id, token, false, (p) => progress.push(p)),
      30_000,
    );
    expect(written.duration_set).toBe("3 s");
    expect(written.settings).toBe("needs_menu_save");
    expect(progress[0]).toEqual({ step: "chunks", done: 1, total: 47 });
    expect(progress.some((p) => p.step === "read_back")).toBe(true);

    const names = sentNames(w, dev);
    expect(names).not.toContain("unknown");
    expect(names.filter((n) => n.startsWith("logo_chunk:"))).toHaveLength(47);
    expect(names.filter((n) => n === "logo_flag:2")).toHaveLength(1);

    const read = await settle(backend.logoRead(id, true), 30_000);
    expect(read.image).toBe(logoWasm(w).logoPreview(PICTURE, false, false).rgba);
    expect(await settle(backend.logoSettingsCheck(id))).toEqual({
      settings: "needs_menu_save",
      startup_duration: null,
    });
    await settle(backend.disconnect());
  });

  it("uploads a built-in logo and removes it", async () => {
    const { backend, dev, id } = await usbBackend();
    await settle(backend.meterSettings());
    const builtin = await settle(
      backend.logoUploadBuiltin(id, "surf", () => {}),
      30_000,
    );
    expect(builtin.duration_set).toBe("3 s");
    expect((await settle(backend.logoRead(id, false))).mode).toBe("alpha");
    const removed = await settle(
      backend.logoRemove(id, () => {}),
      30_000,
    );
    expect(removed.duration_set).toBe("off");
    expect((await settle(backend.logoRead(id, false))).status).toBe("empty");
    const names = sentNames(w, dev);
    expect(names).not.toContain("unknown");
    expect(names.filter((n) => n === "logo_flag:0")).toHaveLength(1);
    await settle(backend.disconnect());
  });

  it("refuses before any write when the settings were not read", async () => {
    const { backend, dev, id } = await usbBackend();
    const error = await rejection(backend.logoUploadBuiltin(id, "surf", () => {}));
    expect(error).toEqual({
      kind: "usage",
      message: "The logo was not written: SETTINGS was not read in this session.",
      hint: "Replug the meter so the app reads it again, then retry.",
    });
    const names = sentNames(w, dev);
    expect(names.filter((n) => n.startsWith("logo_chunk:") || n.startsWith("logo_flag:"))).toEqual(
      [],
    );
    await settle(backend.disconnect());
  });

  it("with no write waiting the settings check refuses", async () => {
    const { backend, id } = await usbBackend();
    expect(await rejection(backend.logoSettingsCheck(id))).toMatchObject({
      kind: "usage",
      message: "No logo write is waiting for a settings save.",
    });
    await settle(backend.disconnect());
  });
});

describe("the built-in demo meter (AC-W8SHXB.3)", () => {
  it("takes an upload and reads it back", async () => {
    const backend = (await settle(
      createWebBackend({
        wasm: w,
        usb: null,
        hid: null,
        download: downloadSink().download,
        pickLogoFile: picker([]).pick,
      } as unknown as Options),
    )) as Backend;
    const demo = (await settle(backend.listDevices())).find((d) => d.demo_scenario !== null);
    if (!demo) throw new Error("no demo meter listed");
    await settle(backend.connect(demo.id, 50, () => {}));
    await vi.advanceTimersByTimeAsync(300);
    await settle(backend.meterSettings());
    await settle(
      backend.logoUploadBuiltin(demo.id, "surf-wordmark", () => {}),
      30_000,
    );
    const read = await settle(backend.logoRead(demo.id, true), 30_000);
    expect(read.image).toBe(logoWasm(w).logoPreviewBuiltin("surf-wordmark").rgba);
    await settle(backend.disconnect());
  });
});

describe("over WebHID (AC-W8SHXB.7)", () => {
  it("refuses every logo command with the basic-readings wording", async () => {
    const hid = new FakeHidNavigator();
    const dev = demoHidDevice(w);
    hid.devices = [dev];
    const { pick } = picker([{ name: "logo.bmp", bytes: PICTURE }]);
    const backend = (await settle(
      createWebBackend({
        wasm: w,
        usb: null,
        hid,
        download: downloadSink().download,
        pickLogoFile: pick,
      } as unknown as Options),
    )) as Backend;
    const meter = (await settle(backend.listDevices())).find((d) => d.id.startsWith("webhid:"));
    if (!meter) throw new Error("the HID meter is not listed");
    await settle(backend.connect(meter.id, 50, () => {}));
    await vi.advanceTimersByTimeAsync(300);
    const token = (await settle(backend.logoChoose())) as string;
    for (const call of [
      () => backend.logoUpload(meter.id, token, false, () => {}),
      () => backend.logoUploadBuiltin(meter.id, "surf", () => {}),
      () => backend.logoRemove(meter.id, () => {}),
      () => backend.logoRead(meter.id, false),
      () => backend.logoSettingsCheck(meter.id),
    ]) {
      const error = await rejection(call());
      expect(error.kind).toBe("usage");
      expect(error.message).toBe(
        "The startup logo is not available over HID: it gives basic readings only.",
      );
    }
    expect(hidSentNames(w, dev).filter((n) => n !== "get_data:adc")).toEqual([]);
    await settle(backend.disconnect());
  });
});

describe("the logo read reports its chunks to the page (AC-CJ3QCD.2)", () => {
  type ReadWithProgress = (
    deviceId: string,
    full: boolean,
    onProgress: (p: LogoProgress) => void,
  ) => Promise<LogoRead>;

  it("cj3qcd: logoRead passes the read's chunk progress on, ending at the total", async () => {
    const { backend, id } = await usbBackend();
    await settle(backend.meterSettings());
    await settle(
      backend.logoUploadBuiltin(id, "surf-wordmark", () => {}),
      30_000,
    );
    const seen: LogoProgress[] = [];
    const read = (backend.logoRead as unknown as ReadWithProgress).bind(backend);
    const result = await settle(
      read(id, true, (p) => seen.push({ ...p })),
      60_000,
    );
    expect(result.status).toBe("logo");
    expect(seen.length).toBeGreaterThan(1);
    const last = seen[seen.length - 1];
    expect(last?.step).toBe("chunks");
    expect(last?.total).toBeGreaterThan(2);
    expect(last?.done).toBe(last?.total);
    await settle(backend.disconnect());
  });
});
