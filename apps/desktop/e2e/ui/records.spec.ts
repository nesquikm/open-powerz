/**
 * FR 1HHT00 (AC .5, .6): a "Records" tab lists the demo meter's offline records
 * with their decoded columns, downloads one or all of them with progress, and
 * opens a downloaded record in its own chart with its statistics, while the
 * live session goes on.
 *
 * The mock serves the records (`recordsList()`) and counts downloads on
 * `window.__OPEN_POWERZ_MOCK__.recordDownloads()`.
 */

import { expect, test, type Page } from "@playwright/test";
import type { OfflineRecordEntry } from "../../src/lib/backend";
import { open, streamDemo } from "./helpers";

const MOCK_HOOK = "__OPEN_POWERZ_MOCK__";

interface RecordsMock {
  recordsList(): Promise<{ records: OfflineRecordEntry[] }>;
  recordDownloads(): number;
}

function mockRecords(page: Page): Promise<OfflineRecordEntry[]> {
  return page.evaluate(
    async (key) =>
      (await (window as unknown as Record<string, RecordsMock>)[key].recordsList()).records,
    MOCK_HOOK,
  );
}

function downloads(page: Page): Promise<number> {
  return page.evaluate(
    (key) => (window as unknown as Record<string, RecordsMock>)[key].recordDownloads(),
    MOCK_HOOK,
  );
}

function recordsTab(page: Page) {
  return page.getByRole("tab", { name: "Records" });
}

function list(page: Page) {
  return page.getByRole("region", { name: "Offline records" });
}

function row(page: Page, name: string) {
  return list(page).getByRole("row").filter({ hasText: name });
}

async function openRecords(page: Page): Promise<OfflineRecordEntry[]> {
  await open(page);
  await streamDemo(page);
  const records = await mockRecords(page);
  await recordsTab(page).click();
  await expect(page.getByRole("tabpanel", { name: "Records" })).toBeVisible();
  return records;
}

test.describe("offline records (FR 1HHT00)", () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 1600, height: 900 });
  });

  test("lists every record with its decoded columns (AC-1HHT00.5, .6)", async ({ page }) => {
    const records = await openRecords(page);
    expect(records.length).toBeGreaterThanOrEqual(3);
    const l = list(page);
    for (const header of ["Name", "Interval", "Points", "Duration", "Charge", "Energy"]) {
      await expect(l.getByRole("columnheader", { name: new RegExp(header, "i") })).toBeVisible();
    }
    for (const r of records) {
      const tr = row(page, r.name);
      await expect(tr).toHaveCount(1);
      await expect(tr).toContainText(`${r.interval_s} s`);
      await expect(tr).toContainText(String(r.points));
      await expect(tr).toContainText("Ah");
      await expect(tr).toContainText("Wh");
    }
    await expect.poll(() => downloads(page)).toBe(0);
  });

  test("downloads one record (AC-1HHT00.5)", async ({ page }) => {
    const records = await openRecords(page);
    const first = records.find((r) => r.points > 0)!;
    await row(page, first.name)
      .getByRole("button", { name: /download/i })
      .click();
    await expect.poll(() => downloads(page)).toBe(1);
    await expect(row(page, first.name)).toContainText(/downloaded/i);
  });

  test("downloads all records with progress (AC-1HHT00.5)", async ({ page }) => {
    const records = await openRecords(page);
    const n = records.filter((r) => r.points > 0).length;
    await list(page)
      .getByRole("button", { name: /download all/i })
      .click();
    await expect.poll(() => downloads(page)).toBe(n);
    await expect(list(page)).toContainText(
      new RegExp(`${n}\\s*(of|/)\\s*${n}|downloaded ${n}`, "i"),
    );
    for (const r of records.filter((x) => x.points > 0)) {
      await expect(row(page, r.name)).toContainText(/downloaded/i);
    }
  });

  test("opens a downloaded record in a chart with its statistics (AC-1HHT00.5)", async ({
    page,
  }) => {
    const records = await openRecords(page);
    const first = records.find((r) => r.points > 0)!;
    await row(page, first.name).getByRole("button", { name: /open/i }).click();
    const view = page.getByRole("region", { name: "Offline record" });
    await expect(view).toBeVisible();
    await expect(view).toContainText(first.name);
    for (const label of ["Duration", "Charge", "Energy", "VBUS", "IBUS"]) {
      await expect(view).toContainText(label);
    }
    await expect(view.locator("canvas").first()).toBeVisible();

    // The live session's chart and readout are untouched: back on the Meter tab
    // the stream still runs.
    await page.getByRole("tab", { name: "Meter" }).click();
    await expect(page.getByRole("button", { name: "Disconnect" })).toBeVisible();
  });

  test("shows no records before connecting (AC-1HHT00.5)", async ({ page }) => {
    await open(page);
    await recordsTab(page).click();
    await expect(page.getByRole("button", { name: /^download$/i })).toHaveCount(0);
    await expect(list(page).getByRole("button", { name: /download all/i })).toHaveCount(0);
  });
});
