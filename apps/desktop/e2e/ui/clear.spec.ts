/**
 * FR 5X154V, Clear chart history clears what the chart shows (bug).
 *
 * - AC-5X154V.1: regression — after a clear the chart shows only data recorded after it;
 * - AC-5X154V.2: the live windows, "Whole session", the overview strip, the gap bands
 *   and the playhead hold only data after the clear, and the meter keeps streaming;
 * - AC-5X154V.4: Clear stays available while recording, and the recording keeps every row;
 * - AC-5X154V.5: the stats and history buttons say what they erase.
 *
 * AC-5X154V.3 (an export after a clear holds only later rows) is checked against the real
 * backend in `src-tauri/tests/history.rs`; the mock's export counts streamed samples.
 */

import { expect, test, type Page } from "@playwright/test";
import { MOCK_OUTAGE } from "../../src/lib/mockBackend";
import type { ChartDraw } from "../hook";
import {
  badge,
  chartBreaks,
  chartDraws,
  chartGapBands,
  chartPlayhead,
  chartSeriesLengths,
  lastDraw,
  open,
  overviewBreaks,
  overviewWindow,
  recordingElapsedSeconds,
  statsElapsedSeconds,
  streamDemo,
} from "./helpers";

test.use({ viewport: { width: 1600, height: 900 } });

const RESET_TOOLTIP =
  "Zero the statistics (min, max, average, charge, energy). The chart history stays.";
const CLEAR_TOOLTIP =
  "Erase the chart history and what Export saves. The meter stays connected; recordings are not affected.";
const CLEAR_QUESTION = "Clear the chart history? The meter stays connected and keeps measuring.";

/** A little slack for the batch in flight when the clear lands, in session ms. */
const SLACK_MS = 250;

function statsPanel(page: Page) {
  return page.getByRole("complementary", { name: "Statistics" });
}

/** The last live draw, or null. */
async function lastLiveDraw(page: Page): Promise<ChartDraw | null> {
  return [...(await chartDraws(page))].reverse().find((d) => d.mode === "live") ?? null;
}

/** Streams until the live window has a few seconds behind it; returns the newest session ms. */
async function streamFor(page: Page, seconds: number): Promise<number> {
  await expect
    .poll(() => statsElapsedSeconds(page), { timeout: (seconds + 5) * 1000 })
    .toBeGreaterThanOrEqual(seconds);
  await expect.poll(async () => (await lastLiveDraw(page)) !== null).toBe(true);
  return (await lastLiveDraw(page))!.t1Ms;
}

/** Opens the confirm with the history button (`name`) and confirms it. */
async function clearHistory(page: Page, name: string | RegExp): Promise<void> {
  const panel = statsPanel(page);
  await panel.getByRole("button", { name, exact: typeof name === "string" }).click();
  await panel.getByRole("button", { name: "Clear", exact: true }).click();
  await expect(panel.getByRole("button", { name: "Cancel", exact: true })).toHaveCount(0);
  await expect(page.getByRole("alert")).toHaveCount(0);
}

/**
 * Pauses the running page clock a moment ahead of its current time. The clock keeps
 * running while the pause request travels to the page, so a target that close can be
 * in the past by the time it lands; then retry with a wider lead.
 */
async function holdClock(page: Page): Promise<void> {
  for (let leadMs = 50; ; leadMs *= 2) {
    const target = (await page.evaluate(() => Date.now())) + leadMs;
    try {
      await page.clock.pauseAt(target);
      return;
    } catch (e) {
      if (leadMs >= 3_200 || !String(e).includes("Cannot fast-forward to the past")) throw e;
    }
  }
}

/** "Whole session" and its first draw that ends after `afterMs`. */
async function wholeSession(page: Page, afterMs: number): Promise<ChartDraw> {
  await page.getByRole("button", { name: "Whole session" }).click();
  await expect
    .poll(async () => (await lastDraw(page)).t1Ms, { timeout: 5_000 })
    .toBeGreaterThan(afterMs);
  return lastDraw(page);
}

test("after a clear the chart shows only data recorded after it", async ({ page }) => {
  await open(page);
  await streamDemo(page);
  const clearAt = await streamFor(page, 3);
  expect(clearAt).toBeGreaterThan(2_000);

  // Either label: the bug is in what the chart shows, not in what the button says.
  await clearHistory(page, /^Clear (chart )?history…$/);

  // The live 10 s window starts again from the clear, not from the first sample.
  await expect
    .poll(async () => (await lastLiveDraw(page))?.t0Ms ?? -1, { timeout: 5_000 })
    .toBeGreaterThanOrEqual(clearAt - SLACK_MS);

  // "Whole session" reaches back only to the clear.
  const whole = await wholeSession(page, clearAt + 500);
  expect(whole.t0Ms, "Whole session starts at the clear, not at 0").toBeGreaterThanOrEqual(
    clearAt - SLACK_MS,
  );
});

test("a clear empties the overview strip at once, before the next sample (AC-5X154V.2)", async ({
  page,
}) => {
  // The page clock runs as usual until just before the clear, then holds still.
  await page.clock.install();
  await open(page, "?mock=outage");
  await streamDemo(page);
  await expect.poll(() => overviewBreaks(page), { timeout: 5_000 }).toHaveLength(1);
  // A slow stream: the next sample is up to half a second away when the clear lands.
  await page.getByLabel("Sample rate").selectOption("2");
  await expect(page.getByLabel("Sample rate")).toHaveValue("2");
  await page.waitForTimeout(1_200);
  // Hold the clock: no stream tick, so no sample, until the checks below are done.
  await holdClock(page);
  await clearHistory(page, "Clear chart history…");
  await expect.poll(() => overviewBreaks(page), { timeout: 150, intervals: [10] }).toEqual([]);
  // No window over an empty strip until the next sample arrives.
  await expect(page.locator(".chart-wrap .overview-window")).toHaveCount(0, { timeout: 150 });
  // AC-KQF5GM.6: the page clock is held around the clear, so no sample can arrive
  // between the clear and its checks however slow the runner is: a full second of
  // real time later the strip is still empty and has no window.
  await page.waitForTimeout(1_000);
  expect(await overviewBreaks(page), "no sample reached the strip after the clear").toEqual([]);
  expect(
    await page.locator(".chart-wrap .overview-window").count(),
    "no sample arrived between the clear and its checks",
  ).toBe(0);
});

test("a clear empties every view of the chart and the meter keeps streaming (AC-5X154V.2)", async ({
  page,
}) => {
  await open(page, "?mock=outage");
  await streamDemo(page);
  // The session before the clear: 20 s, a 40-minute outage, then the live stream.
  await expect.poll(() => overviewBreaks(page), { timeout: 5_000 }).toHaveLength(1);
  const gapTo = MOCK_OUTAGE.startMs + MOCK_OUTAGE.beforeMs + MOCK_OUTAGE.gapMs;
  const clearAt = await streamFor(page, 2);
  expect(clearAt).toBeGreaterThan(gapTo);

  // Pause and place a playhead in the data from before the clear.
  await page.getByRole("button", { name: "Whole session" }).click();
  await expect
    .poll(async () => (await lastDraw(page)).t0Ms, { timeout: 5_000 })
    .toBe(MOCK_OUTAGE.startMs);
  await page.locator(".chart-wrap button.live").click();
  await expect(page.locator(".chart-wrap button.live")).toHaveText("Paused · Go live");
  const plot = await page.locator(".plot .u-over").first().boundingBox();
  if (!plot) throw new Error("no plot area");
  await page.mouse.click(plot.x + plot.width * 0.1, plot.y + plot.height / 2);
  await page.mouse.move(plot.x + plot.width / 2, plot.y - 200);
  await expect.poll(() => chartPlayhead(page)).not.toBeNull();
  expect((await chartPlayhead(page))!).toBeLessThan(clearAt);

  await clearHistory(page, "Clear chart history…");

  // The chart goes back to the live window, and the playhead from before is gone.
  await expect(page.locator(".chart-wrap button.live")).toHaveText("Live");
  await expect
    .poll(async () => {
      const head = await chartPlayhead(page);
      return head === null || head >= clearAt - SLACK_MS;
    })
    .toBe(true);

  // The meter keeps streaming into the chart.
  await expect(badge(page)).toHaveText("Streaming");
  const drawn = (await chartSeriesLengths(page)).vbus;
  await expect.poll(async () => (await chartSeriesLengths(page)).vbus).toBeGreaterThan(drawn);

  // The live window starts at the clear.
  await expect
    .poll(async () => (await lastLiveDraw(page))?.t0Ms ?? -1, { timeout: 5_000 })
    .toBeGreaterThanOrEqual(clearAt - SLACK_MS);

  // The overview strip no longer reaches back over the outage: no break, and the 10 s
  // window covers the couple of seconds since the clear, i.e. nearly the whole strip.
  await expect.poll(() => overviewBreaks(page), { timeout: 5_000 }).toHaveLength(0);
  const strip = await page.locator(".chart-wrap .overview").first().boundingBox();
  if (!strip) throw new Error("no overview strip");
  await expect
    .poll(async () => (await overviewWindow(page))?.widthPx ?? 0, { timeout: 5_000 })
    .toBeGreaterThan(strip.width * 0.8);

  // "Whole session": only data after the clear, so no outage band and no collapsed gap.
  const whole = await wholeSession(page, clearAt + 500);
  expect(whole.t0Ms, "Whole session starts at the clear").toBeGreaterThanOrEqual(
    clearAt - SLACK_MS,
  );
  await expect.poll(() => chartGapBands(page)).toHaveLength(0);
  expect(await chartBreaks(page)).toHaveLength(0);
  // The time axis starts at the clear: every draw from now on starts there too.
  for (const d of (await chartDraws(page)).slice(-3)) {
    expect(d.t0Ms).toBeGreaterThanOrEqual(clearAt - SLACK_MS);
  }
});

test("a clear while recording leaves the recording whole (AC-5X154V.4)", async ({ page }) => {
  await open(page);
  await streamDemo(page);
  await page.getByRole("button", { name: "Record", exact: true }).click();
  const stop = page.getByRole("button", { name: "Stop", exact: true });
  await expect(stop).toHaveAttribute("aria-pressed", "true");
  await expect
    .poll(() => recordingElapsedSeconds(page), { timeout: 8_000 })
    .toBeGreaterThanOrEqual(2);
  const rowsOf = async () => {
    const m = /([\d,]+) rows/.exec(await page.getByRole("timer").innerText());
    return m ? Number(m[1]!.replace(/,/g, "")) : NaN;
  };
  await expect.poll(rowsOf).toBeGreaterThan(0);
  const rowsBefore = await rowsOf();

  // Clear stays available while recording.
  const clearButton = statsPanel(page).getByRole("button", {
    name: "Clear chart history…",
    exact: true,
  });
  await expect(clearButton).toBeEnabled();
  await clearHistory(page, "Clear chart history…");

  // The recording runs on and its row count keeps growing from where it was.
  await expect(stop).toHaveAttribute("aria-pressed", "true");
  await expect.poll(rowsOf, { timeout: 5_000 }).toBeGreaterThan(rowsBefore);
  const rowsAfterClear = await rowsOf();
  expect(rowsAfterClear).toBeGreaterThan(rowsBefore);

  await stop.click();
  await expect(page.getByRole("button", { name: "Record", exact: true })).toHaveAttribute(
    "aria-pressed",
    "false",
  );
  // The summary counts every row, from before the clear and after it.
  const summary = page.getByText(/[\d,]+ rows written/);
  await expect(summary).toBeVisible();
  const written = Number(
    /([\d,]+) rows written/.exec(await summary.innerText())![1]!.replace(/,/g, ""),
  );
  expect(written).toBeGreaterThanOrEqual(rowsAfterClear);
});

test("the stats and history buttons say what they erase (AC-5X154V.5)", async ({ page }) => {
  await open(page);
  await streamDemo(page);
  const panel = statsPanel(page);

  const reset = panel.getByRole("button", { name: "Reset stats", exact: true });
  await expect(reset).toBeVisible();
  await expect(reset).toHaveAttribute("title", RESET_TOOLTIP);

  const clear = panel.getByRole("button", { name: "Clear chart history…", exact: true });
  await expect(clear).toBeVisible();
  await expect(clear).toHaveAttribute("title", CLEAR_TOOLTIP);

  await clear.click();
  await expect(panel.getByText(CLEAR_QUESTION, { exact: true })).toBeVisible();
  await expect(panel.getByRole("group", { name: CLEAR_QUESTION })).toBeVisible();
  await panel.getByRole("button", { name: "Cancel", exact: true }).click();
  await expect(clear).toBeVisible();
});
