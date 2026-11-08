/**
 * FR QFTCX0, AC-QFTCX0.2 (the page side): in the web build the device picker offers a
 * "Connect a meter…" action. Its click handler asks the browser for a meter at once
 * (`requestMeter()` before any await, so the click still counts as the user gesture the
 * WebUSB picker needs), and the meter the browser grants joins the device list. The
 * desktop and mock builds do not show the action.
 *
 * The picker renders with `svelte/server` (no DOM); the click logic lives in
 * `pickMeter`, which the component's handler calls directly.
 */

import { readFileSync } from "node:fs";
import type { Component } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { DeviceListing } from "../backend";

const { createWebBackend } = vi.hoisted(() => ({
  createWebBackend: vi.fn(async () => ({
    features: null,
    listDevices: vi.fn(async () => []),
    watchDevices: vi.fn(async () => () => {}),
    requestMeter: vi.fn(async () => null),
  })),
}));

vi.mock("@tauri-apps/api/core", () => {
  class Channel {
    onmessage: (msg: unknown) => void = () => {};
  }
  return { invoke: vi.fn(async () => []), Channel, isTauri: () => false };
});
vi.mock("../web/webBackend", () => ({ createWebBackend }));

const ACTION = "Connect a meter…";

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
});

/** The picker's HTML in the build `backend` names (`undefined`: the Tauri build). */
async function pickerHtml(backend: "web" | "mock" | undefined): Promise<string> {
  if (backend) vi.stubEnv("VITE_BACKEND", backend);
  const { Session } = await import("../session.svelte");
  const { default: DevicePicker } = await import("./DevicePicker.svelte");
  // Loaded after the module reset, so it shares the component's Svelte runtime.
  const { render } = await import("svelte/server");
  const session = new Session();
  session.setDevices([DEMO]);
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
