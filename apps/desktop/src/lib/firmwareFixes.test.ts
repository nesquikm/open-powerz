/**
 * FR 4PJRVJ: the firmware thresholds come from the backend (AC .4), release notes that
 * number every line are recognised (AC .8) and the "update required" branches follow
 * AC-V9SK52.3 (AC .8).
 */

import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import type { ConnectedInfo, FirmwareFileInfo } from "./backend";
import { deviceDetailsRows, firmwareWarningText } from "./deviceDetails";
import { firmwareNotes, notesNumbered } from "./firmwareFile";

function info(overrides: Record<string, unknown> = {}): ConnectedInfo {
  return {
    model: "KM003C",
    firmware_version: "1.2.3",
    bootloader_version: "2.5",
    mode: "application",
    warnings: [],
    update_required: false,
    firmware_problems: [],
    serial: "SERIAL-0004",
    rate_sps: 50,
    rate_locked: false,
    session_id: 1,
    min_supported_firmware: "1.8.0",
    known_latest_firmware: "2.1.0",
    ...overrides,
  } as ConnectedInfo;
}

function fileWith(release_notes_en: string): FirmwareFileInfo {
  return {
    vendor: "ChargerLab",
    container_version: "V1.0",
    model: "KM003C",
    version: "2.0.7",
    date: "2026.9.29",
    crc: "A175E4A0",
    size: 1,
    release_notes_en,
  };
}

/** `Version::new(a, b, c)` of the Rust constant `name`, as `a.b.c`. */
function rustConstant(source: string, name: string): string {
  const m = source.match(new RegExp(`${name}: Version = Version::new\\((\\d+), (\\d+), (\\d+)\\)`));
  if (m === null) throw new Error(`no ${name} in version.rs`);
  return m.slice(1, 4).join(".");
}

describe("firmware thresholds (AC-4PJRVJ.4)", () => {
  it("words the warnings with the thresholds the backend sent", () => {
    expect(firmwareWarningText("too_old", info())).toContain("1.8.0");
    expect(firmwareWarningText("too_old", info())).not.toContain("1.7.0");
    expect(firmwareWarningText("outdated", info())).toContain("2.1.0");
    expect(firmwareWarningText("outdated", info())).not.toContain("2.0.7");
  });

  it("has the mock backend send the library's thresholds", () => {
    const rust = readFileSync(
      new URL("../../../../crates/powerz-protocol/src/firmware/version.rs", import.meta.url),
      "utf8",
    );
    const mock = readFileSync(new URL("./mockBackend.ts", import.meta.url), "utf8");
    const min = rustConstant(rust, "MIN_SUPPORTED");
    const latest = rustConstant(rust, "KNOWN_LATEST");
    expect(mock).toContain(`min_supported_firmware: "${min}"`);
    expect(mock).toContain(`known_latest_firmware: "${latest}"`);
  });
});

describe("numbered release notes (AC-4PJRVJ.8)", () => {
  const numbered = (text: string) => notesNumbered(firmwareNotes(fileWith(text)));

  it("recognises notes that number every line", () => {
    expect(numbered("1. First.\n2. Second.")).toBe(true);
    expect(numbered("1. Only one.")).toBe(true);
    expect(numbered("1、First.\n2、Second.")).toBe(true);
    expect(numbered("1) First.\n10) Tenth.")).toBe(true);
  });

  it("keeps the bullets for anything else", () => {
    expect(numbered("1. First.\nA plain line.")).toBe(false);
    expect(numbered("Plain.\nLines.")).toBe(false);
    expect(numbered("2.0.7 was released.")).toBe(false);
    expect(numbered("")).toBe(false);
  });
});

describe("the update check (AC-4PJRVJ.8, AC-V9SK52.3)", () => {
  const updateCheck = (i: ConnectedInfo) =>
    deviceDetailsRows(i).find((r) => r.label === "Update check")?.value;

  it("never says update required", () => {
    expect(updateCheck(info({ update_required: true }))).not.toMatch(/update required/i);
  });

  it("keeps up to date and unavailable", () => {
    expect(updateCheck(info({ update_required: false }))).toBe("up to date");
    expect(updateCheck(info({ update_required: null }))).toBe("unavailable");
  });

  it("has no warning text that says an update is required", () => {
    const text = (firmwareWarningText as (c: string, i: ConnectedInfo) => string | undefined)(
      "update_required",
      info(),
    );
    expect(text ?? "").not.toMatch(/update required/i);
  });
});
