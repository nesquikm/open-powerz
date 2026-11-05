/**
 * A phone-wide window (AC-AXHK2M.11): at 375 px, with one meter and with several, the
 * page does not scroll sideways and the Add meter control is whole, on top and usable.
 */

import { expect, test, type Locator, type Page } from "@playwright/test";
import { DEMO_LOAD, open, streamDemo } from "./helpers";

const NARROW = { width: 375, height: 812 };
const DEMO_PD = "demo:pd-9v-2a";

function addMeter(page: Page): Locator {
  return page.getByRole("button", { name: "Add meter" });
}

async function expectNoSidewaysScroll(page: Page): Promise<void> {
  const scroll = await page.evaluate(() => ({
    documentWidth: document.documentElement.scrollWidth,
    windowWidth: document.documentElement.clientWidth,
  }));
  expect(scroll.documentWidth, "no horizontal scroll").toBeLessThanOrEqual(scroll.windowWidth);
  await page.evaluate(() => window.scrollTo(100, 0));
  expect(await page.evaluate(() => window.scrollX), "the page moved sideways").toBe(0);
}

async function expectAddMeterUsable(page: Page): Promise<void> {
  const control = addMeter(page);
  await expect(control).toBeVisible();
  await control.scrollIntoViewIfNeeded();
  const box = await control.boundingBox();
  expect(box, "the control is laid out").toBeTruthy();
  expect(box!.x, "cut off on the left").toBeGreaterThanOrEqual(0);
  expect(box!.x + box!.width, "cut off on the right").toBeLessThanOrEqual(NARROW.width);
  // Nothing covers it, anywhere along the control.
  const covered = await control.evaluate((el) => {
    const r = el.getBoundingClientRect();
    const middle = r.top + r.height / 2;
    return [r.left + 1, r.left + r.width / 2, r.right - 1].filter((x) => {
      const hit = document.elementFromPoint(x, middle);
      return !hit || !(hit === el || el.contains(hit));
    });
  });
  expect(covered, "points of the control under something else").toEqual([]);
}

/** Opens the dialog with a click on the control and adds the 9 V demo meter. */
async function addPdMeter(page: Page): Promise<void> {
  await addMeter(page).click();
  const dialog = page.getByRole("dialog", { name: "Add a meter" });
  await expect(dialog).toBeVisible();
  await dialog.getByLabel("Device", { exact: true }).selectOption(DEMO_PD);
  await dialog.getByRole("button", { name: "Connect" }).click();
  await expect(dialog).toBeHidden();
  await expect(page.getByRole("region", { name: "Meters" }).getByRole("article")).toHaveCount(2);
}

test.describe("at 375 px wide (AC-AXHK2M.11)", () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize(NARROW);
    await open(page);
    await streamDemo(page, DEMO_LOAD);
  });

  test("one meter: no sideways scroll, and Add meter works", async ({ page }) => {
    await expectNoSidewaysScroll(page);
    await expectAddMeterUsable(page);
    await addPdMeter(page);
  });

  test("several meters: no sideways scroll, and Add meter stays usable", async ({ page }) => {
    await addPdMeter(page);
    await expectNoSidewaysScroll(page);
    await expectAddMeterUsable(page);
    await addMeter(page).focus();
    await page.keyboard.press("Enter");
    await expect(page.getByRole("dialog", { name: "Add a meter" })).toBeVisible();
  });

  test("the device select covers no top bar control (AC-AXHK2M.12)", async ({ page }) => {
    const covered = await page
      .locator(".top-bar")
      .getByRole("button")
      .evaluateAll((buttons) =>
        buttons
          .filter((el) => (el as HTMLElement).offsetParent !== null)
          .filter((el) => {
            const r = el.getBoundingClientRect();
            const hit = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
            return !hit || !(hit === el || el.contains(hit));
          })
          .map((el) => el.textContent?.trim() ?? ""),
      );
    expect(covered).toEqual([]);

    const bar = page.locator(".top-bar");
    await expect(bar.getByRole("button", { name: "Disconnect" })).toBeVisible();
    const select = await bar.locator(".device-picker select").boundingBox();
    expect(select, "the select is laid out").toBeTruthy();
    const overlapping = await bar.locator("button, input, select").evaluateAll(
      (controls, s) =>
        controls
          .filter((el) => !el.matches(".device-picker select"))
          .filter((el) => (el as HTMLElement).offsetParent !== null)
          .filter((el) => {
            const r = el.getBoundingClientRect();
            return (
              r.left < s.x + s.width && r.right > s.x && r.top < s.y + s.height && r.bottom > s.y
            );
          })
          .map((el) => el.textContent?.trim() ?? el.tagName),
      select!,
    );
    expect(overlapping).toEqual([]);
  });

  test("the chart options, ruler readout, E.D. note and sub-chart fit (AC-AXHK2T.8)", async ({
    page,
  }) => {
    const chart = page.getByRole("region", { name: "Chart", exact: true });
    const toggle = chart.getByRole("button", { name: "Chart options", exact: true });
    await toggle.scrollIntoViewIfNeeded();
    await toggle.click();
    const group = chart.getByRole("group", { name: "Chart options", exact: true });
    for (const name of ["Ruler", "E.D.", "Sub-chart"]) {
      await group.getByRole("button", { name, exact: true }).click();
    }
    await group.getByLabel("Segment marks", { exact: true }).selectOption("charge");
    const parts = [
      group,
      chart.getByRole("group", { name: "Ruler readout", exact: true }),
      chart.getByRole("button", { name: "Reset ruler", exact: true }),
      chart.locator(".ed-note"),
      chart.getByRole("region", { name: "Sub-chart" }),
    ];
    for (const part of parts) {
      await expect(part).toBeVisible();
      const box = await part.boundingBox();
      expect(box, "laid out").toBeTruthy();
      expect(box!.x, "cut off on the left").toBeGreaterThanOrEqual(0);
      expect(box!.x + box!.width, "cut off on the right").toBeLessThanOrEqual(NARROW.width);
    }
    // Every option button whole, inside the window.
    for (const b of await group.getByRole("button").all()) {
      const box = await b.boundingBox();
      expect(box!.x + box!.width).toBeLessThanOrEqual(NARROW.width);
    }
    await expectNoSidewaysScroll(page);
  });

  for (const view of ["PD analyzer", "Records", "Trigger"]) {
    test(`the ${view} view does not scroll sideways either`, async ({ page }) => {
      await page.getByRole("tab", { name: view, exact: true }).click();
      await expectNoSidewaysScroll(page);
    });
  }
});
