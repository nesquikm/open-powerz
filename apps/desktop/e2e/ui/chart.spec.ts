/**
 * FR AH2AEM in the browser: the live chart scrolls as batches arrive
 * (AC-AH2AEM.2), hovering the plot shows the time and values (AC-AH2AEM.6), and
 * the "Zero-based axes" toggle is off by default and kept for the session
 * (AC-AH2AEM.9).
 */

import { expect, test } from "@playwright/test";
import { chartDraws, chartSeriesLengths, open, streamDemo } from "./helpers";

test.use({ viewport: { width: 1600, height: 900 } });

test("the live chart scrolls as batches arrive (AC-AH2AEM.2)", async ({ page }) => {
  await open(page);
  await streamDemo(page);
  await expect.poll(async () => (await chartSeriesLengths(page)).vbus).toBeGreaterThan(0);
  const lastLive = async () => (await chartDraws(page)).filter((d) => d.mode === "live").at(-1);
  const first = await lastLive();
  expect(first).toBeDefined();
  await expect
    .poll(async () => (await lastLive())?.t1Ms ?? -Infinity, { timeout: 5_000 })
    .toBeGreaterThan((first?.t1Ms ?? 0) + 500);
  const later = await lastLive();
  expect(later?.t0Ms ?? -Infinity, "the window moves, not just grows").toBeGreaterThanOrEqual(
    first?.t0Ms ?? Infinity,
  );
});

test("hovering the plot shows the time and the VBUS value (AC-AH2AEM.6)", async ({ page }) => {
  await open(page);
  await streamDemo(page);
  await expect.poll(async () => (await chartSeriesLengths(page)).vbus).toBeGreaterThan(20);
  const over = page.locator(".plot .u-over");
  const box = await over.boundingBox();
  if (!box) throw new Error("no plot area");
  await page.mouse.move(box.x + box.width * 0.6, box.y + box.height / 2);
  const strip = page.locator(".chart-wrap .hover");
  await expect(strip.locator(".hover-time")).toHaveText(/^\d+\.\d{3} s$/);
  await expect(strip.locator(".hover-value", { hasText: "VBUS" })).toHaveText(
    /VBUS\s+[+-]?\d+\.\d{3}(–[+-]?\d+\.\d{3})? V/,
  );
  // Off the plot, the strip empties.
  await page.mouse.move(box.x + box.width / 2, box.y - 200);
  await expect(strip.locator(".hover-time")).toHaveCount(0);
});

test('"Zero-based axes" is off by default and kept for the session (AC-AH2AEM.9)', async ({
  page,
}) => {
  await open(page);
  const toggle = page.getByRole("button", { name: "Zero-based axes" });
  await expect(toggle).toHaveAttribute("aria-pressed", "false");
  await toggle.click();
  await expect(toggle).toHaveAttribute("aria-pressed", "true");
  // The chart keeps drawing with the zero-based axes.
  await streamDemo(page);
  await expect.poll(async () => (await chartSeriesLengths(page)).vbus).toBeGreaterThan(0);
  await page.reload();
  await expect(page.getByRole("button", { name: "Zero-based axes" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await page.getByRole("button", { name: "Zero-based axes" }).click();
  await page.reload();
  await expect(page.getByRole("button", { name: "Zero-based axes" })).toHaveAttribute(
    "aria-pressed",
    "false",
  );
});
