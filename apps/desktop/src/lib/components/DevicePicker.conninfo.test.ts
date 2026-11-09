/**
 * FR 60QB2S: an (i) beside the web device menu explains the connections.
 *
 * - AC-60QB2S.1: in the web build an (i) button sits right after the device menu, is
 *   keyboard-reachable, has an accessible name and follows the Show (i) icons switch.
 * - AC-60QB2S.2: its popover behaves like the glossary popovers: it reuses the shared
 *   GlossaryPopover and the one-at-a-time popover store (checked on the source here; the
 *   Escape / outside click / focus behaviour runs in e2e/pages/connection-info.spec.ts).
 * - AC-60QB2S.4: the popover links to the desktop app download page.
 * - AC-60QB2S.5: the popover shows the current session's connection line.
 * - AC-60QB2S.6: the desktop and mock builds show no such button.
 *
 * Rendered with `svelte/server` under the same mocked backend as DevicePicker.web.test.ts.
 */

import { existsSync, readFileSync } from "node:fs";
import type { Component } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { DeviceListing } from "../backend";

const { createWebBackend } = vi.hoisted(() => ({
  createWebBackend: vi.fn(async () => ({
    features: null,
    listDevices: vi.fn(async () => []),
    watchDevices: vi.fn(async () => () => {}),
    requestMeter: vi.fn(async () => null),
    requestHidMeter: vi.fn(async () => null),
    cancelWait: vi.fn(async () => {}),
  })),
}));

vi.mock("@tauri-apps/api/core", () => {
  class Channel {
    onmessage: (msg: unknown) => void = () => {};
  }
  return { invoke: vi.fn(async () => []), Channel, isTauri: () => false };
});
vi.mock("../web/webBackend", () => ({ createWebBackend }));

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

const KEY = "section.connection";

beforeEach(() => {
  vi.resetModules();
});

afterEach(() => {
  vi.unstubAllEnvs();
  vi.unstubAllGlobals();
});

/** The picker's HTML in the build `backend` names (`undefined`: the Tauri build), with
 * the Show (i) icons switch at `showInfoIcons` (`undefined`: no app context). */
async function pickerHtml(
  backend: "web" | "mock" | undefined,
  showInfoIcons?: boolean,
): Promise<string> {
  if (backend) vi.stubEnv("VITE_BACKEND", backend);
  const { Session } = await import("../session.svelte");
  const { default: DevicePicker } = await import("./DevicePicker.svelte");
  const { render } = await import("svelte/server");
  const session = new Session();
  session.setDevices([DEMO]);
  const props = { session, hideSerial: false };
  const context = new Map<string, unknown>();
  if (showInfoIcons !== undefined) context.set("showInfoIcons", () => showInfoIcons);
  return render(DevicePicker as Component<Record<string, unknown>>, {
    props,
    context,
  }).body.replace(/<!--.*?-->/g, "");
}

/** The connection (i) button's opening tag, or undefined when none is rendered. */
function infoButton(html: string): string | undefined {
  return new RegExp(`<button\\b[^>]*data-glossary-key="${KEY}"[^>]*>`).exec(html)?.[0];
}

const componentPath = new URL("./ConnectionInfo.svelte", import.meta.url);
const componentSource = (): string =>
  existsSync(componentPath) ? readFileSync(componentPath, "utf8") : "";
const pickerSource = (): string =>
  readFileSync(new URL("./DevicePicker.svelte", import.meta.url), "utf8");

describe("the (i) button beside the web device menu (AC-60QB2S.1)", () => {
  it("the web build renders an (i) button right after the device select", async () => {
    const html = await pickerHtml("web");
    const button = infoButton(html);
    expect(button, "the connection (i) button").toBeDefined();
    const selectEnd = html.indexOf("</select>");
    const at = html.indexOf(button ?? "<no button>");
    expect(selectEnd).toBeGreaterThan(0);
    expect(at, "after the select").toBeGreaterThan(selectEnd);
    // Right after it: no other button (such as the connect hint) in between.
    expect(html.slice(selectEnd, at)).not.toMatch(/<button\b/);
  });

  it("has an accessible name and is a plain, keyboard-reachable button", async () => {
    const button = infoButton(await pickerHtml("web")) ?? "";
    expect(button).toMatch(/aria-label="About connections"/i);
    expect(button).toMatch(/type="button"/);
    expect(button).toMatch(/class="[^"]*\binfo-button\b[^"]*"/);
    expect(button).not.toMatch(/tabindex="-1"/);
    expect(button).not.toMatch(/\bdisabled\b/);
    expect(button).toMatch(/aria-expanded="false"/);
    expect(button).toMatch(/aria-controls="[^"]+"/);
  });

  it("follows the Show (i) icons switch", async () => {
    expect(infoButton(await pickerHtml("web", true))).toBeDefined();
    vi.resetModules();
    expect(infoButton(await pickerHtml("web", false))).toBeUndefined();
  });

  it("the glossary has the section.connection entry the button is named from", async () => {
    const { GLOSSARY } = (await import("../glossary")) as unknown as {
      GLOSSARY: Record<string, { term: string; short: string }>;
    };
    const entry = GLOSSARY[KEY];
    expect(entry, KEY).toBeDefined();
    expect(entry?.term).toMatch(/^connections$/i);
    expect(entry?.short).toMatch(/WebUSB|HID|Web Serial|connect/i);
  });
});

describe("the popover behaves like the glossary popovers (AC-60QB2S.2)", () => {
  it("ConnectionInfo reuses GlossaryPopover and the shared popover store", () => {
    const src = componentSource();
    expect(src, "ConnectionInfo.svelte exists").not.toBe("");
    expect(src).toMatch(/import\s+GlossaryPopover\s+from\s+["']\.\/GlossaryPopover\.svelte["']/);
    expect(src).toMatch(/\bglossaryTrigger\b|\binfoPopover\b/);
    expect(src).toMatch(/<GlossaryPopover\b[^>]*key="section\.connection"/);
  });

  it("the popover is a closed dialog controlled by the button", async () => {
    const html = await pickerHtml("web");
    const id = /aria-controls="([^"]+)"/.exec(infoButton(html) ?? "")?.[1];
    expect(id, "aria-controls").toBeDefined();
    const popover = new RegExp(`<[a-z]+\\b[^>]*id="${id}"[^>]*>`).exec(html)?.[0] ?? "";
    expect(popover).toMatch(/role="dialog"/);
    expect(popover).toMatch(/\bhidden\b/);
  });
});

describe("the popover's extra text (AC-60QB2S.4, AC-60QB2S.5)", () => {
  it("ConnectionInfo links to the desktop app download page", () => {
    const src = componentSource();
    expect(src).toMatch(
      /import\s*\{[^}]*\bRELEASES_URL\b[^}]*\}\s*from\s*["']\.\.\/web\/features["']/,
    );
    expect(src).toMatch(/<a\b[^>]*href=\{RELEASES_URL\}/);
  });

  it("ConnectionInfo shows the connection lines and the session's line", () => {
    const src = componentSource();
    expect(src).toMatch(/\bCONNECTION_INFO_LINES\b/);
    expect(src).toMatch(/\bcurrentConnectionLine\(/);
    expect(src).toMatch(/from\s+["']\.\.\/web\/connectionInfo["']/);
  });
});

describe("no connection (i) outside the web build (AC-60QB2S.6)", () => {
  it("the Tauri build renders no such button", async () => {
    const html = await pickerHtml(undefined);
    expect(html).toMatch(/<select[^>]*aria-label="Device"/);
    expect(infoButton(html)).toBeUndefined();
    expect(html).not.toMatch(/About connections/i);
  });

  it("the mock build renders no such button", async () => {
    const html = await pickerHtml("mock");
    expect(html).toMatch(/<select[^>]*aria-label="Device"/);
    expect(infoButton(html)).toBeUndefined();
    expect(html).not.toMatch(/About connections/i);
  });

  it("DevicePicker gates ConnectionInfo on the web features", () => {
    const src = pickerSource();
    expect(src).toMatch(/import\s+ConnectionInfo\s+from\s+["']\.\/ConnectionInfo\.svelte["']/);
  });
});
