/**
 * FR AXHK2M on the mocked backend, the reopened ACs from the a5442bc hardware run:
 * a stop the user asked for looks stopped, not failed (AC .13), its next step is a plain
 * replug first (AC .14), and after an update the page connects again to the same meter
 * (AC .15) and shows its versions, never a dash (AC .16).
 */

import { expect, test, type Locator, type Page } from "@playwright/test";
import { badge, open, streamDemo } from "./helpers";

const ADVANCED_KEY = "open-powerz.advanced-firmware";

function panel(page: Page): Locator {
  return page.locator("#view-firmware");
}

function confirmDialog(page: Page): Locator {
  return page.getByRole("dialog", { name: "Confirm firmware update" });
}

/** Stream the demo, choose the file and start the update, agreeing to end the session. */
async function startUpdate(page: Page): Promise<void> {
  await open(page);
  await streamDemo(page);
  await page.getByRole("tab", { name: "Firmware" }).click();
  await panel(page)
    .getByRole("button", { name: /choose file/i })
    .click();
  await expect(panel(page).getByRole("region", { name: "Firmware file" })).toBeVisible();
  await panel(page).getByRole("button", { name: "Update", exact: true }).click();
  await confirmDialog(page)
    .getByRole("textbox", { name: /version/i })
    .fill("2.0.7");
  await confirmDialog(page).getByRole("button", { name: "Start update" }).click();
  await page
    .getByRole("dialog", { name: /session|recording/i })
    .getByRole("button", { name: /^end /i })
    .click();
}

/** Every element in the firmware view drawn in the theme's danger colour. */
function dangerElements(page: Page): Promise<string[]> {
  return page.evaluate(() => {
    const probe = document.createElement("span");
    probe.style.color = "var(--danger)";
    document.body.append(probe);
    const danger = getComputedStyle(probe).color;
    probe.remove();
    const root = document.getElementById("view-firmware");
    if (root === null) return ["no firmware view"];
    return [...root.querySelectorAll<HTMLElement>("*")]
      .filter((el) => el.offsetParent !== null && (el.textContent ?? "").trim() !== "")
      .filter((el) => {
        const s = getComputedStyle(el);
        return s.color === danger || s.backgroundColor === danger || s.borderColor === danger;
      })
      .map((el) => `${el.tagName.toLowerCase()}: ${(el.textContent ?? "").trim().slice(0, 60)}`);
  });
}

test.describe("a stopped firmware update (FR AXHK2M)", () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 1600, height: 900 });
    await page.addInitScript((key) => localStorage.setItem(key, "true"), ADVANCED_KEY);
  });

  test("a stop shows as stopped: no error colour, no alert, no retry (AC-AXHK2M.13)", async ({
    page,
  }) => {
    await startUpdate(page);
    const cancel = panel(page).getByRole("button", { name: "Cancel update" });
    await expect(cancel).toBeEnabled();
    await cancel.click();

    await expect(
      panel(page)
        .getByText(/stopped/i)
        .first(),
    ).toBeVisible();
    await expect(panel(page).getByRole("alert")).toHaveCount(0);
    await expect(panel(page)).not.toContainText(/retry the update/i);
    await expect(panel(page)).not.toContainText(/failed/i);
    await expect(panel(page)).not.toContainText(/interrupted:|JUMP_DFU/);
    await expect(panel(page)).toContainText(/bootloader/i);
    expect(await dangerElements(page)).toEqual([]);
  });

  test("after a stop, replug first and hold the button only if that fails (AC-AXHK2M.14)", async ({
    page,
  }) => {
    await startUpdate(page);
    await panel(page).getByRole("button", { name: "Cancel update" }).click();
    await expect(
      panel(page)
        .getByText(/stopped/i)
        .first(),
    ).toBeVisible();

    const steps = panel(page)
      .getByRole("listitem")
      .filter({ hasText: /unplug|replug|hold/i });
    await expect(steps.first()).toBeVisible();
    const texts = await steps.allInnerTexts();
    expect(texts[0]).toMatch(/unplug|replug/i);
    expect(texts[0]).not.toMatch(/\bhold|button/i);
    const hold = texts.findIndex((t) => /\bhold/i.test(t));
    expect(hold).toBeGreaterThan(0);
    expect(texts[hold]).toMatch(/\bif\b/i);
  });

  test("after a done update the meter streams again with its new version (AC-AXHK2M.15, .16)", async ({
    page,
  }) => {
    await startUpdate(page);
    const done = panel(page)
      .getByRole("status")
      .filter({ hasText: /updated/i });
    await expect(done).toBeVisible({ timeout: 30_000 });

    // Connected again on its own, to the same meter.
    await expect(badge(page)).toHaveText("Streaming", { timeout: 10_000 });
    const meter = panel(page).getByRole("region", { name: "Meter" });
    // The versions are read anew: the new firmware, and no dash for either.
    await expect(meter.locator("dt", { hasText: "Firmware" }).locator("+ dd")).toHaveText("2.0.7");
    await expect(meter.locator("dt", { hasText: "Bootloader" }).locator("+ dd")).not.toHaveText(
      "—",
    );
  });

  test("after a stop the meter streams again (AC-AXHK2M.15)", async ({ page }) => {
    await startUpdate(page);
    await panel(page).getByRole("button", { name: "Cancel update" }).click();
    await expect(
      panel(page)
        .getByText(/stopped/i)
        .first(),
    ).toBeVisible();
    await expect(badge(page)).toHaveText("Streaming", { timeout: 10_000 });
    const meter = panel(page).getByRole("region", { name: "Meter" });
    await expect(meter.locator("dt", { hasText: "Firmware" }).locator("+ dd")).not.toHaveText("—");
  });
});
