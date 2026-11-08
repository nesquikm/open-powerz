/**
 * FR QFTCX0, AC-QFTCX0.2 (the page side) and FR QFTCWZ, AC-QFTCWZ.3: `pickMeter` asks the
 * browser for a meter at once (`request()` before any await, so the choice still counts
 * as the user gesture the WebUSB / WebHID picker needs), and the granted meter joins the
 * device list.
 *
 * FR AYR4W6: in the web build those actions are items of the device menu itself, not
 * buttons beside it.
 * - AC-AYR4W6.1: the menu's last item is "Connect a meter…"; choosing it calls the web
 *   backend's requestMeter() synchronously in the change event, and the select goes
 *   back to the picked device.
 * - AC-AYR4W6.2: no Connect button beside the menu.
 * - AC-AYR4W6.3: with the HID fallback offered (device_busy + navigator.hid), "Connect
 *   over HID…" is the item next to it and calls requestHidMeter() the same way.
 * - AC-AYR4W6.4: with only the demo meter listed, a hint next to the menu names
 *   "Connect a meter…" and its click opens the same picker; gone once a meter is listed.
 * - AC-AYR4W6.5: the Tauri and mock builds' menus carry no such item.
 * - AC-AYR4W6.6: these tests replace the ones that checked the old buttons.
 *
 * The picker renders with `svelte/server` (no DOM). Choosing a menu entry goes through
 * `chooseDeviceMenu` in `src/lib/deviceMenu.ts`, which the component's change handler
 * calls directly; `deviceMenuActions` builds the action items.
 */

import { readFileSync } from "node:fs";
import type { Component } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { DeviceListing } from "../backend";
import type { Session } from "../session.svelte";

const { createWebBackend, requestHidMeterMock, requestMeterMock } = vi.hoisted(() => {
  const requestHidMeterMock = vi.fn(async () => null);
  const requestMeterMock = vi.fn(async () => null);
  return {
    requestHidMeterMock,
    requestMeterMock,
    createWebBackend: vi.fn(async () => ({
      features: null,
      listDevices: vi.fn(async () => []),
      watchDevices: vi.fn(async () => () => {}),
      requestMeter: requestMeterMock,
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
 * `error` reported on the session first and `devices` listed. */
async function pickerHtml(
  backend: "web" | "mock" | undefined,
  error: typeof BUSY | null = null,
  devices: readonly DeviceListing[] = [DEMO],
): Promise<string> {
  if (backend) vi.stubEnv("VITE_BACKEND", backend);
  const { Session } = await import("../session.svelte");
  const { default: DevicePicker } = await import("./DevicePicker.svelte");
  // Loaded after the module reset, so it shares the component's Svelte runtime.
  const { render } = await import("svelte/server");
  const session = new Session();
  session.setDevices(devices);
  if (error) session.reportError(error);
  const props = { session, hideSerial: false };
  return render(DevicePicker as Component<Record<string, unknown>>, { props }).body.replace(
    /<!--.*?-->/g,
    "",
  );
}

describe("pickMeter (AC-QFTCX0.2)", () => {
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
});

describe("requestHidMeter (AC-QFTCWZ.3)", () => {
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

// --- FR AYR4W6: the connect actions live in the device menu --------------------------

interface MenuAction {
  value: string;
  label: string;
  run: () => unknown;
}

interface DeviceMenuModule {
  deviceMenuActions(session: Session, offerHid: boolean): MenuAction[];
  chooseDeviceMenu(
    session: Session,
    target: { value: string },
    actions: readonly MenuAction[],
  ): boolean;
}

/** The module under test; named through a variable so the suite type-checks before it
 * exists. */
const DEVICE_MENU = "../deviceMenu";
const loadDeviceMenu = async (): Promise<DeviceMenuModule> =>
  (await import(/* @vite-ignore */ DEVICE_MENU)) as DeviceMenuModule;

/** The device select's options, in order. */
function menuOptions(html: string): { value: string; text: string }[] {
  const select = /<select[^>]*aria-label="Device"[^>]*>([\s\S]*?)<\/select>/.exec(html);
  expect(select, "the device select").not.toBeNull();
  return [
    ...(select?.[1] ?? "").matchAll(/<option[^>]*value="([^"]*)"[^>]*>([^<]*)<\/option>/g),
  ].map((m) => ({ value: m[1] ?? "", text: (m[2] ?? "").trim() }));
}

/** Everything the picker renders outside its select. */
function outsideMenu(html: string): string {
  return html.replace(/<select[\s\S]*?<\/select>/, "");
}

/** The bodies of every `attr={…}` handler in Svelte source `src`, braces balanced. */
function handlers(src: string, attr: string): string[] {
  const out: string[] = [];
  let at = src.indexOf(`${attr}={`);
  while (at >= 0) {
    let depth = 0;
    let i = at + attr.length + 1;
    for (; i < src.length; i++) {
      if (src[i] === "{") depth++;
      else if (src[i] === "}" && --depth === 0) break;
    }
    out.push(src.slice(at + attr.length + 2, i));
    at = src.indexOf(`${attr}={`, i);
  }
  return out;
}

const pickerSource = () => readFileSync(new URL("./DevicePicker.svelte", import.meta.url), "utf8");

/** Loads the web build's backend and lets its preloaded web backend finish loading. */
async function loadWebBackend(): Promise<void> {
  vi.stubEnv("VITE_BACKEND", "web");
  await import("../backend");
  for (let i = 0; i < 20; i++) await Promise.resolve();
  await new Promise((r) => setTimeout(r, 0));
}

describe("Connect a meter… is the device menu's last item (AC-AYR4W6.1)", () => {
  it("the web build's menu ends with the Connect a meter… item", async () => {
    const options = menuOptions(await pickerHtml("web"));
    expect(options.map((o) => o.text)).toEqual([expect.stringMatching(/Demo/), ACTION]);
    const item = options.at(-1);
    expect(item?.value, "the item's value is not a device id").not.toBe(DEMO.id);
    expect(item?.value).not.toBe("");
  });

  it("it stays last with a paired meter listed too", async () => {
    const options = menuOptions(await pickerHtml("web", null, [DEMO, PICKED]));
    expect(options.at(-1)?.text).toBe(ACTION);
    expect(options.filter((o) => o.text === ACTION)).toHaveLength(1);
  });

  it("deviceMenuActions' connect item runs the web backend's requestMeter synchronously", async () => {
    await loadWebBackend();
    const { Session } = await import("../session.svelte");
    const { deviceMenuActions } = await loadDeviceMenu();
    const session = new Session();
    session.setDevices([DEMO]);
    const actions = deviceMenuActions(session, false);
    expect(actions.map((a) => a.label)).toEqual([ACTION]);
    requestMeterMock.mockClear();
    const done = actions[0]?.run();
    expect(requestMeterMock).toHaveBeenCalledTimes(1);
    await done;
  });

  it("choosing the item opens the picker within the change event and the select goes back", async () => {
    await loadWebBackend();
    const { Session } = await import("../session.svelte");
    const { chooseDeviceMenu, deviceMenuActions } = await loadDeviceMenu();
    const session = new Session();
    session.setDevices([DEMO]);
    const actions = deviceMenuActions(session, false);
    const connect = actions.find((a) => a.label === ACTION);
    expect(connect).toBeDefined();
    const target = { value: connect?.value ?? "" };
    requestMeterMock.mockClear();
    const cancel = chooseDeviceMenu(session, target, actions);
    // Same tick as the user's choice: the gesture still holds for the browser picker.
    expect(requestMeterMock).toHaveBeenCalledTimes(1);
    expect(cancel).toBe(false);
    // The action item never stays selected; the picked device does.
    expect(target.value).toBe(DEMO.id);
    expect(session.selectedId).toBe(DEMO.id);
  });

  it("choosing a device still just picks it", async () => {
    await loadWebBackend();
    const { Session } = await import("../session.svelte");
    const { chooseDeviceMenu, deviceMenuActions } = await loadDeviceMenu();
    const session = new Session();
    session.setDevices([DEMO, PICKED]);
    const actions = deviceMenuActions(session, true);
    requestMeterMock.mockClear();
    requestHidMeterMock.mockClear();
    const target = { value: PICKED.id };
    expect(chooseDeviceMenu(session, target, actions)).toBe(false);
    expect(session.selectedId).toBe(PICKED.id);
    expect(target.value).toBe(PICKED.id);
    expect(requestMeterMock).not.toHaveBeenCalled();
    expect(requestHidMeterMock).not.toHaveBeenCalled();
  });

  it("DevicePicker's change handler calls chooseDeviceMenu and awaits nothing first", () => {
    const src = pickerSource();
    expect(src).toMatch(
      /import\s*\{[^}]*\bchooseDeviceMenu\b[^}]*\}\s*from\s*["']\.\.\/deviceMenu["']/,
    );
    const change = handlers(src, "onchange").find((h) => /\bchooseDeviceMenu\(/.test(h));
    expect(change, "an onchange handler that calls chooseDeviceMenu").toBeDefined();
    const before = (change ?? "").slice(0, (change ?? "").indexOf("chooseDeviceMenu("));
    expect(before).not.toMatch(/\bawait\b|\.then\(/);
  });
});

describe("no separate connect button (AC-AYR4W6.2)", () => {
  it("the web build shows no Connect a meter… button beside the menu", async () => {
    const html = await pickerHtml("web");
    expect(html).not.toMatch(new RegExp(`<button[^>]*>\\s*${ACTION}\\s*</button>`));
  });

  it("nor a Connect over HID… button when HID is offered", async () => {
    vi.stubGlobal("navigator", { usb: {}, hid: {} });
    const html = await pickerHtml("web", BUSY);
    expect(html).not.toMatch(new RegExp(`<button[^>]*>\\s*${HID_ACTION}\\s*</button>`));
    expect(outsideMenu(html)).not.toContain(HID_ACTION);
  });

  it("with a meter listed, nothing beside the menu names the connect action", async () => {
    expect(outsideMenu(await pickerHtml("web", null, [DEMO, PICKED]))).not.toContain(ACTION);
  });
});

describe("the HID fallback is a menu item (AC-AYR4W6.3)", () => {
  it("after a device_busy connect with WebHID, it sits next to Connect a meter…", async () => {
    vi.stubGlobal("navigator", { usb: {}, hid: {} });
    const texts = menuOptions(await pickerHtml("web", BUSY)).map((o) => o.text);
    const connect = texts.indexOf(ACTION);
    const hid = texts.indexOf(HID_ACTION);
    expect(connect).toBeGreaterThan(0);
    expect(hid).toBeGreaterThan(0);
    expect(Math.abs(connect - hid)).toBe(1);
    expect(texts.slice(-2).sort()).toEqual([ACTION, HID_ACTION].sort());
  });

  it("is not in the menu without a device_busy error", async () => {
    vi.stubGlobal("navigator", { usb: {}, hid: {} });
    expect(menuOptions(await pickerHtml("web")).map((o) => o.text)).not.toContain(HID_ACTION);
  });

  it("is not in the menu when the browser has no WebHID", async () => {
    vi.stubGlobal("navigator", { usb: {} });
    expect(menuOptions(await pickerHtml("web", BUSY)).map((o) => o.text)).not.toContain(HID_ACTION);
  });

  it("choosing it runs the web backend's requestHidMeter within the change event", async () => {
    await loadWebBackend();
    const { Session } = await import("../session.svelte");
    const { chooseDeviceMenu, deviceMenuActions } = await loadDeviceMenu();
    const session = new Session();
    session.setDevices([DEMO]);
    const actions = deviceMenuActions(session, true);
    expect(actions.map((a) => a.label).sort()).toEqual([ACTION, HID_ACTION].sort());
    const hid = actions.find((a) => a.label === HID_ACTION);
    const target = { value: hid?.value ?? "" };
    requestHidMeterMock.mockClear();
    requestMeterMock.mockClear();
    chooseDeviceMenu(session, target, actions);
    expect(requestHidMeterMock).toHaveBeenCalledTimes(1);
    expect(requestMeterMock).not.toHaveBeenCalled();
    expect(target.value).toBe(DEMO.id);
    expect(session.selectedId).toBe(DEMO.id);
  });
});

describe("a hint points to the menu item while only the demo is listed (AC-AYR4W6.4)", () => {
  it("names Connect a meter… next to the menu", async () => {
    const outside = outsideMenu(await pickerHtml("web"));
    expect(outside).toContain(ACTION);
    // A clickable link or button, not plain text.
    expect(outside).toMatch(new RegExp(`<(a|button)\\b[^>]*>[^<]*${ACTION}`));
  });

  it("is gone once a meter is listed", async () => {
    expect(outsideMenu(await pickerHtml("web", null, [DEMO, PICKED]))).not.toContain(ACTION);
  });

  it("is not shown in the Tauri or mock builds", async () => {
    expect(outsideMenu(await pickerHtml(undefined))).not.toContain(ACTION);
    vi.resetModules();
    expect(outsideMenu(await pickerHtml("mock"))).not.toContain(ACTION);
  });

  it("its click opens the same picker, with nothing awaited first", () => {
    const clicks = handlers(pickerSource(), "onclick");
    const hint = clicks.find((h) =>
      /pickMeter\(\s*session\s*,\s*requestMeter\s*\)|\.run\(/.test(h),
    );
    expect(hint, "an onclick that opens the WebUSB picker").toBeDefined();
    expect(hint).not.toMatch(/\bawait\b|\basync\b|\.then\(/);
  });

  it("the web backend's HID hint tells the user to use the device menu", () => {
    const src = readFileSync(new URL("../web/webBackend.ts", import.meta.url), "utf8");
    const hint = /hint:\s*"([^"]*Connect over HID[^"]*)"/.exec(src);
    expect(hint, "the device_busy hint naming Connect over HID…").not.toBeNull();
    expect(hint?.[1]).toMatch(/device menu/i);
    expect(hint?.[1]).not.toMatch(/^Click\b/);
  });
});

describe("the desktop builds' menu has no connect items (AC-AYR4W6.5)", () => {
  for (const build of [undefined, "mock"] as const) {
    it(`the ${build ?? "Tauri"} build lists only devices, even after device_busy`, async () => {
      vi.stubGlobal("navigator", { usb: {}, hid: {} });
      const texts = menuOptions(await pickerHtml(build, BUSY)).map((o) => o.text);
      expect(texts).not.toContain(ACTION);
      expect(texts).not.toContain(HID_ACTION);
      expect(texts).toHaveLength(1);
    });
  }
});
