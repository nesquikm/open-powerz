/**
 * FR NWGY72, rendered with `svelte/server` (no DOM) under the same mocked backend as
 * DevicePicker.web.test.ts.
 *
 * - AC-NWGY72.3: while a not-connected meter is picked, the picker says so ("Not
 *   connected — plug it in") and offers a Refresh button that reads the device list again
 *   (its handler calls `listDevices`, checked on the source).
 * - AC-NWGY72.5: the startup logo confirm, the record delete confirm and the settings
 *   Apply row each name the target they write to; the Records and Startup logo views pass
 *   it through.
 */

import { readFileSync } from "node:fs";
import type { Component } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { DeviceListing, MeterSettings } from "../backend";

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

const SERIAL = "SERIAL-NWGY72";
const NOT_CONNECTED = "Not connected — plug it in";
const DEMO_TARGET = "Demo device — idle (no meter is written)";
const METER_TARGET = "KM003C — serial hidden";

function meter(id: string): DeviceListing {
  return {
    id,
    label: "POWER-Z KM003C",
    model: "KM003C",
    serial: SERIAL,
    bus: null,
    address: null,
    demo_scenario: null,
    mode: "application",
  };
}

const DEMO: DeviceListing = {
  id: "demo:idle",
  label: "Demo device — idle",
  model: "KM003C",
  serial: "123456",
  bus: null,
  address: null,
  demo_scenario: "idle",
  mode: "application",
};

beforeEach(() => {
  vi.resetModules();
});

afterEach(() => {
  vi.unstubAllEnvs();
  vi.unstubAllGlobals();
});

const strip = (html: string) => html.replace(/<!--.*?-->/g, "");

/** Renders `file` (under ./) with `props`, after the module reset so it shares the
 * component's Svelte runtime. */
async function html(file: string, props: Record<string, unknown>): Promise<string> {
  const { default: C } = await import(`./${file}.svelte`);
  const { render } = await import("svelte/server");
  return strip(render(C as Component<Record<string, unknown>>, { props }).body);
}

/** The picker with `id` picked, then gone from the list (or still listed when `vanish`
 * is false). */
async function pickerHtml(backend: "web" | "mock" | undefined, vanish: boolean): Promise<string> {
  if (backend) vi.stubEnv("VITE_BACKEND", backend);
  const { Session } = await import("../session.svelte");
  const session = new Session();
  const id = backend === "web" ? "webusb:1" : "usb:1:7";
  session.setDevices([meter(id), DEMO]);
  session.select(id);
  if (vanish) session.setDevices([DEMO]);
  return html("DevicePicker", { session, hideSerial: false });
}

describe("The picker says the picked meter is not connected (AC-NWGY72.3)", () => {
  it("shows the notice and a Refresh button on the desktop", async () => {
    const out = await pickerHtml(undefined, true);
    expect(out).toContain(NOT_CONNECTED);
    expect(out).toMatch(/<button[^>]*>\s*Refresh\s*<\/button>/);
    expect(out).toContain("— not connected");
  });

  it("shows the notice and a Refresh button in the web edition", async () => {
    const out = await pickerHtml("web", true);
    expect(out).toContain(NOT_CONNECTED);
    expect(out).toMatch(/<button[^>]*>\s*Refresh\s*<\/button>/);
  });

  it("shows neither while the picked meter is listed", async () => {
    const out = await pickerHtml(undefined, false);
    expect(out).not.toContain(NOT_CONNECTED);
    expect(out).not.toMatch(/<button[^>]*>\s*Refresh\s*<\/button>/);
  });

  it("Refresh reads the device list again", () => {
    const src = readFileSync(new URL("./DevicePicker.svelte", import.meta.url), "utf8");
    const button = src.match(/<button[^>]*?onclick=\{([^}]*)\}[^>]*>\s*Refresh\s*<\/button>/s);
    expect(button).not.toBeNull();
    const handler = button?.[1] ?? "";
    // Either inline or through a named function that calls listDevices.
    const named = handler.match(/^\s*(?:\(\)\s*=>\s*(?:void\s+)?)?(\w+)\s*(?:\(\))?\s*$/)?.[1];
    const body = named
      ? (src.match(new RegExp(`function ${named}\\b[\\s\\S]*?\\n  }`))?.[0] ?? "")
      : "";
    expect(`${handler}\n${body}`).toContain("listDevices");
  });
});

describe("The logo confirm names its target (AC-NWGY72.5)", () => {
  const base = { action: "upload", durationText: "", onCancel: () => {}, onConfirm: () => {} };

  it("names the demo device and says no meter is written", async () => {
    expect(await html("LogoConfirmDialog", { ...base, target: DEMO_TARGET })).toContain(
      DEMO_TARGET,
    );
  });

  it("names the meter with its serial hidden", async () => {
    const out = await html("LogoConfirmDialog", {
      ...base,
      action: "remove",
      target: METER_TARGET,
    });
    expect(out).toContain(METER_TARGET);
  });
});

describe("The record delete confirm names its target (AC-NWGY72.5)", () => {
  const base = { onCancel: () => {}, onConfirm: () => {} };

  it("names the demo device for one record", async () => {
    expect(
      await html("RecordDeleteDialog", { ...base, name: "rec-1", target: DEMO_TARGET }),
    ).toContain(DEMO_TARGET);
  });

  it("names the meter for every record", async () => {
    expect(
      await html("RecordDeleteDialog", { ...base, name: null, target: METER_TARGET }),
    ).toContain(METER_TARGET);
  });
});

const SETTINGS: MeterSettings = {
  fields: [
    { key: "language", label: "Language", value: "English", tag: "confirmed" },
    { key: "brightness", label: "Brightness", value: "50%", tag: "confirmed" },
    { key: "dim_timeout", label: "Dim screen after", value: "off", tag: "confirmed" },
    { key: "startup_logo", label: "Startup logo", value: "off", tag: "confirmed" },
    { key: "theme", label: "Theme", value: "blue", tag: "confirmed" },
    { key: "auto_rotate", label: "Auto-rotate", value: "on", tag: "confirmed" },
    { key: "rotation", label: "Rotation", value: "90°", tag: "inferred" },
    { key: "sample_rate", label: "Sample rate", value: "50 SPS", tag: "confirmed" },
  ],
};

describe("The settings Apply row names its target (AC-NWGY72.5)", () => {
  async function editingHtml(target: string): Promise<string> {
    const { MeterSettingsPanel } = await import("../meterSettings.svelte");
    const panel = new MeterSettingsPanel(async () => SETTINGS);
    await panel.connected();
    panel.startEdit();
    expect(panel.editing).toBe(true);
    return html("MeterSettingsView", { panel, connected: true, target });
  }

  it("names the demo device next to Apply", async () => {
    const out = await editingHtml(DEMO_TARGET);
    expect(out).toContain(DEMO_TARGET);
    // In the Apply row: after the form's fields, before the Apply button closes the form.
    const form = out.slice(out.indexOf("<form"), out.indexOf("</form>"));
    expect(form).toContain(DEMO_TARGET);
  });

  it("names the meter with its serial hidden next to Apply", async () => {
    const out = await editingHtml(METER_TARGET);
    const form = out.slice(out.indexOf("<form"), out.indexOf("</form>"));
    expect(form).toContain(METER_TARGET);
  });
});

describe("The views pass the target through (AC-NWGY72.5)", () => {
  const source = (file: string) =>
    readFileSync(new URL(`./${file}.svelte`, import.meta.url), "utf8");

  it("the Records view hands its target to the delete confirm", () => {
    const src = source("RecordsView");
    expect(src).toMatch(/\btarget\b[^;]*\}\s*=\s*\$props\(\)|let \{[^}]*\btarget\b[^}]*\}/s);
    expect(src).toMatch(/<RecordDeleteDialog[^>]*\{target\}|<RecordDeleteDialog[^>]*target=\{/s);
  });

  it("the Startup logo view hands its target to the logo confirm", () => {
    const src = source("StartupLogoView");
    expect(src).toMatch(/let \{[^}]*\btarget\b[^}]*\}/s);
    expect(src).toMatch(/<LogoConfirmDialog[^>]*\{target\}|<LogoConfirmDialog[^>]*target=\{/s);
  });

  it("App gives the three views a target from writeTargetName", () => {
    const src = readFileSync(new URL("../../App.svelte", import.meta.url), "utf8");
    expect(src).toContain("writeTargetName");
    for (const view of ["RecordsView", "StartupLogoView", "MeterSettingsView"]) {
      expect(src).toMatch(new RegExp(`<${view}[^>]*target=\\{|<${view}[^>]*\\{target\\}`, "s"));
    }
  });
});
