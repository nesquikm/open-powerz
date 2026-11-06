/** The desktop backend reference describes the logo settings check (FR QG6RNY, AC .9):
 * the `logo_settings_check` command and its `logoSettingsCheck` seam function, the new
 * write result fields, the three settings states, and the `settings_not_saved` error kind. */

import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const DOC = readFileSync(
  new URL("../../../../docs/reference/desktop-backend.md", import.meta.url),
  "utf8",
);

/** The doc section that names the logo settings check. */
function logoSection(): string {
  const parts = DOC.split(/^## /m);
  const hit = parts.find((p) => p.includes("logo_settings_check"));
  return hit ?? "";
}

describe("desktop backend reference (AC-QG6RNY.9)", () => {
  it("qg6rny: describes the logo_settings_check command and its seam function", () => {
    const s = logoSection();
    expect(s, "a section names logo_settings_check").not.toBe("");
    expect(s).toContain("logoSettingsCheck");
    for (const state of ["saved", "needs_menu_save", "calibration_changed"]) {
      expect(s).toContain(state);
    }
    expect(s).toMatch(/read-only|reads? the settings|nothing (is )?written/i);
  });

  it("qg6rny: describes the new write result fields", () => {
    const s = logoSection();
    for (const field of ["duration_before", "duration_set", "startup_duration"]) {
      expect(s).toContain(field);
    }
    expect(DOC).not.toContain("startup_screen_off");
  });

  it("qg6rny: describes the settings_not_saved error kind with the steps as its hint", () => {
    expect(DOC).toContain("settings_not_saved");
    const at = DOC.indexOf("settings_not_saved");
    const around = DOC.slice(Math.max(0, at - 600), at + 600);
    expect(around).toMatch(/hint/i);
    expect(around).toMatch(/menu/i);
  });
});
