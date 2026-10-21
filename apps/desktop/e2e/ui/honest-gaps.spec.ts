/**
 * FR CWWKJ6, the overview strip marks every gap the chart shades (AC .1): a gap shorter
 * than the collapse threshold gets a faint band at its true width on the `?mock=blip`
 * mock (a short outage), while the long `?mock=outage` gap keeps its collapsed break.
 */

import { expect, test } from "@playwright/test";
import { formatDuration } from "../../src/lib/chartSeries";
import { MOCK_BLIP, MOCK_OUTAGE } from "../../src/lib/mockBackend";
import { COLLAPSE_MIN_GAP_MS, MAX_BREAK_PX } from "../../src/lib/timeline";
import { chartGapBands, open, overviewBreaks, streamDemo } from "./helpers";

test.use({ viewport: { width: 1600, height: 900 } });

test("AC-CWWKJ6.1: a gap shorter than the collapse threshold gets a band on the strip", async ({
  page,
}) => {
  expect(MOCK_BLIP.gapMs).toBeLessThan(COLLAPSE_MIN_GAP_MS);
  await open(page, "?mock=blip");
  await streamDemo(page);
  await page.getByRole("button", { name: "Whole session" }).click();
  // The main chart shades it ...
  await expect.poll(() => chartGapBands(page), { timeout: 5_000 }).toHaveLength(1);
  // ... and the strip marks it too, at its true width, with its length.
  await expect.poll(() => overviewBreaks(page), { timeout: 5_000 }).toHaveLength(1);
  const [brk] = await overviewBreaks(page);
  expect(brk!.label).toContain(formatDuration(MOCK_BLIP.gapMs));
  // Not squeezed to a break: a 10 s gap in a session well under a minute and a half
  // takes a real share of the strip.
  expect(brk!.widthPx).toBeGreaterThan(MAX_BREAK_PX);
  const strip = await page.locator(".chart-wrap .overview").first().boundingBox();
  expect(strip).not.toBeNull();
  expect(brk!.leftPx).toBeGreaterThan(0);
  expect(brk!.leftPx + brk!.widthPx).toBeLessThan(strip!.width);
});

test("AC-CWWKJ6.1: the collapsed outage keeps its narrow break on the strip", async ({ page }) => {
  expect(MOCK_OUTAGE.gapMs).toBeGreaterThan(COLLAPSE_MIN_GAP_MS);
  await open(page, "?mock=outage");
  await streamDemo(page);
  await expect.poll(() => overviewBreaks(page), { timeout: 5_000 }).toHaveLength(1);
  const [brk] = await overviewBreaks(page);
  expect(brk!.widthPx).toBeGreaterThan(0);
  expect(brk!.widthPx).toBeLessThanOrEqual(MAX_BREAK_PX);
  expect(brk!.label).toContain("40:00");
});
