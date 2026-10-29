/**
 * FR 1HHT01 (AC .2, .4): a read-only "Meter settings" panel on the Meter tab shows
 * the connected meter's settings, each with its confidence tag; it is read on
 * connect and again on the Refresh button, and goes away on disconnect.
 *
 * The mock counts settings reads on `window.__OPEN_POWERZ_MOCK__.meterSettingsReads()`.
 */

import { expect, test, type Page } from "@playwright/test";
import { open, streamDemo } from "./helpers";

const MOCK_HOOK = "__OPEN_POWERZ_MOCK__";

function reads(page: Page): Promise<number> {
  return page.evaluate(
    (key) =>
      (window as unknown as Record<string, { meterSettingsReads(): number }>)[
        key
      ].meterSettingsReads(),
    MOCK_HOOK,
  );
}

function panel(page: Page) {
  return page.getByRole("region", { name: "Meter settings" });
}

test.describe("meter settings panel (FR 1HHT01)", () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 1600, height: 900 });
  });

  test("shows every field with its tag once a meter connects (AC-1HHT01.2, .4)", async ({
    page,
  }) => {
    await open(page);
    await streamDemo(page);
    const p = panel(page);
    await expect(p).toBeVisible();
    for (const text of [
      "Language",
      "English",
      "Brightness",
      "50%",
      "Theme",
      "blue",
      "Rotation",
      "90°",
      "50 SPS",
      "606, 606, 606, 606, 606",
    ]) {
      await expect(p).toContainText(text);
    }
    await expect(p.getByText("confirmed", { exact: true }).first()).toBeVisible();
    await expect(p.getByText("inferred", { exact: true }).first()).toBeVisible();
    await expect.poll(() => reads(page)).toBe(1);
  });

  test("is read-only until Edit: no input controls, only Refresh and Edit (AC-1HHT01.4, AC-JKWT3W.5)", async ({
    page,
  }) => {
    await open(page);
    await streamDemo(page);
    const p = panel(page);
    await expect(p).toContainText("English");
    await expect(p.locator("input, select, textarea, [contenteditable='true']")).toHaveCount(0);
    await expect(p.getByRole("button")).toHaveCount(2);
    await expect(p.getByRole("button", { name: /refresh/i })).toBeVisible();
    await expect(p.getByRole("button", { name: "Edit" })).toBeVisible();
  });

  test("Refresh reads the settings again (AC-1HHT01.4)", async ({ page }) => {
    await open(page);
    await streamDemo(page);
    const p = panel(page);
    await expect(p).toContainText("English");
    await expect.poll(() => reads(page)).toBe(1);
    await p.getByRole("button", { name: /refresh/i }).click();
    await expect.poll(() => reads(page)).toBe(2);
    await expect(p).toContainText("English");
  });

  test("shows no meter values before connecting and after disconnecting (AC-1HHT01.4)", async ({
    page,
  }) => {
    await open(page);
    await expect(page.getByText("English", { exact: true })).toHaveCount(0);
    await streamDemo(page);
    await expect(panel(page)).toContainText("English");
    await page.getByRole("button", { name: "Disconnect" }).click();
    await expect(page.getByRole("button", { name: "Connect" })).toBeVisible();
    await expect(page.getByText("English", { exact: true })).toHaveCount(0);
  });
});
