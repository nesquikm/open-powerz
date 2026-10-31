/**
 * FR 4PJRVJ on the mocked backend: Cancel until the erase starts (AC .1), the prompt
 * before a running recording ends (AC .2), a running update after a page reload (AC .3),
 * the firmware notice's live region (AC .6) and numbered release notes (AC .8).
 *
 * `?mock=firmware-running` opens the page as a reload does during an update: the
 * backend already runs one, at the write stage.
 */

import { expect, test, type Locator, type Page } from "@playwright/test";
import { open, streamDemo } from "./helpers";

const ADVANCED_KEY = "open-powerz.advanced-firmware";

async function useAdvancedFirmware(page: Page): Promise<void> {
  await page.addInitScript((key) => localStorage.setItem(key, "true"), ADVANCED_KEY);
}

function panel(page: Page): Locator {
  return page.locator("#view-firmware");
}

function confirmDialog(page: Page): Locator {
  return page.getByRole("dialog", { name: "Confirm firmware update" });
}

function cancelButton(page: Page): Locator {
  return panel(page).getByRole("button", { name: "Cancel update" });
}

function flashCount(page: Page): Promise<number> {
  return page.evaluate(() => {
    const mock = (window as unknown as Record<string, { firmwareFlashLog(): unknown[] }>)
      .__OPEN_POWERZ_MOCK__;
    return mock ? mock.firmwareFlashLog().length : 0;
  });
}

async function chooseFile(page: Page): Promise<void> {
  await page.getByRole("tab", { name: "Firmware" }).click();
  await panel(page)
    .getByRole("button", { name: /choose file/i })
    .click();
  await expect(panel(page).getByRole("region", { name: "Firmware file" })).toBeVisible();
}

async function requestStart(page: Page): Promise<void> {
  await panel(page).getByRole("button", { name: "Update", exact: true }).click();
  await confirmDialog(page)
    .getByRole("textbox", { name: /version/i })
    .fill("2.0.7");
  await confirmDialog(page).getByRole("button", { name: "Start update" }).click();
}

test.describe("firmware fixes (FR 4PJRVJ)", () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 1600, height: 900 });
    await useAdvancedFirmware(page);
  });

  test("the update can be cancelled before the erase (AC-4PJRVJ.1)", async ({ page }) => {
    await open(page);
    await streamDemo(page);
    await chooseFile(page);
    await requestStart(page);
    await expect(cancelButton(page)).toBeEnabled();
    await cancelButton(page).click();
    await expect(panel(page).getByText(/stopped|cancelled/i)).toBeVisible();
    await expect(panel(page).getByRole("listitem").filter({ hasText: "Erasing" })).toHaveAttribute(
      "data-state",
      "pending",
    );
  });

  test("Cancel is disabled with a reason once erasing began (AC-4PJRVJ.1)", async ({ page }) => {
    await open(page);
    await streamDemo(page);
    await chooseFile(page);
    await requestStart(page);
    const erasing = panel(page).getByRole("listitem").filter({ hasText: "Erasing" });
    await expect(erasing).toHaveAttribute("data-state", "current");
    await expect(cancelButton(page)).toBeDisabled();
    await expect(cancelButton(page)).toHaveAccessibleDescription(/eras/i);
  });

  test("a running recording is not ended without asking (AC-4PJRVJ.2)", async ({ page }) => {
    await open(page);
    await streamDemo(page);
    await page
      .getByRole("button", { name: /^(start )?record/i })
      .first()
      .click();
    await chooseFile(page);
    await requestStart(page);

    const ask = page.getByRole("dialog", { name: /recording/i });
    await expect(ask).toBeVisible();
    expect(await flashCount(page)).toBe(0);

    await ask.getByRole("button", { name: "Keep recording" }).click();
    await expect(ask).toHaveCount(0);
    expect(await flashCount(page)).toBe(0);

    await confirmDialog(page).getByRole("button", { name: "Start update" }).click();
    await ask.getByRole("button", { name: /end recording/i }).click();
    await expect.poll(() => flashCount(page)).toBe(1);
  });

  test("a reload shows the running update's stage (AC-4PJRVJ.3)", async ({ page }) => {
    await page.goto("/?mock=firmware-running");
    await expect(page.getByLabel("Device", { exact: true })).toBeDisabled();
    await expect(panel(page).getByRole("region", { name: "Update progress" })).toBeVisible();
    await expect(panel(page).getByRole("listitem").filter({ hasText: "Writing" })).toHaveAttribute(
      "data-state",
      "current",
    );
  });

  test("the firmware text lands in a live region already mounted (AC-4PJRVJ.6)", async ({
    page,
  }) => {
    await page.addInitScript(() => {
      const seen: { insideLive: boolean; bringsItsRegion: boolean }[] = [];
      (window as unknown as { __noticeMutations: typeof seen }).__noticeMutations = seen;
      new MutationObserver((records) => {
        for (const r of records) {
          for (const node of r.addedNodes) {
            if (!node.textContent?.includes("Firmware warning")) continue;
            const el = node instanceof Element ? node : null;
            seen.push({
              insideLive: (r.target as Element).closest?.("[aria-live]") != null,
              bringsItsRegion:
                el !== null &&
                (el.matches("[aria-live]") || el.querySelector("[aria-live]") !== null),
            });
          }
        }
      }).observe(document, { childList: true, subtree: true });
    });
    await open(page);
    await streamDemo(page);
    await expect(page.getByRole("status").filter({ hasText: "2.0.7" })).toBeVisible();
    const seen = await page.evaluate(
      () => (window as unknown as { __noticeMutations: unknown[] }).__noticeMutations,
    );
    expect(seen.length).toBeGreaterThan(0);
    for (const mutation of seen) {
      expect(mutation).toEqual({ insideLive: true, bringsItsRegion: false });
    }
  });

  test("numbered release notes show no bullets as well (AC-4PJRVJ.8)", async ({ page }) => {
    await open(page);
    await chooseFile(page);
    const item = panel(page).getByRole("region", { name: "Firmware file" }).getByRole("listitem");
    await expect(item).toHaveCount(2);
    const marker = await item.first().evaluate((li) => getComputedStyle(li).listStyleType);
    expect(await item.first().innerText()).toMatch(/^\d/);
    expect(marker).toBe("none");
  });
});
