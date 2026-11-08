/**
 * FR QFTCX0, AC-QFTCX0.1: `VITE_BACKEND=web` sends the UI's backend calls to the web
 * backend; the Tauri and mock paths are unchanged, and those builds carry no WebUSB
 * code (the web backend is reached only through a guarded dynamic import).
 */

import { readFileSync } from "node:fs";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { invoke, createWebBackend, fakeWeb, revealItemInDir } = vi.hoisted(() => {
  /** Any command the dispatch asks the fake for exists and resolves to `null`. */
  const lenient = <T extends object>(target: T): T =>
    new Proxy(target, {
      get(t, p) {
        if (p === "then" || typeof p === "symbol") return undefined;
        const rec = t as Record<string, unknown>;
        if (!(p in rec)) rec[p] = vi.fn(async () => null);
        return rec[p];
      },
    });
  const fakeMeter = lenient({
    stats: vi.fn(async () => ({ samples: 7 })),
    setRate: vi.fn(async () => {}),
  });
  const fakeWeb = {
    features: null as unknown,
    listDevices: vi.fn(async () => [{ id: "demo:load-5v-1a", demo_scenario: "load-5v-1a" }]),
    watchDevices: vi.fn(async () => () => {}),
    requestMeter: vi.fn(async () => ({ id: "usb:1", serial: "123456" })),
    connect: vi.fn(async () => {}),
    disconnect: vi.fn(async () => {}),
    setRate: vi.fn(async () => {}),
    stats: vi.fn(async () => ({ samples: 3 })),
    meter: vi.fn(() => fakeMeter),
  };
  const webProxy = lenient(fakeWeb);
  return {
    invoke: vi.fn(async () => []),
    createWebBackend: vi.fn(async () => webProxy),
    fakeWeb,
    fakeMeter,
    revealItemInDir: vi.fn(async () => {}),
  };
});

vi.mock("@tauri-apps/api/core", () => {
  class Channel {
    onmessage: (msg: unknown) => void = () => {};
  }
  return { invoke, Channel, isTauri: () => true };
});

vi.mock("./web/webBackend", () => ({ createWebBackend }));
vi.mock("@tauri-apps/plugin-opener", () => ({ revealItemInDir }));

beforeEach(() => {
  vi.resetModules();
  invoke.mockClear();
  createWebBackend.mockClear();
  revealItemInDir.mockClear();
  for (const fn of Object.values(fakeWeb))
    if (typeof fn === "function") (fn as { mockClear?: () => void }).mockClear?.();
});

afterEach(() => {
  vi.unstubAllEnvs();
});

describe("web dispatch (AC-QFTCX0.1)", () => {
  it("VITE_BACKEND=web routes the backend calls to the web backend", async () => {
    vi.stubEnv("VITE_BACKEND", "web");
    const { WEB_FEATURES } = await import("./web/features");
    const b = await import("./backend");

    expect(b.backendAvailable()).toBe(true);
    expect(b.features).toEqual(WEB_FEATURES);
    expect(await b.listDevices()).toEqual([{ id: "demo:load-5v-1a", demo_scenario: "load-5v-1a" }]);
    expect(await b.requestMeter()).toEqual({ id: "usb:1", serial: "123456" });
    const onEvent = () => {};
    await b.connect("usb:1", 50, onEvent);
    expect(fakeWeb.connect).toHaveBeenCalledWith("usb:1", 50, onEvent, "queue");
    await b.setRate(10);
    expect(fakeWeb.setRate).toHaveBeenCalledWith(10);
    expect(await b.stats()).toEqual({ samples: 3 });

    expect(await b.meterBackend("usb:1").stats()).toEqual({ samples: 7 });
    expect(fakeWeb.meter).toHaveBeenCalledWith("usb:1");

    expect(createWebBackend).toHaveBeenCalledTimes(1);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("the Tauri path is unchanged and never loads the web backend", async () => {
    const { DESKTOP_FEATURES } = await import("./web/features");
    const b = await import("./backend");
    await b.listDevices();
    expect(invoke).toHaveBeenCalledWith("list_devices");
    expect(b.features).toEqual(DESKTOP_FEATURES);
    expect(createWebBackend).not.toHaveBeenCalled();
  });

  it("the mock path is unchanged and never loads the web backend", async () => {
    vi.stubEnv("VITE_BACKEND", "mock");
    const { DESKTOP_FEATURES } = await import("./web/features");
    const b = await import("./backend");
    const list = await b.listDevices();
    expect(list.length).toBeGreaterThan(0);
    expect(invoke).not.toHaveBeenCalled();
    expect(b.features).toEqual(DESKTOP_FEATURES);
    expect(createWebBackend).not.toHaveBeenCalled();
  });
});

describe("no WebUSB code in the Tauri and mock builds (AC-QFTCX0.1)", () => {
  const read = (path: string) => readFileSync(new URL(path, import.meta.url), "utf8");
  const WEB_ONLY =
    /from\s+["']\.\/web\/(webBackend|webusb|webhid|driver|demoTransport|transport)["']/;

  it("backend.ts reaches the web backend only through a guarded dynamic import", () => {
    const src = read("./backend.ts");
    expect(src).not.toMatch(WEB_ONLY);
    expect(src).toMatch(
      /import\.meta\.env\.VITE_BACKEND\s*===\s*["']web["'][\s\S]{0,200}import\(\s*["']\.\/web\/webBackend["']\s*\)/,
    );
    expect(src.match(/import\(\s*["']\.\/web\/[^"']+["']\s*\)/g)).toEqual([
      'import("./web/webBackend")',
    ]);
  });

  it("mockBackend.ts imports nothing of the web backend", () => {
    const src = read("./mockBackend.ts");
    expect(src).not.toMatch(/["']\.\/web\/(webBackend|webusb|webhid|driver|demoTransport)["']/);
  });
});

describe("the web build never calls Tauri (AC-QFTCX0.6)", () => {
  /** Arguments that let a command get past its own argument handling. */
  const noop = () => {};
  const ARGS: Record<string, unknown[]> = {
    connect: ["usb:1", 50, noop],
    attach: [noop],
    watchDevices: [noop],
    setRate: [10],
    startRecording: ["rec", {}],
    saveRecordingAs: ["/tmp/x.csv"],
    exportSession: ["session", noop],
    exportSessionDb: ["session", noop],
    openRecordingFile: ["/tmp/x.csv", noop],
    recordingFileRange: [["vbus"], 0, 1000, 10],
    historyRange: [["vbus"], 0, 1000, 10],
    historyGaps: [0, 1000],
    rangeStats: [0, 1000],
    recordingFileRangeStats: [0, 1000],
    runProgress: [{ kind: "charge", scope: "session" }],
    recordingFileRunProgress: [{ kind: "charge", scope: "session" }],
    pdDetails: [0],
    meterSettingsWrite: [{ brightness: 3 }],
    recordDownload: [0],
    recordsDelete: [0, "DELETE"],
    firmwareInspect: ["/tmp/fw.mencrypt"],
    logoPreview: ["/tmp/logo.png", false],
    logoPreviewBuiltin: ["surf"],
    logoRead: ["usb:1", false],
    logoUpload: ["usb:1", "/tmp/logo.png", false, noop],
    logoUploadBuiltin: ["usb:1", "surf", noop],
    logoRemove: ["usb:1", noop],
    logoSettingsCheck: ["usb:1"],
    firmwareFlash: ["usb:1", "/tmp/fw.mencrypt", "FLASH", noop],
    pdSave: ["capture"],
    saveChartImage: ["chart", "png", new Uint8Array([1, 2, 3])],
    revealPath: ["/tmp/x.csv"],
    triggerList: ["pd"],
    triggerRisk: [{ protocol: "qc", volts: 9 }],
    triggerRequest: [{ protocol: "qc", volts: 9 }, false, false],
    triggerMessageRisk: [{ sop: "sop", kind: "get_source_cap" }],
    triggerMessageSend: [{ sop: "sop", kind: "get_source_cap" }, false],
    triggerConsoleRisk: ["help"],
    triggerConsoleSend: ["help", false],
  };
  /** Not commands: pure helpers and the per-meter factory (checked below). */
  const NOT_COMMANDS = new Set(["toUiError", "backendAvailable", "meterBackend"]);

  /** Runs `call` and checks it neither reached Tauri nor failed with anything but the
   * desktop-only usage error. */
  async function expectWebSafe(name: string, call: () => Promise<unknown>) {
    const { toUiError } = await import("./backend");
    let outcome: "resolved" | "rejected" = "resolved";
    let error: unknown = null;
    try {
      await call();
    } catch (err) {
      outcome = "rejected";
      error = err;
    }
    expect(invoke, `${name} called invoke`).not.toHaveBeenCalled();
    expect(revealItemInDir, `${name} called revealItemInDir`).not.toHaveBeenCalled();
    if (outcome === "rejected") {
      const ui = toUiError(error);
      expect(ui.kind, `${name} rejected with ${String(error)}`).toBe("usage");
      expect(ui.message, `${name}'s usage error`).toMatch(/desktop app/i);
    }
  }

  it("every exported backend function routes to the web backend or rejects as desktop-only", async () => {
    vi.stubEnv("VITE_BACKEND", "web");
    const b = (await import("./backend")) as unknown as Record<string, unknown>;
    const names = Object.keys(b).filter((n) => typeof b[n] === "function" && !NOT_COMMANDS.has(n));
    expect(names.length).toBeGreaterThan(60);
    const offenders: string[] = [];
    for (const name of names) {
      invoke.mockClear();
      revealItemInDir.mockClear();
      const fn = b[name] as (...a: unknown[]) => Promise<unknown>;
      try {
        await expectWebSafe(name, () => Promise.resolve().then(() => fn(...(ARGS[name] ?? []))));
      } catch {
        offenders.push(name);
      }
    }
    expect(offenders).toEqual([]);
  });

  it("meterBackend's commands never call Tauri in the web build", async () => {
    vi.stubEnv("VITE_BACKEND", "web");
    const b = await import("./backend");
    const m = b.meterBackend("usb:1") as unknown as Record<string, unknown>;
    const offenders: string[] = [];
    for (const [name, value] of Object.entries(m)) {
      if (typeof value !== "function") continue;
      invoke.mockClear();
      const fn = value as (...a: unknown[]) => Promise<unknown>;
      try {
        await expectWebSafe(name, () => Promise.resolve().then(() => fn(...(ARGS[name] ?? []))));
      } catch {
        offenders.push(name);
      }
    }
    expect(offenders).toEqual([]);
  });

  it("appInfo resolves with the web edition's version, not a Tauri call", async () => {
    vi.stubEnv("VITE_BACKEND", "web");
    const pkg = JSON.parse(
      readFileSync(new URL("../../package.json", import.meta.url), "utf8"),
    ) as { version: string };
    const b = await import("./backend");
    const info = await b.appInfo();
    expect(info.appVersion).toBe(pkg.version);
    expect(typeof info.protocolVersion).toBe("string");
    expect(info.protocolVersion.length).toBeGreaterThan(0);
    expect(invoke).not.toHaveBeenCalled();
  });
});
