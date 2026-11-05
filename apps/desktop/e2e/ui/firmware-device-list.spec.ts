/**
 * FR XCCQAK on the mocked backend: the Update progress section keeps a "Device list" line
 * of the labels the updated meter's entry took in the device select (AC .1-.3), read
 * after a mocked update with Hide serial numbers off and on (AC .4).
 *
 * `?mock=replug` lists one real meter (`MOCK_REPLUG.serial`) at USB address 7. Updating it
 * without connecting it first, the mock lists that meter in bootloader mode once the
 * update has switched it, and in application mode again before the update ends, as the
 * backend sees a real meter jump.
 *
 * `?mock=replug-late` (FR PGY704) is the same, except that the meter is still listed in
 * bootloader mode when the update ends and is listed in application mode again about
 * 1.5 s later, as the real backend sees it.
 *
 * The line reads `<label> → <label> → …`, each label the closed select's own text.
 */

import { expect, test, type Locator, type Page } from "@playwright/test";
import { MOCK_REPLUG } from "../../src/lib/mockBackend";
import { open } from "./helpers";

const HIDE_KEY = "open-powerz.hide-serial";
const ADVANCED_KEY = "open-powerz.advanced-firmware";
const REAL_METER = "usb:1:7";
/** The whole mocked update, its stages and both jumps, takes about 8 s. */
const UPDATE_MS = 20_000;

const SHOWN = `KM003C — ${MOCK_REPLUG.serial}`;
const HIDDEN = "KM003C — Hidden";

test.use({ viewport: { width: 1600, height: 900 } });

function panel(page: Page): Locator {
  return page.locator("#view-firmware");
}

function progress(page: Page): Locator {
  return panel(page).getByRole("region", { name: "Update progress" });
}

/** The labels on the progress section's "Device list" line, in order; null without one. */
async function deviceListLine(page: Page): Promise<string[] | null> {
  const text = await progress(page).innerText();
  const m = /Device list:?\s*([^\n]+)/.exec(text);
  if (m === null) return null;
  return m[1].split(/\s*→\s*/).map((s) => s.trim());
}

/** Pick the listed real meter, without connecting it, and start its update. */
async function startUpdateOfRealMeter(page: Page, query = "?mock=replug"): Promise<void> {
  await open(page, query);
  await page.getByLabel("Device", { exact: true }).selectOption(REAL_METER);
  await page.getByRole("tab", { name: "Firmware" }).click();
  await panel(page)
    .getByRole("button", { name: /choose file/i })
    .click();
  await expect(panel(page).getByRole("region", { name: "Firmware file" })).toBeVisible();
  await panel(page).getByRole("button", { name: "Update", exact: true }).click();
  const confirm = page.getByRole("dialog", { name: "Confirm firmware update" });
  await confirm.getByRole("textbox", { name: /version/i }).fill("2.0.7");
  await confirm.getByRole("button", { name: "Start update" }).click();
  await expect(progress(page)).toBeVisible();
}

async function waitForDone(page: Page): Promise<void> {
  await expect(panel(page).getByText("The meter was updated to 2.0.7.")).toBeVisible({
    timeout: UPDATE_MS,
  });
}

test.describe("the firmware update's device list line (FR XCCQAK)", () => {
  test.beforeEach(async ({ page }) => {
    await page.addInitScript((key) => localStorage.setItem(key, "true"), ADVANCED_KEY);
  });

  test("with the switch off, the line names the bootloader label between the two application ones (AC-XCCQAK.1, .2, .4)", async ({
    page,
  }) => {
    await startUpdateOfRealMeter(page);

    // While the update runs: the line is there, and gains the bootloader label.
    await expect.poll(() => deviceListLine(page)).toEqual([SHOWN]);
    await expect
      .poll(() => deviceListLine(page), { timeout: UPDATE_MS })
      .toEqual([SHOWN, `${SHOWN} (bootloader mode)`]);
    await expect(panel(page).getByRole("button", { name: /cancel/i })).toBeVisible();

    await waitForDone(page);
    expect(await deviceListLine(page)).toEqual([SHOWN, `${SHOWN} (bootloader mode)`, SHOWN]);
  });

  test("with the switch on, no serial appears in the line (AC-XCCQAK.2, .4)", async ({ page }) => {
    await page.addInitScript((key) => localStorage.setItem(key, "true"), HIDE_KEY);
    await startUpdateOfRealMeter(page);
    await waitForDone(page);

    expect(await deviceListLine(page)).toEqual([HIDDEN, `${HIDDEN} (bootloader mode)`, HIDDEN]);
    expect(await progress(page).innerText()).not.toContain(MOCK_REPLUG.serial);
  });

  test("the switch restyles the kept line, which stays until Close (AC-XCCQAK.2, .3)", async ({
    page,
  }) => {
    await startUpdateOfRealMeter(page);
    await waitForDone(page);
    expect(await deviceListLine(page)).toEqual([SHOWN, `${SHOWN} (bootloader mode)`, SHOWN]);

    await page.locator("#top-bar-about").click();
    const about = page.getByRole("dialog");
    await about.getByRole("switch", { name: "Hide serial numbers" }).click();
    await page.keyboard.press("Escape");
    await expect
      .poll(() => deviceListLine(page))
      .toEqual([HIDDEN, `${HIDDEN} (bootloader mode)`, HIDDEN]);

    await panel(page).getByRole("button", { name: "Close", exact: true }).click();
    await expect(progress(page)).toHaveCount(0);
    await expect(panel(page).getByText(/Device list/)).toHaveCount(0);
  });
});

test.describe("a relisting after the update ended (FR PGY704)", () => {
  test.beforeEach(async ({ page }) => {
    await page.addInitScript((key) => localStorage.setItem(key, "true"), ADVANCED_KEY);
  });

  test("a relisting that arrives after the update ended still adds the third label", async ({
    page,
  }) => {
    await startUpdateOfRealMeter(page, "?mock=replug-late");
    await waitForDone(page);

    // At the outcome the meter is still listed in bootloader mode.
    expect(await deviceListLine(page)).toEqual([SHOWN, `${SHOWN} (bootloader mode)`]);
    // It is listed in application mode again only after the update ended.
    await expect
      .poll(() => deviceListLine(page), { timeout: 10_000 })
      .toEqual([SHOWN, `${SHOWN} (bootloader mode)`, SHOWN]);

    await page.locator("#top-bar-about").click();
    const about = page.getByRole("dialog");
    await about.getByRole("switch", { name: "Hide serial numbers" }).click();
    await page.keyboard.press("Escape");
    await expect
      .poll(() => deviceListLine(page))
      .toEqual([HIDDEN, `${HIDDEN} (bootloader mode)`, HIDDEN]);
    expect(await progress(page).innerText()).not.toContain(MOCK_REPLUG.serial);
  });
});
