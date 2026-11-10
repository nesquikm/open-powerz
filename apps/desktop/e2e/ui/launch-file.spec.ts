/**
 * FR M078V5 on the mocked backend: a recording handed to the app at launch opens straight
 * into the File tab, with no Open recording click and no dialog. `?launchfile=1` hands the
 * mock its demo recording at launch; `?recordingfile=refused` makes the reader refuse it.
 */

import { expect, test, type Locator, type Page } from "@playwright/test";
import { MOCK_RECORDING_FILE } from "../../src/lib/mockBackend";
import { open } from "./helpers";

test.use({ viewport: { width: 1600, height: 900 } });

function fileTab(page: Page): Locator {
  return page.getByRole("tab", { name: "File", exact: true });
}

function fileView(page: Page): Locator {
  return page.getByRole("region", { name: "Recording file" });
}

test("a launch file opens in the File tab at startup, with no click (AC-M078V5.1)", async ({
  page,
}) => {
  await open(page, "?launchfile=1");

  await expect(fileView(page)).toBeVisible({ timeout: 20_000 });
  await expect(fileTab(page)).toHaveAttribute("aria-selected", "true");
  await expect(fileView(page)).toContainText(MOCK_RECORDING_FILE.name);
  await expect(fileView(page)).toContainText(MOCK_RECORDING_FILE.rows.toLocaleString("en-US"));
});

test("with no launch file the app starts as usual, with no File tab (AC-M078V5.2)", async ({
  page,
}) => {
  await open(page);
  await expect(page.getByRole("tab", { name: "Meter", exact: true })).toBeVisible();
  await expect(fileTab(page)).toHaveCount(0);
  await expect(page.getByRole("alert")).toHaveCount(0);
});

test("a refused launch file shows Open recording's error and opens no tab (AC-M078V5.4)", async ({
  page,
}) => {
  await open(page, "?launchfile=1&recordingfile=refused");

  await expect(page.getByRole("alert")).toContainText(/header/i, { timeout: 20_000 });
  await expect(fileTab(page)).toHaveCount(0);
});
