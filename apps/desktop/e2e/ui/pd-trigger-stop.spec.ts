/**
 * FR JQ971R (AC .2, .3, .4): pressing Detect in the Trigger tab stops a running PD
 * capture; back in the PD analyzer, its status area says why, the captured rows stay,
 * and the notice goes away when a capture starts again. A stop the user asked for
 * shows no notice. On the mocked backend, run on WebKit (the macOS app's engine).
 */

import { expect, test, type Locator, type Page } from "@playwright/test";
import {
  capture,
  pdView,
  rowCount,
  showPd,
  startButton,
  stopButton,
  stopCapture,
  openPd,
} from "./pd";

test.use({ viewport: { width: 1600, height: 900 } });

const NOTICE =
  "The PD capture stopped when Detect opened a trigger session. Start it again once you are done in the Trigger tab.";

function notice(page: Page): Locator {
  return pdView(page).getByText(NOTICE, { exact: true });
}

async function detectInTriggerTab(page: Page): Promise<void> {
  await page.getByRole("tab", { name: "Trigger" }).click();
  const view = page.locator("#view-trigger");
  await view.getByRole("button", { name: "Detect", exact: true }).click();
  await expect(view.getByRole("region", { name: "Detection" })).toBeVisible();
}

test("Detect stopping the capture shows the notice, keeps the rows, and Start removes it (AC-JQ971R.2, .3, .4)", async ({
  page,
}) => {
  await openPd(page);
  await capture(page);
  await expect(notice(page)).toHaveCount(0);

  await detectInTriggerTab(page);
  await showPd(page);

  await expect(startButton(page)).toBeEnabled();
  await expect(notice(page)).toBeVisible();
  expect(await rowCount(page)).toBeGreaterThan(0);

  await startButton(page).click();
  await expect(stopButton(page)).toBeEnabled();
  await expect(notice(page)).toHaveCount(0);
});

test("a stop the user asked for shows no notice (AC-JQ971R.3)", async ({ page }) => {
  await openPd(page);
  await capture(page);
  await stopCapture(page);
  await expect(notice(page)).toHaveCount(0);
});

test("the notice goes away when the meter disconnects (AC-JQ971R.3)", async ({ page }) => {
  await openPd(page);
  await capture(page);
  await detectInTriggerTab(page);
  await showPd(page);
  await expect(notice(page)).toBeVisible();

  await page.getByRole("button", { name: "Disconnect", exact: true }).first().click();
  await expect(notice(page)).toHaveCount(0);
});
