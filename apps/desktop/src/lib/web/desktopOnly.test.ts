/**
 * FR QFTCX0, AC-QFTCX0.6: a desktop-only control renders disabled with the text
 * "Desktop app only" and a link to the releases page, and the views of the meter
 * writes gate their controls through it.
 *
 * Rendered with `svelte/server` (no DOM needed).
 */

import { readFileSync } from "node:fs";
import { createRawSnippet } from "svelte";
import { render } from "svelte/server";
import { describe, expect, it } from "vitest";
import DesktopOnly from "../components/DesktopOnly.svelte";
import { RELEASES_URL } from "./features";

type Props = Record<string, unknown>;

function html(props: Props): string {
  return render(DesktopOnly as never, { props: props as never }).body;
}

const child = createRawSnippet(() => ({ render: () => "<button>Flash firmware</button>" }));

describe("DesktopOnly (AC-QFTCX0.6)", () => {
  it("renders the control disabled, says Desktop app only and links the releases page", () => {
    const body = html({ available: false, label: "Update firmware", children: child });
    expect(body).toContain("Desktop app only");
    expect(body).toContain("Update firmware");
    expect(body).toMatch(/<button[^>]*\sdisabled/);
    expect(body).toContain(`href="${RELEASES_URL}"`);
    // The real control is not rendered when the feature is off.
    expect(body).not.toContain("Flash firmware");
  });

  it("renders the real control untouched when the feature is available", () => {
    const body = html({ available: true, label: "Update firmware", children: child });
    expect(body).toContain("<button>Flash firmware</button>");
    expect(body).not.toContain("Desktop app only");
    expect(body).not.toContain(RELEASES_URL);
  });
});

describe("the meter-write views gate their controls (AC-QFTCX0.6)", () => {
  const views: [file: string, feature: string][] = [
    ["FirmwareView.svelte", "firmwareUpdate"],
    ["StartupLogoView.svelte", "logoUpload"],
    ["MeterSettingsView.svelte", "settingsWrite"],
    ["RecordsView.svelte", "recordDelete"],
    ["TriggerView.svelte", "trigger"],
  ];

  for (const [file, feature] of views) {
    it(`${file} wraps its ${feature} control in DesktopOnly`, () => {
      const src = readFileSync(new URL(`../components/${file}`, import.meta.url), "utf8");
      expect(src).toMatch(/import\s+DesktopOnly\s+from\s+["']\.\/DesktopOnly\.svelte["']/);
      expect(src).toMatch(new RegExp(`<DesktopOnly[^>]*features\\.${feature}\\b`));
    });
  }
});

describe("the meter-read views gate their reads (AC-QFTCX0.7)", () => {
  const reads: [file: string, feature: string][] = [
    ["MeterSettingsView.svelte", "settingsRead"],
    ["RecordsView.svelte", "recordsList"],
    ["RecordsView.svelte", "recordDownload"],
  ];

  for (const [file, feature] of reads) {
    it(`${file} wraps its ${feature} control in DesktopOnly`, () => {
      const src = readFileSync(new URL(`../components/${file}`, import.meta.url), "utf8");
      expect(src).toMatch(new RegExp(`<DesktopOnly[^>]*features\\.${feature}\\b`));
    });
  }
});
