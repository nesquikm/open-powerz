/**
 * FR V9SK52 (AC .4, .5): the device details show the meter's mode and both firmware
 * versions, an outdated firmware shows a warning, and a meter in bootloader mode is
 * listed as such instead of failing to connect.
 *
 * The device details live in the About dialog while a meter is connected (region
 * "Device details"); the firmware warning is a status notice on the main page.
 * `?mock=bootloader` adds one real meter in bootloader mode to the picker.
 */

import { expect, test, type Page } from "@playwright/test";
import { open, streamDemo } from "./helpers";

async function openAbout(page: Page): Promise<void> {
  await page.locator("#top-bar-about").click();
  await expect(page.getByRole("dialog")).toBeVisible();
}

test.describe("device details (FR V9SK52)", () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 1600, height: 900 });
  });

  test("show the mode and both versions of the connected meter (AC-V9SK52.5)", async ({ page }) => {
    await open(page);
    await streamDemo(page);
    await openAbout(page);
    const details = page.getByRole("dialog").getByRole("region", { name: "Device details" });
    await expect(details).toBeVisible();
    await expect(details).toContainText("Application");
    await expect(details).toContainText("2.0.6");
    await expect(details).toContainText("2.5");
  });

  test("an outdated firmware shows a warning naming the newest version (AC-V9SK52.4)", async ({
    page,
  }) => {
    await open(page);
    await streamDemo(page);
    const warning = page.getByRole("status").filter({ hasText: "2.0.7" });
    await expect(warning).toBeVisible();
    await expect(warning).toContainText("2.0.6");
    await expect(page.getByRole("alert")).toHaveCount(0);
  });

  test("a meter in bootloader mode is listed as such and not connected (AC-V9SK52.5)", async ({
    page,
  }) => {
    await open(page, "?mock=bootloader");
    const picker = page.getByLabel("Device", { exact: true });
    const boot = picker.locator('option[value="usb:1:9"]');
    await expect(boot).toHaveCount(1);
    await expect(boot).toHaveText(/bootloader/i);
    await picker.selectOption("usb:1:9");
    await expect(page.getByRole("button", { name: "Connect" })).toBeDisabled();
    await expect(page.getByRole("alert")).toHaveCount(0);
    // The demo device still connects.
    await streamDemo(page);
  });
});
