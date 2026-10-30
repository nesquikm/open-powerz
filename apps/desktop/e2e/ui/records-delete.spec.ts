/**
 * FR JKWT3V (AC .3, .4 and .6): the Records tab deletes one record or all of them
 * only after the record name (or "all") is typed in a confirmation dialog; the mock
 * backs the records up first and shows the backup folder. `?recordsfault=backup`
 * fails the backup (nothing is deleted); `?recordsfault=mismatch` keeps the old table
 * so the re-read does not match (the error names the backup).
 */

import { expect, test, type Page } from "@playwright/test";
import type { OfflineRecordEntry } from "../../src/lib/backend";
import { open, streamDemo } from "./helpers";

const MOCK_HOOK = "__OPEN_POWERZ_MOCK__";

interface RecordsMock {
  recordsList(): Promise<{ records: OfflineRecordEntry[] }>;
}

function mockRecords(page: Page): Promise<OfflineRecordEntry[]> {
  return page.evaluate(
    async (key) =>
      (await (window as unknown as Record<string, RecordsMock>)[key].recordsList()).records,
    MOCK_HOOK,
  );
}

function list(page: Page) {
  return page.getByRole("region", { name: "Offline records" });
}

function row(page: Page, name: string) {
  return list(page).getByRole("row").filter({ hasText: name });
}

function dialog(page: Page) {
  return page.getByRole("alertdialog", { name: /delete/i });
}

async function openRecords(page: Page, query = ""): Promise<OfflineRecordEntry[]> {
  await open(page, query);
  await streamDemo(page);
  const records = await mockRecords(page);
  await page.getByRole("tab", { name: "Records" }).click();
  await expect(page.getByRole("tabpanel", { name: "Records" })).toBeVisible();
  await expect(row(page, records[0]!.name)).toHaveCount(1);
  return records;
}

test.describe("delete offline records (FR JKWT3V)", () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 1600, height: 900 });
  });

  test("delete one asks for the typed name before it deletes (AC-JKWT3V.3)", async ({ page }) => {
    const records = await openRecords(page);
    const victim = records[1]!;
    await row(page, victim.name)
      .getByRole("button", { name: /^delete$/i })
      .click();
    const d = dialog(page);
    await expect(d).toBeVisible();
    await expect(d).toContainText(victim.name);
    await expect(d).toContainText(/backup/i);
    const confirm = d.getByRole("button", { name: /^delete$/i });
    await expect(confirm).toBeDisabled();
    const input = d.getByRole("textbox");
    await input.fill(victim.name.toLowerCase());
    await expect(confirm).toBeDisabled();
    await input.fill(records[0]!.name);
    await expect(confirm).toBeDisabled();
    await input.fill(victim.name);
    await expect(confirm).toBeEnabled();
    await confirm.click();
    await expect(d).toHaveCount(0);
    await expect(row(page, victim.name)).toHaveCount(0);
    for (const r of records.filter((x) => x.name !== victim.name)) {
      await expect(row(page, r.name)).toHaveCount(1);
    }
    await expect(list(page)).toContainText(/backup/i);
    expect((await mockRecords(page)).map((r) => r.name)).not.toContain(victim.name);
  });

  test("cancel and Escape close the dialog and delete nothing", async ({ page }) => {
    const records = await openRecords(page);
    const victim = records[0]!;
    await row(page, victim.name)
      .getByRole("button", { name: /^delete$/i })
      .click();
    await dialog(page)
      .getByRole("button", { name: /cancel/i })
      .click();
    await expect(dialog(page)).toHaveCount(0);
    await row(page, victim.name)
      .getByRole("button", { name: /^delete$/i })
      .click();
    await expect(dialog(page)).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(dialog(page)).toHaveCount(0);
    await expect(row(page, victim.name)).toHaveCount(1);
    expect(await mockRecords(page)).toHaveLength(records.length);
  });

  test("delete all needs the word all typed (AC-JKWT3V.3)", async ({ page }) => {
    const records = await openRecords(page);
    await list(page)
      .getByRole("button", { name: /delete all/i })
      .click();
    const d = dialog(page);
    await expect(d).toBeVisible();
    const confirm = d.getByRole("button", { name: /^delete( all)?$/i });
    await expect(confirm).toBeDisabled();
    await d.getByRole("textbox").fill(records[0]!.name);
    await expect(confirm).toBeDisabled();
    await d.getByRole("textbox").fill("all");
    await expect(confirm).toBeEnabled();
    await confirm.click();
    await expect(d).toHaveCount(0);
    for (const r of records) await expect(row(page, r.name)).toHaveCount(0);
    expect(await mockRecords(page)).toEqual([]);
  });

  test("a failed backup shows the error and deletes nothing (AC-JKWT3V.2)", async ({ page }) => {
    const records = await openRecords(page, "?recordsfault=backup");
    const victim = records[0]!;
    await row(page, victim.name)
      .getByRole("button", { name: /^delete$/i })
      .click();
    await dialog(page).getByRole("textbox").fill(victim.name);
    await dialog(page)
      .getByRole("button", { name: /^delete$/i })
      .click();
    await expect(page.getByRole("tabpanel", { name: "Records" })).toContainText(/backup/i);
    await expect(row(page, victim.name)).toHaveCount(1);
    expect(await mockRecords(page)).toHaveLength(records.length);
  });

  test("a mismatching re-read shows the error with the backup path (AC-JKWT3V.4)", async ({
    page,
  }) => {
    const records = await openRecords(page, "?recordsfault=mismatch");
    const victim = records[0]!;
    await row(page, victim.name)
      .getByRole("button", { name: /^delete$/i })
      .click();
    await dialog(page).getByRole("textbox").fill(victim.name);
    await dialog(page)
      .getByRole("button", { name: /^delete$/i })
      .click();
    const panel = page.getByRole("tabpanel", { name: "Records" });
    await expect(panel.getByRole("alert")).toContainText(/backup/i);
    await expect(panel.getByRole("alert")).toContainText("/");
  });
});
