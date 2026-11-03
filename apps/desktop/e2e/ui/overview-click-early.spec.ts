/**
 * FR B5XBTA, AC .2: a click on the overview strip right after connect, while the live
 * view is still narrower than the session's narrowest allowed view, pauses the chart
 * and centres the view on the clicked time.
 *
 * The page clock is held from before the connect and stepped by hand, so the stream
 * cannot widen the live view between the check and the click: the test reaches the
 * narrow-view state every run instead of by luck.
 */

import { expect, test, type Page } from "@playwright/test";
import type { ChartDraw } from "../hook";
import { minSpanMs } from "../../src/lib/timeline";
import { badge, chartDraws, connectDemo, holdClock, open, overviewWindow } from "./helpers";

test.use({ viewport: { width: 1600, height: 900 } });

const SEEDED = 200_000;
/** One hand step of the held page clock, ms: well under a sample period at 50 SPS. */
const STEP_MS = 4;

function span(d: { t0Ms: number; t1Ms: number }): number {
  return d.t1Ms - d.t0Ms;
}

async function box(page: Page, selector: string) {
  const b = await page.locator(selector).first().boundingBox();
  if (!b) throw new Error(`no box for ${selector}`);
  return b;
}

/** The newest live draw, or undefined before any. */
async function newestLive(page: Page): Promise<ChartDraw | undefined> {
  return [...(await chartDraws(page))].reverse().find((d) => d.mode === "live");
}

test("AC-B5XBTA.2: a click on the strip while the live view is narrower than the narrowest view pauses and centres there", async ({
  page,
}) => {
  await page.clock.install();
  await open(page, `?synthetic=${SEEDED}`);
  const rate = Number(await page.getByLabel("Sample rate").inputValue());
  const minSpan = minSpanMs(Number.isFinite(rate) && rate > 0 ? rate : null);
  // Held from before the connect: the stream only moves when the test steps the clock.
  await holdClock(page);
  await connectDemo(page);

  // Step the clock until the strip shows the seeded session with the live window at its
  // right end while the live view is still narrower than the narrowest allowed view.
  const strip = page.locator(".chart-wrap .overview");
  let live: ChartDraw | undefined;
  let ready = false;
  for (let i = 0; i < 500 && !ready; i++) {
    live = await newestLive(page);
    const streaming = (await badge(page).textContent())?.trim() === "Streaming";
    if (streaming && live && (await strip.count()) > 0) {
      expect(span(live), "the live view is still narrower than the narrowest view").toBeLessThan(
        minSpan,
      );
      const s = await box(page, ".chart-wrap .overview");
      const win = await overviewWindow(page);
      ready = win !== null && win.leftPx > s.width * 0.3;
    }
    if (!ready) await page.clock.runFor(STEP_MS);
  }
  expect(ready, "reached a narrow live view over the seeded session").toBe(true);
  expect(span(live!)).toBeLessThan(minSpan);

  const s = await box(page, ".chart-wrap .overview");
  await page.mouse.click(s.x + s.width * 0.25, s.y + s.height / 2);
  await page.clock.resume();

  const liveButton = page.locator(".chart-wrap button.live");
  await expect(liveButton).toHaveText("Paused · Go live");
  await expect
    .poll(async () => {
      const d = (await chartDraws(page)).at(-1);
      return d?.mode === "history" && d.fromHistory;
    })
    .toBe(true);
  const after = (await chartDraws(page)).at(-1)!;
  const centre = (after.t0Ms + after.t1Ms) / 2;
  const session = live!.t1Ms;
  expect(centre / session).toBeGreaterThan(0.15);
  expect(centre / session).toBeLessThan(0.35);
  // The view is at least the narrowest allowed one.
  expect(span(after)).toBeGreaterThanOrEqual(minSpan - 1e-6);
});
