/**
 * FR AXHK2R on the mocked backend: pause, auto-stop, the row limit, the prefix and folder,
 * and the memory depth. The demo meter draws a steady 1 A at 5 V, so an auto-stop set
 * below 2 A fires once its wait is over.
 */

import { expect, test, type Locator, type Page } from "@playwright/test";
import { DEFAULT_RECORDER_SETTINGS, RECORDER_SETTINGS_STORAGE_KEY } from "../../src/lib/settings";
import type { RecorderSettings } from "../../src/lib/settings";
import { open, reloadApp, statsElapsedSeconds, streamDemo } from "./helpers";

const DEMO_PD = "demo:pd-9v-2a";
const OPTIONS = "Recorder options";

test.use({ viewport: { width: 1600, height: 900 } });

function button(scope: Page | Locator, name: string): Locator {
  return scope.getByRole("button", { name, exact: true });
}

function optionsDialog(page: Page): Locator {
  return page.getByRole("dialog", { name: OPTIONS });
}

async function openOptions(page: Page): Promise<Locator> {
  await button(page, OPTIONS).click();
  const dialog = optionsDialog(page);
  await expect(dialog).toBeVisible();
  return dialog;
}

async function save(page: Page): Promise<void> {
  await button(optionsDialog(page), "Save").click();
  await expect(optionsDialog(page)).toBeHidden();
}

/** Start the app with these recorder settings saved, as a returning user has them. */
async function useSettings(page: Page, changes: Partial<RecorderSettings>): Promise<void> {
  const value = JSON.stringify({ ...DEFAULT_RECORDER_SETTINGS, ...changes });
  await page.addInitScript(
    ([key, saved]) => localStorage.setItem(key as string, saved as string),
    [RECORDER_SETTINGS_STORAGE_KEY, value],
  );
}

/** The row count of the top bar's recording indicator. */
async function shownRows(page: Page): Promise<number> {
  const text = await page.getByRole("timer").innerText();
  return Number((/([\d,]+) rows?/.exec(text)?.[1] ?? "0").replace(/,/g, ""));
}

async function recordUntilRows(page: Page): Promise<void> {
  await button(page, "Record").click();
  await expect.poll(() => shownRows(page)).toBeGreaterThan(0);
}

test("Pause holds the rows while the meter goes on, and Resume continues (AC-AXHK2R.1)", async ({
  page,
}) => {
  await open(page);
  await streamDemo(page);
  await recordUntilRows(page);

  await button(page, "Pause").click();
  await expect(button(page, "Resume")).toBeVisible();
  // Let the progress in flight land before reading the count.
  await page.waitForTimeout(700);
  const rows = await shownRows(page);
  const elapsed = await statsElapsedSeconds(page);
  await page.waitForTimeout(1_500);
  expect(await shownRows(page)).toBe(rows);
  expect(await statsElapsedSeconds(page)).toBeGreaterThan(elapsed);

  await button(page, "Resume").click();
  await expect(button(page, "Pause")).toBeVisible();
  await expect.poll(() => shownRows(page)).toBeGreaterThan(rows);

  await button(page, "Stop").click();
  await expect(page.getByText(/Recording saved:.*\b1 gap\b/)).toBeVisible();
  await expect(button(page, "Pause")).toHaveCount(0);
});

test("a reload keeps the pause (AC-AXHK2R.1)", async ({ page }) => {
  await open(page);
  await streamDemo(page);
  await recordUntilRows(page);
  await button(page, "Pause").click();
  await expect(button(page, "Resume")).toBeVisible();
  await reloadApp(page);
  await expect(button(page, "Resume")).toBeVisible();
});

test("auto-stop ends the recording and says why (AC-AXHK2R.3)", async ({ page }) => {
  await useSettings(page, { autoStop: true, belowA: 2, forSeconds: 1 });
  await open(page);
  await streamDemo(page);
  await button(page, "Record").click();
  await expect(button(page, "Stop")).toBeVisible();
  await expect(page.getByText(/low current/i)).toBeVisible({ timeout: 5_000 });
  await expect(button(page, "Record")).toBeVisible();
  await expect(page.locator(".badge")).toHaveText("Streaming");
});

test("the row limit ends the recording and says why (AC-AXHK2R.5)", async ({ page }) => {
  await useSettings(page, { maxRows: 20 });
  await open(page);
  await streamDemo(page);
  await button(page, "Record").click();
  await expect(page.getByText(/row limit.*\b20 rows\b|\b20 rows\b.*row limit/i)).toBeVisible({
    timeout: 5_000,
  });
  await expect(button(page, "Record")).toBeVisible();
});

test("settings that could not work cannot be saved (AC-AXHK2R.3, AC-AXHK2R.6)", async ({
  page,
}) => {
  await open(page);
  await streamDemo(page);
  const dialog = await openOptions(page);
  const saveButton = button(dialog, "Save");
  await dialog.getByRole("checkbox", { name: /auto-stop/i }).check();
  const below = dialog.getByLabel(/below/i);
  const wait = dialog.getByLabel(/seconds/i);
  await wait.fill("5");

  await below.fill("0");
  await expect(saveButton).toBeDisabled();
  await expect(dialog.getByRole("alert")).toBeVisible();
  await below.fill("");
  await expect(saveButton).toBeDisabled();
  await below.fill("0.1");
  await expect(saveButton).toBeEnabled();

  const prefix = dialog.getByLabel(/prefix/i);
  await prefix.fill("a/b");
  await expect(saveButton).toBeDisabled();
  await prefix.fill("bench");
  await expect(saveButton).toBeEnabled();
});

test("every setting persists (AC-AXHK2R.8)", async ({ page }) => {
  await open(page);
  await streamDemo(page);
  const dialog = await openOptions(page);
  await dialog.getByLabel(/prefix/i).fill("bench");
  await dialog.getByRole("checkbox", { name: /auto-stop/i }).check();
  await dialog.getByLabel(/below/i).fill("0.05");
  await dialog.getByLabel(/seconds/i).fill("30");
  await dialog.getByLabel(/row limit/i).fill("5000");
  await save(page);

  await page.reload();
  await open(page);
  await streamDemo(page);
  const again = await openOptions(page);
  await expect(again.getByLabel(/prefix/i)).toHaveValue("bench");
  await expect(again.getByRole("checkbox", { name: /auto-stop/i })).toBeChecked();
  await expect(again.getByLabel(/below/i)).toHaveValue("0.05");
  await expect(again.getByLabel(/seconds/i)).toHaveValue("30");
  await expect(again.getByLabel(/row limit/i)).toHaveValue("5000");
});

test("the prefix and the folder set the file's name and place (AC-AXHK2R.6)", async ({ page }) => {
  await useSettings(page, { prefix: "bench", folder: "/data/runs" });
  await open(page);
  await streamDemo(page);
  await recordUntilRows(page);
  await button(page, "Stop").click();
  await expect(page.getByText(/\/data\/runs\/bench-\d{8}-\d{6}\.csv/)).toBeVisible();
});

test("the fields lock while a meter records (AC-AXHK2R.6)", async ({ page }) => {
  await open(page);
  await streamDemo(page);
  await recordUntilRows(page);
  const dialog = await openOptions(page);
  await expect(dialog.getByLabel(/prefix/i)).toBeDisabled();
  await expect(dialog.getByLabel(/row limit/i)).toBeDisabled();
});

test("the dialog says how long the chart's memory lasts, by rate (AC-AXHK2R.7)", async ({
  page,
}) => {
  await open(page);
  await streamDemo(page);
  await page.getByLabel("Sample rate").selectOption("1000");
  await expect(page.getByLabel("Sample rate")).toHaveValue("1000");
  const dialog = await openOptions(page);
  await expect(dialog).toContainText("10 minutes");
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();

  await page.getByLabel("Sample rate").selectOption("50");
  await expect(page.getByLabel("Sample rate")).toHaveValue("50");
  const again = await openOptions(page);
  await expect(again).not.toContainText("10 minutes");
  await expect(again).toContainText(/\b3(\.\d)? hours?\b/);
});

test("tile recordings pause and honour the settings (AC-AXHK2R.1, AC-AXHK2R.8)", async ({
  page,
}) => {
  await useSettings(page, { prefix: "bench" });
  await open(page);
  await streamDemo(page);
  await button(page, "Add meter").click();
  const adding = page.getByRole("dialog", { name: "Add a meter" });
  await adding.getByLabel("Device", { exact: true }).selectOption(DEMO_PD);
  await button(adding, "Connect").click();
  await expect(adding).toBeHidden();
  const tile = page
    .getByRole("region", { name: "Meters" })
    .getByRole("article")
    .filter({ hasText: "pd-9v-2a" });
  await expect(tile).toHaveCount(1);

  await button(tile, "Record").click();
  await button(tile, "Pause").click();
  await expect(button(tile, "Resume")).toBeVisible();
  await button(tile, "Resume").click();
  await expect(button(tile, "Pause")).toBeVisible();
  await button(tile, "Stop").click();
  await expect(page.getByText(/bench-\d{8}-\d{6}\.csv/)).toBeVisible();
});

test("a tile's options show that meter's rate (AC-AXHK2R.7)", async ({ page }) => {
  await open(page);
  await streamDemo(page);
  await button(page, "Add meter").click();
  const adding = page.getByRole("dialog", { name: "Add a meter" });
  await adding.getByLabel("Device", { exact: true }).selectOption(DEMO_PD);
  await button(adding, "Connect").click();
  await expect(adding).toBeHidden();
  const tile = page
    .getByRole("region", { name: "Meters" })
    .getByRole("article")
    .filter({ hasText: "pd-9v-2a" });
  await tile.getByLabel("Sample rate").selectOption("1000");

  await button(tile, OPTIONS).click();
  await expect(optionsDialog(page)).toContainText("10 minutes");
  await button(optionsDialog(page), "Cancel").click();
});

test("the dialog keeps the keyboard and gives focus back (review)", async ({ page }) => {
  await open(page);
  await streamDemo(page);
  const dialog = await openOptions(page);

  // From the dialog itself, as it opens, Shift+Tab stays inside.
  await page.keyboard.press("Shift+Tab");
  await expect(button(dialog, "Save")).toBeFocused();

  // A click on the backdrop drops focus to the page: keys still stop at the dialog.
  await page.mouse.click(5, 5);
  await page.keyboard.press("Space");
  await expect(button(page, "Record")).toBeVisible();
  await page.keyboard.press("Tab");
  await expect(dialog.locator(":focus")).toHaveCount(1);
  await page.mouse.click(5, 5);
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  await expect(button(page, OPTIONS)).toBeFocused();
});
