/**
 * FR AXHK2F on the mocked backend: the prompt names every session an update would end
 * and says which of them record (AC .4). Declining keeps them running and returns focus
 * to the confirmation field; agreeing starts the update.
 */

import { expect, test, type Locator, type Page } from "@playwright/test";
import { DEMO_LOAD_LABEL, open, streamDemo } from "./helpers";

function panel(page: Page): Locator {
  return page.locator("#view-firmware");
}

function confirmDialog(page: Page): Locator {
  return page.getByRole("dialog", { name: "Confirm firmware update" });
}

function prompt(page: Page): Locator {
  return page.getByRole("dialog", { name: /session|recording/i });
}

function flashCount(page: Page): Promise<number> {
  return page.evaluate(() => {
    const mock = (window as unknown as Record<string, { firmwareFlashLog(): unknown[] }>)
      .__OPEN_POWERZ_MOCK__;
    return mock ? mock.firmwareFlashLog().length : 0;
  });
}

async function requestStart(page: Page): Promise<void> {
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
}

test.describe("firmware consent (FR AXHK2F)", () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 1600, height: 900 });
    await page.addInitScript(() => localStorage.setItem("open-powerz.advanced-firmware", "true"));
    await open(page);
    await streamDemo(page);
  });

  test("a streaming session is named and the update waits (AC-AXHK2F.4)", async ({ page }) => {
    await requestStart(page);

    const session = prompt(page).getByRole("listitem").filter({ hasText: DEMO_LOAD_LABEL });
    await expect(session).toBeVisible();
    await expect(session).not.toContainText(/recording/i);
    expect(await flashCount(page)).toBe(0);

    await prompt(page).getByRole("button", { name: /^end /i }).click();
    await expect.poll(() => flashCount(page)).toBe(1);
  });

  test("a recording session says so, and declining keeps it running (AC-AXHK2F.4)", async ({
    page,
  }) => {
    await page
      .getByRole("button", { name: /^(start )?record/i })
      .first()
      .click();
    await requestStart(page);

    const session = prompt(page).getByRole("listitem").filter({ hasText: DEMO_LOAD_LABEL });
    await expect(session).toContainText(/recording/i);

    await prompt(page)
      .getByRole("button", { name: /^keep /i })
      .click();
    await expect(prompt(page)).toHaveCount(0);
    const field = confirmDialog(page).getByRole("textbox", { name: /version/i });
    await expect(field).toBeFocused();
    await expect(field).toHaveValue("2.0.7");
    expect(await flashCount(page)).toBe(0);
    await expect(page.getByText(/REC \d+:\d\d:\d\d/).first()).toBeVisible();
  });
});
