/**
 * FR JKWT3W (AC .3, .4, .5 and .6): the Meter settings panel gets an Edit button;
 * in edit mode only the writable fields have controls, a summary lists the change
 * before Apply sends it, and the panel then shows the block as read back.
 * `?settingsfault=mismatch` keeps the old values so the re-read does not match: the
 * error names the field and the write is not retried.
 *
 * The mock counts writes on `window.__OPEN_POWERZ_MOCK__.meterSettingsWrites()`.
 */

import { expect, test, type Page } from "@playwright/test";
import { open, streamDemo } from "./helpers";

const MOCK_HOOK = "__OPEN_POWERZ_MOCK__";

function writes(page: Page): Promise<number> {
  return page.evaluate(
    (key) =>
      (window as unknown as Record<string, { meterSettingsWrites(): number }>)[
        key
      ].meterSettingsWrites(),
    MOCK_HOOK,
  );
}

function panel(page: Page) {
  return page.getByRole("region", { name: "Meter settings" });
}

async function startEdit(page: Page, query = "") {
  await open(page, query);
  await streamDemo(page);
  const p = panel(page);
  await expect(p).toContainText("50%");
  await p.getByRole("button", { name: "Edit" }).click();
  return p;
}

test.describe("meter settings edit (FR JKWT3W)", () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 1600, height: 900 });
  });

  test("edit mode has controls for the writable fields only (AC-JKWT3W.3, .5)", async ({
    page,
  }) => {
    const p = await startEdit(page);
    for (const label of [
      "Language",
      "Brightness",
      "Dim screen after",
      "Theme",
      "Rotation",
      "Sample rate",
    ]) {
      await expect(p.getByLabel(label, { exact: true })).toBeVisible();
    }
    for (const label of [
      "Startup logo",
      "Auto-rotate",
      "Voltage calibration (raw)",
      "Current calibration (raw)",
      "Offset calibration (raw)",
    ]) {
      await expect(p.getByLabel(label, { exact: true })).toHaveCount(0);
    }
    await expect(p.locator("input, select")).toHaveCount(6);
    await expect(p.getByLabel("Brightness", { exact: true })).toHaveValue("50");
    await expect(p.getByLabel("Theme", { exact: true })).toHaveValue("0");
    // Nothing changed yet: Apply is disabled.
    await expect(p.getByRole("button", { name: "Apply" })).toBeDisabled();
  });

  test("the summary lists the change before Apply sends it (AC-JKWT3W.5, .6)", async ({ page }) => {
    const p = await startEdit(page);
    await p.getByLabel("Brightness", { exact: true }).fill("80");
    await p.getByLabel("Theme", { exact: true }).selectOption({ label: "pink" });
    await expect(p).toContainText("Brightness: 50% → 80%");
    await expect(p).toContainText("Theme: blue → pink");
    expect(await writes(page)).toBe(0);
    await p.getByRole("button", { name: "Apply" }).click();
    await expect.poll(() => writes(page)).toBe(1);
    // Back to the read-only rows, as read back from the meter.
    await expect(p.locator("input, select")).toHaveCount(0);
    await expect(p).toContainText("80%");
    await expect(p).toContainText("pink");
    await expect(p.getByRole("button", { name: "Edit" })).toBeVisible();
  });

  test("Cancel leaves the meter untouched (AC-JKWT3W.5)", async ({ page }) => {
    const p = await startEdit(page);
    await p.getByLabel("Brightness", { exact: true }).fill("80");
    await p.getByRole("button", { name: "Cancel" }).click();
    await expect(p.locator("input, select")).toHaveCount(0);
    await expect(p).toContainText("50%");
    expect(await writes(page)).toBe(0);
  });

  test("a brightness outside 1..100 cannot be applied (AC-JKWT3W.3)", async ({ page }) => {
    const p = await startEdit(page);
    await p.getByLabel("Brightness", { exact: true }).fill("150");
    await expect(p.getByRole("button", { name: "Apply" })).toBeDisabled();
    expect(await writes(page)).toBe(0);
  });

  test("a mismatching re-read shows the error once (AC-JKWT3W.4)", async ({ page }) => {
    const p = await startEdit(page, "?settingsfault=mismatch");
    await p.getByLabel("Brightness", { exact: true }).fill("80");
    await p.getByRole("button", { name: "Apply" }).click();
    await expect(p.getByRole("alert")).toContainText(/brightness/i);
    await page.waitForTimeout(500);
    expect(await writes(page)).toBe(1);
  });
});
