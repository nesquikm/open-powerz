/**
 * FR TGE839, replug reconnects to the meter, on the mocked backend.
 *
 * `?mock=replug` lists one real meter (serial `MOCK_REPLUG.serial`) next to the
 * demo entries. A session on it streams for `MOCK_REPLUG.beforeMs`, then the
 * meter is unplugged: the session ends with an unplug error, the backend says it
 * waits for that serial, and the device list loses the meter. After
 * `MOCK_REPLUG.awayMs` the OS lists the meter again (at a new address) and the
 * backend reconnects on its own: `connected`, the outage gap, then samples.
 *
 * - AC-TGE839.1: while away, the picker keeps the meter and the badge says waiting;
 *   no demo device is picked or connected on its own;
 * - AC-TGE839.2: the meter streams again without user action;
 * - AC-TGE839.3: the gap shows as a "No data" band, live and in Whole session;
 * - AC-TGE839.5: a demo picked by hand while waiting connects and ends the wait;
 * - AC-TGE839.6: a running recording ends at the unplug with a disconnect notice.
 */

import { expect, type Page, test } from "@playwright/test";
import { MOCK_REPLUG } from "../../src/lib/mockBackend";
import { badge, chartGapBands, DEMO_LOAD, open } from "./helpers";

test.use({ viewport: { width: 1600, height: 900 } });

function picker(page: Page) {
  return page.getByLabel("Device", { exact: true });
}

/** The picked entry: its value and its visible text. */
async function picked(page: Page): Promise<{ value: string; text: string }> {
  return picker(page).evaluate((el: HTMLSelectElement) => ({
    value: el.value,
    text: el.selectedOptions[0]?.text.trim() ?? "",
  }));
}

/** Pick the mock's real meter and connect; wait until it streams. */
async function streamMeter(page: Page): Promise<void> {
  const option = picker(page).locator("option", { hasText: MOCK_REPLUG.serial });
  await expect(option).toHaveCount(1);
  const value = await option.getAttribute("value");
  expect(value, "the meter's picker entry").not.toMatch(/^demo:/);
  await picker(page).selectOption(value ?? "");
  await page.getByRole("button", { name: "Connect" }).click();
  await expect(badge(page)).toHaveText("Streaming");
}

async function waitForUnplug(page: Page): Promise<void> {
  await expect(badge(page)).toHaveText(/waiting/i, { timeout: MOCK_REPLUG.beforeMs + 5_000 });
}

test("an unplugged meter stays picked, the app waits, then reconnects with a No data band (AC-TGE839.1, AC-TGE839.2, AC-TGE839.3)", async ({
  page,
}) => {
  await open(page, "?mock=replug");
  await streamMeter(page);
  await waitForUnplug(page);

  // Waiting: the meter stays the picked entry, never a demo.
  const away = await picked(page);
  expect(away.text).toContain(MOCK_REPLUG.serial);
  expect(away.value).not.toMatch(/^demo:/);
  await expect(picker(page)).toBeEnabled();

  // The OS lists it again: it streams on its own, still picked.
  await expect(badge(page)).toHaveText("Streaming", { timeout: MOCK_REPLUG.awayMs + 3_000 });
  const back = await picked(page);
  expect(back.text).toContain(MOCK_REPLUG.serial);
  expect(back.value).not.toMatch(/^demo:/);

  // The time the meter was away is a No data band in the live chart.
  await expect
    .poll(async () => (await chartGapBands(page)).map((b) => b.label), { timeout: 10_000 })
    .toContainEqual(expect.stringMatching(/^No data/));

  // And in Whole session.
  await page.getByRole("button", { name: "Whole session" }).click();
  await expect
    .poll(async () => (await chartGapBands(page)).length, { timeout: 10_000 })
    .toBeGreaterThanOrEqual(1);
  const bands = await chartGapBands(page);
  expect(bands.some((b) => b.label.startsWith("No data"))).toBe(true);
});

test("a demo picked by hand while the meter is away connects and ends the wait (AC-TGE839.5)", async ({
  page,
}) => {
  await open(page, "?mock=replug");
  await streamMeter(page);
  await waitForUnplug(page);

  await picker(page).selectOption(DEMO_LOAD);
  await expect(badge(page)).not.toHaveText(/waiting/i);
  await page.getByRole("button", { name: "Connect" }).click();
  await expect(badge(page)).toHaveText("Streaming");

  // The meter comes back (listed at its new address), but the demo session stays.
  await expect(picker(page).locator('option[value="usb:1:9"]')).toBeAttached({
    timeout: MOCK_REPLUG.awayMs + 5_000,
  });
  expect((await picked(page)).value).toBe(DEMO_LOAD);
  await expect(badge(page)).toHaveText("Streaming");
});

test("a recording running at the unplug ends with a disconnect notice (AC-TGE839.6)", async ({
  page,
}) => {
  await open(page, "?mock=replug");
  await streamMeter(page);
  await page.getByRole("button", { name: "Record", exact: true }).click();
  await expect(page.getByRole("button", { name: "Stop", exact: true })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await waitForUnplug(page);

  const notice = page
    .locator(".notice-stack .notice")
    .filter({ hasText: /disconnected|unplugged/i });
  await expect(notice).toBeVisible();
  await expect(page.getByRole("button", { name: "Record", exact: true })).toBeVisible();

  // The reconnect does not restart the recording.
  await expect(badge(page)).toHaveText("Streaming", { timeout: MOCK_REPLUG.awayMs + 3_000 });
  await expect(page.getByRole("button", { name: "Stop", exact: true })).toHaveCount(0);
});

test("the unplug banner sits below the view tabs, and leaves once the meter streams again (HIL 1HHT04.multi)", async ({
  page,
}) => {
  await open(page, "?mock=replug");
  await streamMeter(page);
  await waitForUnplug(page);
  const banner = page.locator(".notice.error").first();
  await expect(banner).toContainText("The meter was unplugged.");
  const tabs = (await page.getByRole("tab", { name: "Meter" }).boundingBox())!;
  const box = (await banner.boundingBox())!;
  expect(box.y, "the banner's top under the tab strip").toBeGreaterThanOrEqual(
    tabs.y + tabs.height,
  );
  await expect(badge(page)).toHaveText("Streaming", { timeout: MOCK_REPLUG.awayMs + 3_000 });
  await expect(banner).toHaveCount(0);
});
