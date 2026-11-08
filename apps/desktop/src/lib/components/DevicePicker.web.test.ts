/**
 * FR QFTCX0, AC-QFTCX0.2 (the page side): in the web build the device picker offers a
 * "Connect a meter…" action. Its click handler asks the browser for a meter at once
 * (`requestMeter()` before any await, so the click still counts as the user gesture the
 * WebUSB picker needs), and the meter the browser grants joins the device list. The
 * desktop and mock builds do not show the action.
 *
 * The picker renders with `svelte/server` (no DOM); the click logic lives in
 * `pickMeter`, which the component's handler calls directly.
 *
 * FR QFTCWZ, AC-QFTCWZ.3: when the connect failed with `device_busy` and the browser has
 * WebHID (`navigator.hid`), the web build's picker also shows "Connect over HID…"; its
 * click calls `pickMeter(session, requestHidMeter)`, and `requestHidMeter()` reaches the
 * web backend's `requestHidMeter()` (navigator.hid.requestDevice) before any await.
 */

import { readFileSync } from "node:fs";
import type { Component } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { DeviceListing } from "../backend";

const { createWebBackend, requestHidMeterMock } = vi.hoisted(() => {
  const requestHidMeterMock = vi.fn(async () => null);
  return {
    requestHidMeterMock,
    createWebBackend: vi.fn(async () => ({
      features: null,
      listDevices: vi.fn(async () => []),
      watchDevices: vi.fn(async () => () => {}),
      requestMeter: vi.fn(async () => null),
      requestHidMeter: requestHidMeterMock,
    })),
  };
});

vi.mock("@tauri-apps/api/core", () => {
  class Channel {
    onmessage: (msg: unknown) => void = () => {};
  }
  return { invoke: vi.fn(async () => []), Channel, isTauri: () => false };
});
vi.mock("../web/webBackend", () => ({ createWebBackend }));

const ACTION = "Connect a meter…";
const HID_ACTION = "Connect over HID…";

const DEMO: DeviceListing = {
  id: "demo:load-5v-1a",
  label: "Demo device — load-5v-1a",
  model: "KM003C",
  serial: "123456",
  bus: null,
  address: null,
  demo_scenario: "load-5v-1a",
  mode: "application",
};

const PICKED: DeviceListing = {
  id: "webusb:1",
  label: "KM003C — WEB-0001",
  model: "KM003C",
  serial: "WEB-0001",
  bus: null,
  address: null,
  demo_scenario: null,
  mode: "application",
};

beforeEach(() => {
  vi.resetModules();
});

afterEach(() => {
  vi.unstubAllEnvs();
  vi.unstubAllGlobals();
});

const BUSY = {
  kind: "device_busy",
  message: "Another app or driver holds the meter's USB interface. It can connect over HID.",
  hint: null,
} as const;

/** The picker's HTML in the build `backend` names (`undefined`: the Tauri build), with
 * `error` reported on the session first. */
async function pickerHtml(
  backend: "web" | "mock" | undefined,
  error: typeof BUSY | null = null,
): Promise<string> {
  if (backend) vi.stubEnv("VITE_BACKEND", backend);
  const { Session } = await import("../session.svelte");
  const { default: DevicePicker } = await import("./DevicePicker.svelte");
  // Loaded after the module reset, so it shares the component's Svelte runtime.
  const { render } = await import("svelte/server");
  const session = new Session();
  session.setDevices([DEMO]);
  if (error) session.reportError(error);
  const props = { session, hideSerial: false };
  return render(DevicePicker as Component<Record<string, unknown>>, { props }).body.replace(
    /<!--.*?-->/g,
    "",
  );
}

describe("the Connect a meter action (AC-QFTCX0.2)", () => {
  it("the web build's picker shows a Connect a meter… button", async () => {
    const body = await pickerHtml("web");
    expect(body).toMatch(new RegExp(`<button[^>]*>\\s*${ACTION}\\s*</button>`));
  });

  it("the Tauri build's picker does not show it", async () => {
    expect(await pickerHtml(undefined)).not.toContain(ACTION);
  });

  it("the mock build's picker does not show it", async () => {
    expect(await pickerHtml("mock")).not.toContain(ACTION);
  });

  it("pickMeter asks the browser synchronously, before any await", async () => {
    const { Session } = await import("../session.svelte");
    const { pickMeter } = await import("../pickMeter");
    const session = new Session();
    session.setDevices([DEMO]);
    const request = vi.fn(async () => PICKED);
    const done = pickMeter(session, request);
    // Called within the same tick as the click: the user gesture still holds.
    expect(request).toHaveBeenCalledTimes(1);
    await done;
  });

  it("the granted meter joins the device list next to the demo", async () => {
    const { Session } = await import("../session.svelte");
    const { pickMeter } = await import("../pickMeter");
    const session = new Session();
    session.setDevices([DEMO]);
    await pickMeter(session, async () => PICKED);
    expect(session.devices.map((d) => d.id).sort()).toEqual([DEMO.id, PICKED.id].sort());
    // Picking it again does not list it twice.
    await pickMeter(session, async () => PICKED);
    expect(session.devices.filter((d) => d.id === PICKED.id)).toHaveLength(1);
  });

  it("a cancelled browser picker leaves the list as it was", async () => {
    const { Session } = await import("../session.svelte");
    const { pickMeter } = await import("../pickMeter");
    const session = new Session();
    session.setDevices([DEMO]);
    await pickMeter(session, async () => null);
    expect(session.devices.map((d) => d.id)).toEqual([DEMO.id]);
    expect(session.error).toBeNull();
  });

  it("a failed request is reported, not thrown", async () => {
    const { Session } = await import("../session.svelte");
    const { pickMeter } = await import("../pickMeter");
    const session = new Session();
    session.setDevices([DEMO]);
    const failure = { kind: "permission_denied", message: "No access to the meter.", hint: null };
    await expect(pickMeter(session, () => Promise.reject(failure))).resolves.toBeUndefined();
    expect(session.error?.kind).toBe("permission_denied");
  });

  it("DevicePicker's click handler calls pickMeter with requestMeter and awaits nothing first", () => {
    const src = readFileSync(new URL("./DevicePicker.svelte", import.meta.url), "utf8");
    expect(src).toMatch(/import\s*\{[^}]*\brequestMeter\b[^}]*\}\s*from\s*["']\.\.\/backend["']/);
    expect(src).toMatch(/import\s*\{[^}]*\bpickMeter\b[^}]*\}\s*from\s*["']\.\.\/pickMeter["']/);
    const handler = /onclick=\{([^}]*\bpickMeter\b[^}]*)\}/.exec(src);
    expect(handler, "a button whose onclick calls pickMeter").not.toBeNull();
    const body = handler?.[1] ?? "";
    expect(body).toMatch(/pickMeter\(\s*session\s*,\s*requestMeter\s*\)/);
    expect(body).not.toMatch(/\bawait\b|\basync\b|\.then\(/);
  });
});

describe("the Connect over HID action (AC-QFTCWZ.3)", () => {
  const button = new RegExp(`<button[^>]*>\\s*${HID_ACTION}\\s*</button>`);

  it("shows after a device_busy connect when the browser has WebHID", async () => {
    vi.stubGlobal("navigator", { usb: {}, hid: {} });
    expect(await pickerHtml("web", BUSY)).toMatch(button);
  });

  it("is not shown without a device_busy error", async () => {
    vi.stubGlobal("navigator", { usb: {}, hid: {} });
    expect(await pickerHtml("web")).not.toContain(HID_ACTION);
  });

  it("is not shown when the browser has no WebHID", async () => {
    vi.stubGlobal("navigator", { usb: {} });
    expect(await pickerHtml("web", BUSY)).not.toContain(HID_ACTION);
  });

  it("is not shown in the Tauri or mock builds", async () => {
    vi.stubGlobal("navigator", { usb: {}, hid: {} });
    expect(await pickerHtml(undefined, BUSY)).not.toContain(HID_ACTION);
    vi.resetModules();
    expect(await pickerHtml("mock", BUSY)).not.toContain(HID_ACTION);
  });

  it("its click handler calls pickMeter with requestHidMeter and awaits nothing first", () => {
    const src = readFileSync(new URL("./DevicePicker.svelte", import.meta.url), "utf8");
    expect(src).toMatch(
      /import\s*\{[^}]*\brequestHidMeter\b[^}]*\}\s*from\s*["']\.\.\/backend["']/,
    );
    const handler = /onclick=\{([^}]*\brequestHidMeter\b[^}]*)\}/.exec(src);
    expect(handler, "a button whose onclick calls pickMeter with requestHidMeter").not.toBeNull();
    const body = handler?.[1] ?? "";
    expect(body).toMatch(/pickMeter\(\s*session\s*,\s*requestHidMeter\s*\)/);
    expect(body).not.toMatch(/\bawait\b|\basync\b|\.then\(/);
  });

  it("requestHidMeter reaches the loaded web backend synchronously", async () => {
    vi.stubEnv("VITE_BACKEND", "web");
    requestHidMeterMock.mockClear();
    const backend = (await import("../backend")) as unknown as {
      requestHidMeter: () => Promise<DeviceListing | null>;
    };
    // Let the preloaded web backend finish loading.
    for (let i = 0; i < 20; i++) await Promise.resolve();
    await new Promise((r) => setTimeout(r, 0));
    const pending = backend.requestHidMeter();
    expect(requestHidMeterMock).toHaveBeenCalledTimes(1);
    await expect(pending).resolves.toBeNull();
  });

  it("requestHidMeter resolves null in the Tauri build", async () => {
    const backend = (await import("../backend")) as unknown as {
      requestHidMeter: () => Promise<DeviceListing | null>;
    };
    expect(typeof backend.requestHidMeter).toBe("function");
    await expect(backend.requestHidMeter()).resolves.toBeNull();
  });
});
