/**
 * FR TGE83E: PD capture save and load in the desktop PD analyzer, on the mocked
 * backend (AC .2, .6; the file format itself, the lossless round trip and the vendor
 * database import are checked against the real backend in `src-tauri/tests/`).
 *
 * The page contract these specs pin (on top of `pd.ts`):
 *
 * - The PD analyzer holds the buttons "Save capture", "Open capture" and
 *   "Import vendor log".
 * - "Save capture" saves the capture as a `.pdcap` file. Its result shows in the
 *   app's export notice (a `.notice`): the number of records and the file path, with
 *   the "Reveal in Finder" / "Show in folder" and "Copy path" actions. The mock has
 *   no save dialog: the proposed file name (`powerz-pd-YYYYMMDD-HHMMSS.pdcap`) is the
 *   path.
 * - "Open capture" opens a `.pdcap` file into the table, details and timeline. The
 *   mock has no open dialog: it opens the capture saved last in the page.
 * - "Import vendor log" imports a vendor PD log database read-only into the view; the
 *   mock imports a vendor log of its demo PD scenario. A notice names the import.
 * - Opening and importing need no meter.
 */

import { expect, test, type Page } from "@playwright/test";
import { open } from "./helpers";
import {
  capture,
  dataRows,
  details,
  messageTexts,
  openPd,
  pdView,
  rowCount,
  scrollToTop,
  showPd,
  stopCapture,
} from "./pd";

test.use({ viewport: { width: 1600, height: 900 } });

const MOCK_PATH = /powerz-pd-\d{8}-\d{6}\.pdcap/;
const REVEAL = /^(Reveal in Finder|Show in folder)$/;

function button(page: Page, name: string) {
  return pdView(page).getByRole("button", { name, exact: true });
}

/** Fail the test on any native browser dialog. */
function noNativeDialogs(page: Page): void {
  page.on("dialog", async (d) => {
    await d.dismiss();
    throw new Error(`a native ${d.type()} dialog opened: ${d.message()}`);
  });
}

async function clearCapture(page: Page): Promise<void> {
  await button(page, "Clear capture").click();
  await page.getByRole("alertdialog").getByRole("button", { name: "Clear", exact: true }).click();
  await expect.poll(() => rowCount(page)).toBe(0);
}

test("Save capture writes a .pdcap file and says so in the export notice (AC-TGE83E.1)", async ({
  page,
}) => {
  noNativeDialogs(page);
  await openPd(page);
  await capture(page);
  await stopCapture(page);
  // The file holds every record; the table hides GoodCRC unless asked, so count them all.
  await pdView(page).getByRole("checkbox", { name: "Show GoodCRC" }).check();
  await expect.poll(() => rowCount(page)).toBeGreaterThan(0);
  const n = await rowCount(page);

  await button(page, "Save capture").click();
  const notice = page.locator(".notice").filter({ hasText: MOCK_PATH });
  await expect(notice).toBeVisible();
  await expect(notice).toHaveText(new RegExp(`${n.toLocaleString("en-US")} records?`));
  await expect(notice.getByRole("button", { name: REVEAL })).toBeVisible();
  await expect(notice.getByRole("button", { name: "Copy path", exact: true })).toBeVisible();
});

test("Open capture brings a saved capture back into the table and details (AC-TGE83E.2)", async ({
  page,
}) => {
  noNativeDialogs(page);
  await openPd(page);
  await capture(page);
  await stopCapture(page);
  const n = await rowCount(page);
  await scrollToTop(page);
  const messages = await messageTexts(page);

  await button(page, "Save capture").click();
  await expect(page.locator(".notice").filter({ hasText: MOCK_PATH })).toBeVisible();

  await clearCapture(page);
  await button(page, "Open capture").click();
  await expect.poll(() => rowCount(page)).toBe(n);
  await scrollToTop(page);
  expect(await messageTexts(page)).toEqual(messages);

  // The details of an opened record are there too.
  await dataRows(page).first().click();
  await expect(details(page).locator("[data-byte]").first()).toBeVisible();
});

test("Open capture and Import vendor log work without a meter (AC-TGE83E.2, AC-TGE83E.6)", async ({
  page,
}) => {
  await open(page);
  await showPd(page);
  await expect(button(page, "Open capture")).toBeEnabled();
  await expect(button(page, "Import vendor log")).toBeEnabled();
});

test("Import vendor log fills the view with the decoded vendor records (AC-TGE83E.6)", async ({
  page,
}) => {
  noNativeDialogs(page);
  await open(page);
  await showPd(page);

  await button(page, "Import vendor log").click();
  await expect.poll(() => rowCount(page)).toBeGreaterThan(0);
  await scrollToTop(page);
  expect(await messageTexts(page)).toContain("Source_Capabilities");
  await expect(page.locator(".notice").filter({ hasText: /import/i })).toBeVisible();
});

test("Open capture and Import vendor log wait until the live capture stops (FR TGE83E)", async ({
  page,
}) => {
  noNativeDialogs(page);
  await openPd(page);
  await capture(page);
  // Rows of a running capture would be hidden behind an opened file.
  await expect(button(page, "Open capture")).toBeDisabled();
  await expect(button(page, "Import vendor log")).toBeDisabled();
  await expect(pdView(page).getByText("Stop the capture first", { exact: false })).toBeVisible();
  await expect(button(page, "Import vendor log")).toHaveAccessibleDescription(
    /stop the capture first/i,
  );

  await stopCapture(page);
  await expect(button(page, "Open capture")).toBeEnabled();
  await expect(button(page, "Import vendor log")).toBeEnabled();
  await expect(pdView(page).getByText("Stop the capture first", { exact: false })).toHaveCount(0);
});
