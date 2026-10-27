/**
 * FR 1HHT03, several meters at once, on the mocked backend with two simulated
 * meters (AC-1HHT03.6). The mock gives the second demo meter a serial of its own.
 *
 * - AC-1HHT03.1: with one meter streaming, "Add meter" connects a second one; both
 *   stream (the mock reports both device ids on `connectedIds()`);
 * - AC-1HHT03.2: the "Meters" region shows one tile (an `article`) per meter, side by
 *   side on a wide window and stacked on a narrow one, each with its name, serial
 *   suffix and live readout; "Expand" opens one meter in the full single-meter view,
 *   "All meters" goes back;
 * - AC-1HHT03.3 / .4: a tile's Disconnect ends only its own meter; the other keeps
 *   streaming, back in the full single-meter view as before.
 */

import { expect, type Locator, type Page, test } from "@playwright/test";
import { MOCK_REPLUG } from "../../src/lib/mockBackend";
import { badge, DEMO_LOAD, expectReadoutNear, open, reloadApp, streamDemo } from "./helpers";

const DEMO_PD = "demo:pd-9v-2a";
const MOCK_HOOK = "__OPEN_POWERZ_MOCK__";

test.use({ viewport: { width: 1600, height: 900 } });

function grid(page: Page): Locator {
  return page.getByRole("region", { name: "Meters" });
}

function tiles(page: Page): Locator {
  return grid(page).getByRole("article");
}

function tileOf(page: Page, name: string): Locator {
  return tiles(page).filter({ hasText: name });
}

/** The number a tile's readout shows for `name` (`VBUS`, `IBUS`, …). */
async function tileValue(tile: Locator, name: string): Promise<number> {
  const text = await tile.getByLabel(name, { exact: true }).locator(".digits").innerText();
  return Number.parseFloat(text.replace(/−/g, "-").replace(/[^\d.+-]/g, ""));
}

async function expectTileNear(tile: Locator, name: string, target: number): Promise<void> {
  await expect
    .poll(async () => Math.abs((await tileValue(tile, name)) - target) / target, {
      message: `${name} near ${target}`,
      timeout: 5_000,
    })
    .toBeLessThanOrEqual(0.05);
}

function connectedIds(page: Page): Promise<string[]> {
  return page.evaluate(
    (key) =>
      (window as unknown as Record<string, { connectedIds(): string[] }>)[key].connectedIds(),
    MOCK_HOOK,
  );
}

/** With the 5 V demo meter streaming, add the 9 V demo meter as a second one. */
async function addSecondMeter(page: Page): Promise<void> {
  await page.getByRole("button", { name: "Add meter" }).click();
  const dialog = page.getByRole("dialog", { name: "Add a meter" });
  await expect(dialog).toBeVisible();
  // The meter already connected is not offered again.
  await expect(
    dialog.getByLabel("Device", { exact: true }).locator(`option[value="${DEMO_LOAD}"]`),
  ).toHaveCount(0);
  await dialog.getByLabel("Device", { exact: true }).selectOption(DEMO_PD);
  await dialog.getByRole("button", { name: "Connect" }).click();
  await expect(dialog).toBeHidden();
}

async function twoMeters(page: Page): Promise<void> {
  await open(page);
  await streamDemo(page);
  // One meter: the full single-meter view as before, no grid.
  await expect(grid(page)).toHaveCount(0);
  await addSecondMeter(page);
  await expect(tiles(page)).toHaveCount(2);
}

test("a second meter joins the first, both stream in a two-column grid (AC-1HHT03.1, AC-1HHT03.2, AC-1HHT03.6)", async ({
  page,
}) => {
  await twoMeters(page);
  await expect.poll(() => connectedIds(page)).toEqual([DEMO_LOAD, DEMO_PD]);

  const load = tileOf(page, "load-5v-1a");
  const pd = tileOf(page, "pd-9v-2a");
  await expect(load).toHaveCount(1);
  await expect(pd).toHaveCount(1);
  // Name and serial suffix: the second simulated meter reports a serial of its own.
  await expect(load).toContainText("…3456");
  const pdText = await pd.innerText();
  const pdSuffix = /…\S{1,4}/.exec(pdText)?.[0];
  expect(pdSuffix, `the 9 V tile shows a serial suffix: ${pdText}`).toBeTruthy();
  expect(pdSuffix).not.toBe("…3456");

  // Each tile's live readout follows its own meter.
  await expectTileNear(load, "VBUS", 5);
  await expectTileNear(pd, "VBUS", 9);
  const before = await tileValue(pd, "VBUS");
  await expect.poll(() => tileValue(pd, "VBUS"), { timeout: 5_000 }).not.toBe(before);

  // Wide window: side by side.
  const a = await load.boundingBox();
  const b = await pd.boundingBox();
  expect(a && b, "both tiles are laid out").toBeTruthy();
  expect(Math.abs(a!.y - b!.y)).toBeLessThan(4);
  expect(Math.abs(a!.x - b!.x)).toBeGreaterThan(a!.width / 2);
});

test("on a narrow window the tiles stack (AC-1HHT03.2)", async ({ page }) => {
  await twoMeters(page);
  await page.setViewportSize({ width: 900, height: 1200 });
  const a = await tileOf(page, "load-5v-1a").boundingBox();
  const b = await tileOf(page, "pd-9v-2a").boundingBox();
  expect(a && b, "both tiles are laid out").toBeTruthy();
  expect(Math.abs(a!.x - b!.x)).toBeLessThan(4);
  expect(b!.y).toBeGreaterThan(a!.y + a!.height / 2);
});

test("one tile expands to the full single-meter view and back (AC-1HHT03.2)", async ({ page }) => {
  await twoMeters(page);
  await tileOf(page, "pd-9v-2a").getByRole("button", { name: "Expand" }).click();
  await expect(grid(page)).toBeHidden();
  // The full view shows the expanded meter, the 9 V one.
  await expectReadoutNear(page, "VBUS", 9, 0.05);
  await expect(page.getByRole("region", { name: "Live readout" })).toBeVisible();
  await page.getByRole("button", { name: "All meters" }).click();
  await expect(tiles(page)).toHaveCount(2);
});

test("disconnecting one tile leaves the other meter streaming (AC-1HHT03.3, AC-1HHT03.4)", async ({
  page,
}) => {
  await twoMeters(page);
  await tileOf(page, "load-5v-1a").getByRole("button", { name: "Disconnect" }).click();
  await expect.poll(() => connectedIds(page)).toEqual([DEMO_PD]);

  // Back to one meter: the full single-meter view of the one left, still streaming.
  await expect(grid(page)).toHaveCount(0);
  await expect(badge(page)).toHaveText("Streaming");
  await expectReadoutNear(page, "VBUS", 9, 0.05);
});

test("disconnecting the meter added last leaves the first one working (AC-1HHT03.3, AC-1HHT03.4)", async ({
  page,
}) => {
  await twoMeters(page);
  await tileOf(page, "pd-9v-2a").getByRole("button", { name: "Disconnect" }).click();
  await expect.poll(() => connectedIds(page)).toEqual([DEMO_LOAD]);

  await expect(grid(page)).toHaveCount(0);
  await expect(badge(page)).toHaveText("Streaming");
  await expectReadoutNear(page, "VBUS", 5, 0.05);
  // Actions still reach the meter that is left.
  await page.getByRole("button", { name: "Record", exact: true }).click();
  await expect
    .poll(async () => (await page.getByRole("timer").innerText()).trim())
    .toMatch(/[1-9][\d,]* rows/);
  await page.getByRole("button", { name: "Stop", exact: true }).click();
});

// ---- Review findings: an unplugged tile, and a reload with two meters ----

/** The status line of a tile: its meter's connection state. */
function tileState(tile: Locator): Locator {
  return tile.getByRole("status");
}

/** With the 5 V demo meter streaming, add the `?mock=replug` real meter beside it. */
async function addReplugMeter(page: Page): Promise<void> {
  await page.getByRole("button", { name: "Add meter" }).click();
  const dialog = page.getByRole("dialog", { name: "Add a meter" });
  const option = dialog
    .getByLabel("Device", { exact: true })
    .locator("option", { hasText: MOCK_REPLUG.serial });
  await dialog
    .getByLabel("Device", { exact: true })
    .selectOption((await option.getAttribute("value")) ?? "");
  await dialog.getByRole("button", { name: "Connect" }).click();
  await expect(tiles(page)).toHaveCount(2);
}

test("an unplugged meter's tile stays, shows the wait and streams again on the replug (AC-1HHT03.4, AC-TGE839.2)", async ({
  page,
}) => {
  await open(page, "?mock=replug");
  await streamDemo(page);
  await addReplugMeter(page);
  const meterTile = tiles(page).filter({ hasText: `…${MOCK_REPLUG.serial.slice(-4)}` });
  await expect(tileState(meterTile)).toHaveText("Streaming");

  // Unplugged: the tile stays and says it waits; the first meter streams on.
  await expect(tileState(meterTile)).toHaveText(/waiting/i, {
    timeout: MOCK_REPLUG.beforeMs + 5_000,
  });
  await expect(tiles(page)).toHaveCount(2);
  await expect(badge(page)).toHaveText("Streaming");

  // Back: the same tile streams again on its own.
  await expect(tileState(meterTile)).toHaveText("Streaming", {
    timeout: MOCK_REPLUG.awayMs + 5_000,
  });
  await expectTileNear(meterTile, "VBUS", 5);
  const before = await tileValue(meterTile, "IBUS");
  await expect.poll(() => tileValue(meterTile, "IBUS"), { timeout: 5_000 }).not.toBe(before);
  await expect(tiles(page)).toHaveCount(2);
});

test("Disconnect on an unplugged meter's tile ends its wait and takes the tile away (AC-1HHT03.3, AC-TGE839.5)", async ({
  page,
}) => {
  await open(page, "?mock=replug");
  await streamDemo(page);
  await addReplugMeter(page);
  const meterTile = tiles(page).filter({ hasText: `…${MOCK_REPLUG.serial.slice(-4)}` });
  await expect(tileState(meterTile)).toHaveText(/waiting/i, {
    timeout: MOCK_REPLUG.beforeMs + 5_000,
  });
  await meterTile.getByRole("button", { name: "Disconnect" }).click();
  // Back to the single-meter view of the meter left; the unplugged one never comes back.
  await expect(grid(page)).toHaveCount(0);
  await expect(badge(page)).toHaveText("Streaming");
  await page.waitForTimeout(MOCK_REPLUG.awayMs + 1_000);
  expect(await connectedIds(page)).toEqual([DEMO_LOAD]);
  await expect(grid(page)).toHaveCount(0);
  await expect(page.getByRole("alert")).toHaveCount(0);
});

test("a reload keeps every meter streaming in its tile (FR VEDDFD, AC-1HHT03.1)", async ({
  page,
}) => {
  await twoMeters(page);
  await reloadApp(page);

  await expect(tiles(page)).toHaveCount(2);
  await expect.poll(() => connectedIds(page)).toEqual([DEMO_LOAD, DEMO_PD]);
  const load = tileOf(page, "load-5v-1a");
  const pd = tileOf(page, "pd-9v-2a");
  await expectTileNear(load, "VBUS", 5);
  await expectTileNear(pd, "VBUS", 9);
  // Both readouts keep moving: each meter's events reach the new page.
  for (const tile of [load, pd]) {
    const before = await tileValue(tile, "VBUS");
    await expect.poll(() => tileValue(tile, "VBUS"), { timeout: 5_000 }).not.toBe(before);
  }
  // A tile's Disconnect still reaches only its meter.
  await pd.getByRole("button", { name: "Disconnect" }).click();
  await expect.poll(() => connectedIds(page)).toEqual([DEMO_LOAD]);
  await expect(grid(page)).toHaveCount(0);
  await expect(badge(page)).toHaveText("Streaming");
  await expectReadoutNear(page, "VBUS", 5, 0.05);
});
