/**
 * FR 4PJRVM, the multi-meter fixes on the mocked backend with two simulated meters.
 *
 * - AC-4PJRVM.1: Record, Export, rate and Disconnect sit in a control bar named after its
 *   meter: one per tile in the grid, one on the expanded meter in the full view, and they
 *   act on that meter only;
 * - AC-4PJRVM.3: the tile order stays the same across a page reload;
 * - AC-4PJRVM.4: PD capture keeps running when its meter leaves the full view, and another
 *   meter can be connected while one flashes firmware.
 */

import { expect, type Locator, type Page, test } from "@playwright/test";
import { open, reloadApp, streamDemo } from "./helpers";
import { grid } from "./pd";

const DEMO_PD = "demo:pd-9v-2a";
const LOAD = "load-5v-1a";
const PD = "pd-9v-2a";
const MOCK_HOOK = "__OPEN_POWERZ_MOCK__";
/** The meter `?mock=firmware-running` flashes. */
const FLASHING = "demo:idle";

test.use({ viewport: { width: 1600, height: 900 } });

function tiles(page: Page): Locator {
  return page.getByRole("region", { name: "Meters" }).getByRole("article");
}

function tileOf(page: Page, name: string): Locator {
  return tiles(page).filter({ hasText: name });
}

/** The control bar of the meter called `name`, in a tile or in the full view. */
function controls(page: Page, name: string): Locator {
  return page.getByRole("group", { name: new RegExp(`^Controls for .*${name}`) });
}

async function twoMeters(page: Page): Promise<void> {
  await open(page);
  await streamDemo(page);
  await page.getByRole("button", { name: "Add meter" }).click();
  const dialog = page.getByRole("dialog", { name: "Add a meter" });
  await dialog.getByLabel("Device", { exact: true }).selectOption(DEMO_PD);
  await dialog.getByRole("button", { name: "Connect" }).click();
  await expect(dialog).toBeHidden();
  await expect(tiles(page)).toHaveCount(2);
}

/** The meters the mock has a session for, read through its test hook. */
function connectedIds(page: Page): Promise<string[]> {
  return page.evaluate(
    (key) =>
      (window as unknown as Record<string, { connectedIds(): string[] }>)[key]!.connectedIds(),
    MOCK_HOOK,
  );
}

async function tileOrder(page: Page): Promise<(string | null)[]> {
  return tiles(page).evaluateAll((all) => all.map((t) => t.getAttribute("aria-label")));
}

test("each tile carries the controls of its own meter (AC-4PJRVM.1)", async ({ page }) => {
  await twoMeters(page);
  for (const name of [LOAD, PD]) {
    const bar = controls(page, name);
    await expect(tileOf(page, name).getByRole("group", { name: /^Controls for / })).toHaveCount(1);
    await expect(bar.getByRole("button", { name: "Record", exact: true })).toBeVisible();
    await expect(bar.getByRole("button", { name: "Export", exact: true })).toBeVisible();
    await expect(bar.getByLabel("Sample rate")).toBeVisible();
    await expect(bar.getByRole("button", { name: "Disconnect", exact: true })).toBeVisible();
  }
  // No control is left that acts on whichever meter the picker shows.
  await expect(page.getByRole("button", { name: "Record", exact: true })).toHaveCount(2);
  await expect(page.getByRole("button", { name: "Disconnect", exact: true })).toHaveCount(2);
});

test("a tile's Record and rate act on that meter only (AC-4PJRVM.1)", async ({ page }) => {
  await twoMeters(page);
  const load = controls(page, LOAD);
  const pd = controls(page, PD);

  await pd.getByLabel("Sample rate").selectOption("10");
  await expect(pd.getByLabel("Sample rate")).toHaveValue("10");
  await expect(load.getByLabel("Sample rate")).toHaveValue("50");

  await pd.getByRole("button", { name: "Record", exact: true }).click();
  await expect(pd.getByRole("button", { name: "Stop", exact: true })).toBeVisible();
  await expect(load.getByRole("button", { name: "Record", exact: true })).toBeVisible();
  await pd.getByRole("button", { name: "Stop", exact: true }).click();
  await expect(pd.getByRole("button", { name: "Record", exact: true })).toBeVisible();
  await expect(page.getByRole("alert")).toHaveCount(0);
});

test("a tile's Export reports on that meter only (AC-4PJRVM.1)", async ({ page }) => {
  await twoMeters(page);
  const exported = /^Exported /;
  await controls(page, PD).getByRole("button", { name: "Export", exact: true }).click();
  await expect(tileOf(page, PD).getByText(exported)).toBeVisible();
  await expect(tileOf(page, LOAD).getByText(exported)).toHaveCount(0);
});

test("the full view's controls name the expanded meter (AC-4PJRVM.1)", async ({ page }) => {
  await twoMeters(page);
  await tileOf(page, PD)
    .getByRole("button", { name: /Expand/ })
    .click();
  const bar = controls(page, PD);
  await expect(bar).toHaveCount(1);
  await expect(controls(page, LOAD)).toHaveCount(0);
  await expect(bar.getByRole("button", { name: "Record", exact: true })).toBeVisible();
  await expect(bar.getByRole("button", { name: "Export", exact: true })).toBeVisible();
  await expect(bar.getByLabel("Sample rate")).toBeVisible();
  await bar.getByRole("button", { name: "Disconnect", exact: true }).click();
  // Only the named meter left: the other one takes the view.
  await expect(controls(page, LOAD)).toHaveCount(1);
  await expect(controls(page, PD)).toHaveCount(0);
});

test("the tile order stays the same across a reload (AC-4PJRVM.3)", async ({ page }) => {
  await twoMeters(page);
  const before = await tileOrder(page);
  // The full view's meter is the one a reload attaches first: make it the second tile.
  await tileOf(page, PD)
    .getByRole("button", { name: /Expand/ })
    .click();
  await page.getByRole("button", { name: "All meters" }).click();
  await expect(tiles(page)).toHaveCount(2);
  expect(await tileOrder(page)).toEqual(before);

  await reloadApp(page);
  await expect(tiles(page)).toHaveCount(2);
  expect(await tileOrder(page)).toEqual(before);
});

/** The rows of the PD table, the header row included. */
async function pdRowCount(page: Page): Promise<number> {
  return Number((await grid(page).getAttribute("aria-rowcount")) ?? "0");
}

/** Wait until the running capture adds rows to the PD table. */
async function expectPdRowsGrow(page: Page): Promise<void> {
  const before = await pdRowCount(page);
  await expect.poll(() => pdRowCount(page), { timeout: 10_000 }).toBeGreaterThan(before);
}

test("PD capture keeps running when its meter leaves the full view (AC-4PJRVM.4)", async ({
  page,
}) => {
  await twoMeters(page);
  const tab = (name: string) => page.getByRole("tab", { name, exact: true });
  const start = page.getByRole("button", { name: "Start capture" });
  const stop = page.getByRole("button", { name: "Stop capture" });

  await tileOf(page, LOAD)
    .getByRole("button", { name: /Expand/ })
    .click();
  await tab("PD analyzer").click();
  await start.click();
  await expect(stop).toBeEnabled();
  await expectPdRowsGrow(page);

  // The other meter takes the full view, then the capturing one comes back.
  await tab("Meter").click();
  await page.getByRole("button", { name: "All meters" }).click();
  await tileOf(page, PD)
    .getByRole("button", { name: /Expand/ })
    .click();
  await page.getByRole("button", { name: "All meters" }).click();
  await tileOf(page, LOAD)
    .getByRole("button", { name: /Expand/ })
    .click();
  await tab("PD analyzer").click();

  await expect(stop).toBeEnabled();
  await expect(start).toBeDisabled();
  await expectPdRowsGrow(page);
});

test("another meter connects while one flashes firmware (AC-4PJRVM.4)", async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem("open-powerz.advanced-firmware", "true"));
  // `firmware-running`: the update of `demo:idle` is under way.
  await page.goto("/?mock=firmware-running");
  const updating = page.locator("#view-firmware").getByRole("region", { name: "Update progress" });
  await expect(updating).toBeVisible();

  await page.getByRole("button", { name: "Add meter" }).click();
  const dialog = page.getByRole("dialog", { name: "Add a meter" });
  const device = dialog.getByLabel("Device", { exact: true });
  // The flashing meter stays locked: it is not offered.
  await expect(device.locator(`option[value="${FLASHING}"]:enabled`)).toHaveCount(0);
  await device.selectOption(DEMO_PD);
  await dialog.getByRole("button", { name: "Connect" }).click();
  await expect(dialog).toBeHidden();

  await expect.poll(() => connectedIds(page)).toEqual([DEMO_PD]);
  await expect(page.getByRole("alert")).toHaveCount(0);

  // The update still holds its meter.
  await expect(updating).toBeVisible();
  await expect(page.getByLabel("Device", { exact: true })).toBeDisabled();
});
