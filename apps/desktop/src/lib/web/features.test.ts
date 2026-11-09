/**
 * FR QFTCX0: the capability map components read to hide or disable desktop-only
 * controls.
 *
 * - AC-QFTCX0.6: firmware update, logo upload, settings writes, record delete and
 *   protocol triggering are off in the web build, with the "Desktop app only" text and
 *   the releases page link.
 * - AC-QFTCX0.7: settings read, the offline record list and record download are
 *   capability flags too (on only when they use the safe read set).
 * - AC-W8SHX8.7: the web build switches the settings read, the record list and the
 *   record download on; firmware update and file reveal stay off.
 * - The desktop (Tauri and mock) builds have every feature on.
 */

import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import {
  DESKTOP_FEATURES,
  DESKTOP_ONLY_TEXT,
  RELEASES_URL,
  WEB_FEATURES,
  type Features,
} from "./features";

const WRITE_FEATURES = [
  "firmwareUpdate",
  "logoUpload",
  "settingsWrite",
  "recordDelete",
  "trigger",
] as const satisfies readonly (keyof Features)[];

const READ_FEATURES = [
  "settingsRead",
  "recordsList",
  "recordDownload",
] as const satisfies readonly (keyof Features)[];

describe("feature map (AC-QFTCX0.6, AC-QFTCX0.7)", () => {
  it("names the desktop-only text and the releases page", () => {
    expect(DESKTOP_ONLY_TEXT).toBe("Desktop app only");
    expect(RELEASES_URL).toBe("https://github.com/nesquikm/open-powerz/releases");
  });

  it("turns every meter write off in the web build", () => {
    for (const key of WRITE_FEATURES) expect(WEB_FEATURES[key], key).toBe(false);
  });

  it("has a boolean flag for each meter read", () => {
    for (const key of READ_FEATURES) expect(typeof WEB_FEATURES[key], key).toBe("boolean");
  });

  it("switches the settings read, record list and record download on (AC-W8SHX8.7)", () => {
    for (const key of READ_FEATURES) expect(WEB_FEATURES[key], key).toBe(true);
  });

  it("keeps firmware update and file reveal off in the web build (AC-W8SHX8.7)", () => {
    expect(WEB_FEATURES.firmwareUpdate).toBe(false);
    expect(WEB_FEATURES.fileReveal).toBe(false);
  });

  it("keeps every feature on in the desktop build", () => {
    for (const key of [...WRITE_FEATURES, ...READ_FEATURES]) {
      expect(DESKTOP_FEATURES[key], key).toBe(true);
    }
    expect(Object.values(DESKTOP_FEATURES).every((v) => v === true)).toBe(true);
    expect(Object.keys(WEB_FEATURES).sort()).toEqual(Object.keys(DESKTOP_FEATURES).sort());
  });

  it("holds no WebUSB code (backend.ts imports it statically)", () => {
    const src = readFileSync(new URL("./features.ts", import.meta.url), "utf8");
    expect(src).not.toMatch(/navigator\.usb|navigator\.hid|requestDevice|transferOut|import\s*\(/);
    expect(src).not.toMatch(/from\s+["']\.\/(webusb|webhid|driver|webBackend|demoTransport)["']/);
  });
});
