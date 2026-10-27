/**
 * FR 1HHT02 (AC .4): the app offers the CDC ADCNEW stream as a measurement source
 * beside the queue. The source is picked before connecting ("Measurement source");
 * the readout and the chart work the same with either.
 *
 * The mock reports the source a session connected with on
 * `window.__OPEN_POWERZ_MOCK__.connectedSource()` (`null` when none is connected).
 */

import { expect, test, type Page } from "@playwright/test";
import { badge, chartSeriesLengths, expectReadoutNear, open, streamDemo } from "./helpers";

const MOCK_HOOK = "__OPEN_POWERZ_MOCK__";

function connectedSource(page: Page): Promise<string | null> {
  return page.evaluate(
    (key) =>
      (window as unknown as Record<string, { connectedSource(): string | null }>)[
        key
      ].connectedSource(),
    MOCK_HOOK,
  );
}

function sourcePicker(page: Page) {
  return page.getByLabel("Measurement source", { exact: true });
}

test.describe("measurement source (FR 1HHT02)", () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 1600, height: 900 });
  });

  test("offers the queue and the CDC stream, the queue picked by default", async ({ page }) => {
    await open(page);
    const picker = sourcePicker(page);
    await expect(picker).toBeEnabled();
    await expect(picker).toHaveValue("queue");
    await expect(picker.locator("option")).toHaveCount(2);
    await expect(picker.locator('option[value="cdc"]')).toHaveCount(1);
    await streamDemo(page);
    expect(await connectedSource(page)).toBe("queue");
  });

  test("streams the demo meter through the CDC source into the readout and chart", async ({
    page,
  }) => {
    await open(page);
    await sourcePicker(page).selectOption("cdc");
    await streamDemo(page);
    expect(await connectedSource(page)).toBe("cdc");
    await expectReadoutNear(page, "VBUS", 5, 0.02);
    await expect.poll(async () => (await chartSeriesLengths(page)).vbus).toBeGreaterThan(10);
    // The source cannot change under a running session.
    await expect(sourcePicker(page)).toBeDisabled();
    await page.getByRole("button", { name: "Disconnect" }).click();
    await expect(badge(page)).not.toHaveText("Streaming");
    await expect(sourcePicker(page)).toBeEnabled();
    await expect(sourcePicker(page)).toHaveValue("cdc");
  });
});
