/**
 * FR V9SK52 (AC .4, .5): the device details show the meter's mode and both firmware
 * versions, an outdated firmware shows a warning, and a meter in bootloader mode is
 * listed as such instead of failing to connect.
 *
 * The device details live in the About dialog while a meter is connected (region
 * "Device details"); the firmware warning is a status notice on the main page.
 * `?mock=bootloader` adds one real meter in bootloader mode to the picker.
 *
 * FR AXHK2M (AC .17): `?mock=mode-flip` lists one real meter in application mode that
 * turns to bootloader mode under the same entry after `MOCK_MODE_FLIP.appMs`, and back
 * after `MOCK_MODE_FLIP.bootMs` more.
 */

import { expect, test, type Page } from "@playwright/test";
import { MOCK_MODE_FLIP } from "../../src/lib/mockBackend";
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

  test("a picked meter changing mode under the same entry is shown in its new mode, open and closed (AC-AXHK2M.17)", async ({
    page,
  }) => {
    await open(page, "?mock=mode-flip");
    const picker = page.getByLabel("Device", { exact: true });
    const id = `usb:1:${MOCK_MODE_FLIP.address}`;
    const option = picker.locator(`option[value="${id}"]`);
    await expect(option).toHaveCount(1);
    await expect(option).not.toHaveText(/bootloader mode/);
    await picker.selectOption(id);

    /** The closed select: its value, the text it shows, its tooltip. */
    const closed = () =>
      picker.evaluate((el: HTMLSelectElement) => ({
        value: el.value,
        text: el.selectedOptions[0]?.text.trim() ?? "",
        title: el.title,
      }));
    // Tag the option the select shows now. WebKit keeps a closed select's painted
    // text when only its picked option's text changes in place, so the flipped entry
    // must come as a new option for the closed select to repaint.
    await option.evaluate((el) => el.setAttribute("data-e2e-before-flip", "1"));

    // Into the bootloader: within the 1 s the AC allows after the change.
    await expect(option).toHaveText(/\(bootloader mode\)/, {
      timeout: MOCK_MODE_FLIP.appMs + 1_000,
    });
    await expect.poll(closed).toMatchObject({ value: id });
    const boot = await closed();
    expect(boot.text).toContain("(bootloader mode)");
    expect(boot.title).toContain("(bootloader mode)");
    await expect(picker.locator("option[data-e2e-before-flip]")).toHaveCount(0);

    // And back: the suffix goes from both.
    await option.evaluate((el) => el.setAttribute("data-e2e-before-back", "1"));
    await expect(option).not.toHaveText(/bootloader mode/, {
      timeout: MOCK_MODE_FLIP.bootMs + 1_000,
    });
    const back = await closed();
    expect(back.value).toBe(id);
    expect(back.text).not.toContain("bootloader mode");
    expect(back.title).not.toContain("bootloader mode");
    await expect(picker.locator("option[data-e2e-before-back]")).toHaveCount(0);
    await expect(page.getByRole("alert")).toHaveCount(0);
  });
});
