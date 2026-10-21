/**
 * FR 272WSN, the "Whole session" view after an outage (milestone hardware
 * review): `?mock=outage` starts the meter at a long uptime, keeps 20 s of data,
 * then 40 minutes without any before the live stream goes on.
 *
 * - AC-272WSN.1: the time axis counts from the session start (m:ss once the
 *   range passes a minute), never the meter's own clock;
 * - AC-272WSN.2: the hover readout uses the same session-relative time;
 * - AC-272WSN.4: "Whole session" reaches back across the outage, which the
 *   backend reports as a gap, so no line crosses it;
 * - AC-272WSN.5: the outage is a shaded "No data · 40:00" band.
 */

import { expect, test } from "@playwright/test";
import { MOCK_OUTAGE } from "../../src/lib/mockBackend";
import { chartDraws, chartGapBands, chartXTicks, open, streamDemo } from "./helpers";

test.use({ viewport: { width: 1600, height: 900 } });

/** `m:ss` → seconds. */
function minutesSeconds(label: string): number {
  const [m, s] = label.split(":").map(Number);
  return (m ?? NaN) * 60 + (s ?? NaN);
}

test("Whole session counts from the start and shades the outage (AC-272WSN.1, AC-272WSN.4, AC-272WSN.5)", async ({
  page,
}) => {
  await open(page, "?mock=outage");
  await streamDemo(page);
  await page.getByRole("button", { name: "Whole session" }).click();

  // The view reaches back to the first sample, before the outage.
  await expect
    .poll(async () => (await chartDraws(page)).at(-1)?.t0Ms, { timeout: 5_000 })
    .toBe(MOCK_OUTAGE.startMs);
  const ticks = await chartXTicks(page);
  expect(ticks.length).toBeGreaterThan(2);
  for (const t of ticks) expect(t, "a session-relative m:ss label").toMatch(/^\d{1,2}:\d{2}$/);
  // 20 s before and 40 min of outage: the axis never passes about 41 minutes.
  expect(Math.max(...ticks.map(minutesSeconds))).toBeLessThanOrEqual(41 * 60);

  await expect.poll(() => chartGapBands(page)).toHaveLength(1);
  const [band] = await chartGapBands(page);
  expect(band?.label).toBe("No data · 40:00");
  expect(band?.toMs).toBe(MOCK_OUTAGE.startMs + MOCK_OUTAGE.beforeMs + MOCK_OUTAGE.gapMs);
});

test("the hover readout counts from the session start too (AC-272WSN.2)", async ({ page }) => {
  await open(page, "?mock=outage");
  await streamDemo(page);
  // The 10 s live window sits just after the outage: about 40:20 into the session.
  await expect
    .poll(async () => (await chartDraws(page)).filter((d) => d.mode === "live").length)
    .toBeGreaterThan(3);
  const over = page.locator(".plot .u-over");
  const box = await over.boundingBox();
  if (!box) throw new Error("no plot area");
  await page.mouse.move(box.x + box.width * 0.5, box.y + box.height / 2);
  await expect(page.locator(".chart-wrap .hover .hover-time")).toHaveText(/^40:2\d\.\d{3}$/);
});
