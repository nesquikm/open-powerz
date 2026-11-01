/**
 * AC-AXHK2M.6: every view shows an error's hint, its quoted commands as code and no raw
 * backticks. AC-AXHK2M.7: the firmware notices and the firmware view's status region.
 * The components render on the server side here, so a test reads the HTML they produce;
 * everything is on fake backends and nothing reaches a meter.
 */

import type { Component } from "svelte";
import { render } from "svelte/server";
import { describe, expect, it, vi } from "vitest";
import type { UiError } from "../backend";
import { PdConsole, type ConsoleApi } from "../console.svelte";
import { FirmwareUpdate, type FirmwareUpdateApi } from "../firmwareUpdate.svelte";
import { MeterSettingsPanel } from "../meterSettings.svelte";
import { RecordsPanel } from "../records.svelte";
import { Session } from "../session.svelte";
import { StatsControls } from "../stats.svelte";
import { TriggerPanel, type TriggerApi } from "../trigger.svelte";
import ErrorBanner from "./ErrorBanner.svelte";
import FirmwareNotice from "./FirmwareNotice.svelte";
import FirmwareView from "./FirmwareView.svelte";
import MeterSettingsView from "./MeterSettingsView.svelte";
import PdConsoleView from "./PdConsoleView.svelte";
import RecordsView from "./RecordsView.svelte";
import StatsPanel from "./StatsPanel.svelte";
import TriggerView from "./TriggerView.svelte";

const HINTED: UiError = {
  kind: "usage",
  message: "It went wrong.",
  hint: "Run `sudo fix-it` first.",
};

/** The HTML of `component` with `props`, without Svelte's comment markers. */
function html(component: unknown, props: Record<string, unknown>): string {
  const body = render(component as Component<Record<string, unknown>>, { props }).body;
  return body.replace(/<!--.*?-->/g, "");
}

/** The text of `markup`, tags dropped and runs of blanks closed up. */
function text(markup: string): string {
  return markup
    .replace(/<[^>]*>/g, "")
    .replace(/\s+/g, " ")
    .trim();
}

function expectHint(markup: string): void {
  expect(text(markup)).toContain("It went wrong.");
  expect(text(markup)).toContain("Run sudo fix-it first.");
  expect(markup).toMatch(/<code[^>]*>sudo fix-it<\/code>/);
  expect(markup).not.toContain("`");
}

const never = (): Promise<never> => new Promise(() => {});

function pdConsole(): PdConsole {
  return new PdConsole({} as ConsoleApi);
}

describe("every view shows an error's hint (AC-AXHK2M.6)", () => {
  it("the banner, as the reference", () => {
    const session = new Session();
    session.reportError(HINTED);
    expectHint(html(ErrorBanner, { session }));
  });

  it("meter settings", () => {
    const panel = new MeterSettingsPanel(never);
    panel.error = HINTED;
    expectHint(html(MeterSettingsView, { panel, connected: true }));
  });

  it("records", () => {
    const panel = new RecordsPanel(never, never);
    panel.error = HINTED;
    expectHint(html(RecordsView, { panel, connected: true, theme: "light" }));
  });

  it("the statistics controls", () => {
    const controls = new StatsControls();
    controls.error = HINTED;
    const deps = { resetStats: never, clearHistory: never };
    expectHint(html(StatsPanel, { stats: null, controls, deps, canReset: true }));
  });

  it("the firmware outcome", () => {
    const firmware = new FirmwareUpdate({} as FirmwareUpdateApi);
    firmware.error = HINTED;
    firmware.phase = "failed";
    expectHint(html(FirmwareView, { firmware, meter: null }));
  });

  it("the trigger view, with no raw backticks", () => {
    const panel = new TriggerPanel({} as TriggerApi);
    panel.error = HINTED;
    expectHint(html(TriggerView, { panel, connected: true, pdConsole: pdConsole() }));
  });

  it("a failed trigger request, with no raw backticks", () => {
    const panel = new TriggerPanel({} as TriggerApi);
    panel.requestFailure = HINTED;
    expectHint(html(TriggerView, { panel, connected: true, pdConsole: pdConsole() }));
  });

  it("the console, with no raw backticks", () => {
    const console = pdConsole();
    console.error = HINTED;
    expectHint(html(PdConsoleView, { pdConsole: console }));
  });
});

describe("the firmware notice (AC-AXHK2M.7)", () => {
  it("keeps its live region mounted with nothing to say", () => {
    const markup = html(FirmwareNotice, { warnings: [] });
    expect(markup).toContain('role="status"');
    expect(markup).toContain('aria-live="polite"');
    expect(text(markup)).toBe("");
  });

  it("puts each warning inside that region", () => {
    const markup = html(FirmwareNotice, { warnings: ["Too old.", "Update it."] });
    const region = /<div[^>]*role="status"[^>]*>(.*)<\/div>/s.exec(markup)?.[1] ?? "";
    expect(text(region)).toContain("Too old.");
    expect(text(region)).toContain("Update it.");
  });
});

describe("the firmware view's notes (AC-AXHK2M.7)", () => {
  const firmware = () => new FirmwareUpdate({ cancelFlash: vi.fn() } as never);

  it("has its status region mounted before there is a note", () => {
    const markup = html(FirmwareView, { firmware: firmware(), meter: null });
    const region = /<[a-z]+[^>]*id="firmware-followed"[^>]*>(.*?)<\/[a-z]+>/s.exec(markup);
    expect(region, "no status region").not.toBeNull();
    expect(region?.[0]).toContain('role="status"');
    expect(text(region?.[1] ?? "x")).toBe("");
  });

  it("has the lost note's region mounted before its text", () => {
    const markup = html(FirmwareView, { firmware: firmware(), meter: null });
    const region = /<[a-z]+[^>]*id="firmware-lost"[^>]*>(.*?)<\/[a-z]+>/s.exec(markup);
    expect(region, "no status region").not.toBeNull();
    expect(region?.[0]).toContain('role="status"');
    expect(text(region?.[1] ?? "x")).toBe("");

    const fw = firmware();
    fw.followedLost = true;
    const filled = html(FirmwareView, { firmware: fw, meter: null });
    const lost = /<[a-z]+[^>]*id="firmware-lost"[^>]*>(.*?)<\/[a-z]+>/s.exec(filled);
    expect(text(lost?.[1] ?? "")).not.toBe("");
  });

  it("shows the hint of a file that failed inspection", () => {
    const fw = firmware();
    fw.fileError = "This is not a firmware file.";
    fw.fileErrorHint = "Pick a `.mencrypt` file.";
    const markup = html(FirmwareView, { firmware: fw, meter: null });
    expect(text(markup)).toContain("Pick a .mencrypt file.");
    expect(markup).toMatch(/<code[^>]*>\.mencrypt<\/code>/);
  });

  it("fills that same region when a followed update ended", () => {
    const fw = firmware();
    fw.followedEnded = true;
    const markup = html(FirmwareView, { firmware: fw, meter: null });
    const region = /<[a-z]+[^>]*id="firmware-followed"[^>]*>(.*?)<\/[a-z]+>/s.exec(markup);
    expect(text(region?.[1] ?? "")).toMatch(/the update has ended/i);
  });
});
