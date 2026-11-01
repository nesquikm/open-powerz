/**
 * FR AXHK2K, on the mocked backend: what connecting a meter does to an opened PD file
 * (AC-AXHK2K.5) and what a reload reads of a parked meter's capture (AC-AXHK2K.7).
 *
 * The page alone decides when an opened file goes: the backend holds it for exactly as
 * long as the page shows it.
 */

import { expect, type Page, test } from "@playwright/test";
import { badge, DEMO_LOAD, open, reloadApp, streamDemo } from "./helpers";
import {
  capture,
  dataRows,
  details,
  openPd,
  pdView,
  rowCount,
  showPd,
  startButton,
  stopButton,
  stopCapture,
} from "./pd";

const DEMO_PD = "demo:pd-9v-2a";

test.use({ viewport: { width: 1600, height: 900 } });

function tileOf(page: Page, name: string) {
  return page
    .getByRole("region", { name: "Meters" })
    .getByRole("article")
    .filter({ hasText: name });
}

async function addMeter(page: Page, id: string): Promise<void> {
  await page.getByRole("button", { name: "Add meter" }).click();
  const dialog = page.getByRole("dialog", { name: "Add a meter" });
  await dialog.getByLabel("Device", { exact: true }).selectOption(id);
  await dialog.getByRole("button", { name: "Connect" }).click();
  await expect(dialog).toBeHidden();
}

/** Capture on the demo meter, save it and open the file in the PD view. */
async function openSavedCapture(page: Page): Promise<void> {
  await openPd(page);
  await capture(page);
  await stopCapture(page);
  await pdView(page).getByRole("button", { name: "Save capture", exact: true }).click();
  await pdView(page).getByRole("button", { name: "Open capture", exact: true }).click();
  await expect(dataRows(page).first()).toBeVisible();
}

/** The backend still holds an opened file. */
function backendHoldsFile(page: Page): Promise<boolean> {
  return page.evaluate(() => {
    const mock = (
      window as unknown as {
        __OPEN_POWERZ_MOCK__: { pdDetails(index: number, opened: boolean): Promise<unknown> };
      }
    ).__OPEN_POWERZ_MOCK__;
    return mock.pdDetails(0, true).then(
      () => true,
      () => false,
    );
  });
}

test("an opened capture stays usable while a second meter connects (AC-AXHK2K.5)", async ({
  page,
}) => {
  await openSavedCapture(page);
  const rows = await rowCount(page);

  await addMeter(page, DEMO_PD);

  expect(await rowCount(page)).toBe(rows);
  await dataRows(page).first().click();
  await expect(details(page).getByRole("tree", { name: "Fields" })).toBeVisible();
  expect(await backendHoldsFile(page)).toBe(true);
});

test("a meter connecting into the full view keeps the opened file (AC-AXHK2K.5)", async ({
  page,
}) => {
  await openSavedCapture(page);
  const rows = await rowCount(page);
  await page.getByRole("button", { name: "Disconnect" }).first().click();
  await expect(badge(page)).not.toHaveText("Streaming");
  expect(await backendHoldsFile(page)).toBe(true);

  await streamDemo(page, DEMO_LOAD);

  await expect.poll(() => rowCount(page)).toBe(rows);
  expect(await backendHoldsFile(page)).toBe(true);
});

test("clearing an opened file sends the clear alone (AC-AXHK2K.5)", async ({ page }) => {
  await openSavedCapture(page);
  await page.evaluate(() => {
    const w = window as unknown as {
      __OPEN_POWERZ_MOCK__: { pdReleaseOpened(): Promise<void> };
      __releases: number;
    };
    const mock = w.__OPEN_POWERZ_MOCK__;
    const release = mock.pdReleaseOpened.bind(mock);
    w.__releases = 0;
    mock.pdReleaseOpened = () => {
      w.__releases++;
      return release();
    };
  });

  await page.getByRole("button", { name: "Clear capture" }).click();
  await page.getByRole("alertdialog").getByRole("button", { name: "Clear", exact: true }).click();
  await expect.poll(() => rowCount(page)).toBe(0);

  const releases = await page.evaluate(
    () => (window as unknown as { __releases: number }).__releases,
  );
  expect(releases).toBe(0);
  expect(await backendHoldsFile(page)).toBe(false);
});

test("a tile meter's PD capture is read from the backend after a reload (AC-AXHK2K.7)", async ({
  page,
}) => {
  await open(page);
  await streamDemo(page);
  await addMeter(page, DEMO_PD);
  const allMeters = page.getByRole("button", { name: "All meters" });
  const expand = (name: string) =>
    tileOf(page, name)
      .getByRole("button", { name: /Expand/ })
      .click();

  await expand("load-5v-1a");
  await showPd(page);
  await startButton(page).click();
  await expect(stopButton(page)).toBeEnabled();
  await page.getByRole("tab", { name: "Meter", exact: true }).click();
  await allMeters.click();
  await expand("pd-9v-2a");
  await allMeters.click();

  await reloadApp(page);

  await expand("load-5v-1a");
  await showPd(page);
  await expect(stopButton(page)).toBeEnabled();
  await expect(startButton(page)).toBeDisabled();
});
