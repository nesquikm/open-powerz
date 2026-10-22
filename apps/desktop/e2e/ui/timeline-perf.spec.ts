/**
 * AC-FENCD9.12: with a 10-million-sample history in the mocked backend
 * (`?synthetic=10000000`), an overview viewport drag step, a wheel zoom step
 * and a shuttle step each render in at most 100 ms: from the input to the
 * chart's draw of the new range from its history response, read through the
 * e2e test hook (the pattern of `pan.spec.ts`). With the 1000 SPS demo device
 * and the overview strip shown, live redraws stay at 20 to 60 a second.
 *
 * Timings run on Chromium only: the seed is ~320 MB of columns per page, and
 * the bound is a property of the app, not of the browser engine.
 */

import { expect, test, type Page } from "@playwright/test";
import { HOOK, type ChartDraw, type OpenPowerzE2E } from "../hook";
import {
  blur,
  chartDraws,
  chartPlayhead,
  chartSeriesLengths,
  expectHook,
  open,
  streamDemo,
} from "./helpers";
import { liveRedrawBand, renderReport, renderStepBudgetMs } from "./budget";

const SYNTHETIC = 10_000_000;
const STEPS = 6;
/** AC-KQF5GM.5: 100 ms, times the CI factor of 3 when CI is set. */
const BOUND_MS = renderStepBudgetMs();

test.use({ viewport: { width: 1600, height: 900 } });

/** Waits for the first history draw after draw `from` whose range differs from `prev`. */
async function drawAfter(page: Page, from: number, prev: ChartDraw | null): Promise<ChartDraw> {
  const done = await page.waitForFunction(
    ([name, start, p]) => {
      const h = (window as unknown as Record<string, OpenPowerzE2E>)[name]!;
      return (
        h
          .chartDraws()
          .slice(start)
          .find(
            (d) =>
              d.mode === "history" &&
              d.fromHistory &&
              (p === null || d.t0Ms !== p.t0Ms || d.t1Ms !== p.t1Ms),
          ) ?? false
      );
    },
    [HOOK, from, prev] as const,
    { timeout: 10_000, polling: "raf" },
  );
  return (await done.jsonValue()) as ChartDraw;
}

async function seeded(page: Page, browserName: string): Promise<void> {
  test.skip(browserName !== "chromium", "timed on Chromium only");
  test.setTimeout(180_000);
  await open(page, `?synthetic=${SYNTHETIC}`);
  await streamDemo(page);
  await expectHook(page);
  await expect.poll(async () => (await chartSeriesLengths(page)).vbus).toBeGreaterThan(0);
  // The chart must know where the seeded session starts (Home, panning back): show
  // the whole session until it draws from 0, then return to the 10 s window.
  await page.getByRole("button", { name: "Whole session" }).click();
  await expect.poll(async () => (await chartDraws(page)).at(-1)?.t0Ms, { timeout: 60_000 }).toBe(0);
  await page.getByRole("button", { name: "10 s" }).click();
}

/** Pauses with the Live button and waits for the paused range's history draw. */
async function pause(page: Page): Promise<ChartDraw> {
  await page.locator(".chart-wrap button.live").click();
  await expect(page.locator(".chart-wrap button.live")).toHaveText("Paused · Go live");
  await expect
    .poll(async () => {
      const d = (await chartDraws(page)).at(-1);
      return d?.mode === "history" && d.fromHistory;
    })
    .toBe(true);
  return (await chartDraws(page)).at(-1)!;
}

function report(name: string, timings: number[]): string {
  return renderReport(name, timings, BOUND_MS);
}

test("an overview viewport drag step over a 10M-sample history renders in ≤ 100 ms (AC-FENCD9.12)", async ({
  page,
  browserName,
}) => {
  await seeded(page, browserName);
  let previous = await pause(page);
  const w = await page.locator(".chart-wrap .overview-window").boundingBox();
  if (!w) throw new Error("no overview window");
  let x = w.x + w.width / 2;
  const y = w.y + w.height / 2;
  await page.mouse.move(x, y);
  await page.mouse.down();
  const timings: number[] = [];
  for (let step = 0; step < STEPS; step++) {
    const before = (await chartDraws(page)).length;
    x -= 40;
    // The clock starts before the move is sent, so the round trip counts against the bound.
    const movedAt = await page.evaluate(() => performance.now());
    await page.mouse.move(x, y);
    const draw = await drawAfter(page, before, previous);
    timings.push(draw.at - movedAt);
    previous = draw;
  }
  await page.mouse.up();
  expect(Math.max(...timings), report("overview drag step", timings)).toBeLessThanOrEqual(BOUND_MS);
});

test("a wheel zoom step over a 10M-sample history renders in ≤ 100 ms (AC-FENCD9.12)", async ({
  page,
  browserName,
}) => {
  await seeded(page, browserName);
  let previous = await pause(page);
  await page.getByRole("application").focus();
  const timings: number[] = [];
  for (let step = 0; step < STEPS; step++) {
    const before = (await chartDraws(page)).length;
    // Wheel out inside the page so the start time and the draw time share a clock.
    const wheeledAt = await page.evaluate(() => {
      const over = document.querySelector(".plot .u-over");
      if (!over) throw new Error("no plot area");
      const r = over.getBoundingClientRect();
      const t = performance.now();
      over.dispatchEvent(
        new WheelEvent("wheel", {
          deltaY: 100,
          clientX: r.left + r.width / 2,
          clientY: r.top + r.height / 2,
          bubbles: true,
          cancelable: true,
        }),
      );
      return t;
    });
    const draw = await drawAfter(page, before, previous);
    timings.push(draw.at - wheeledAt);
    previous = draw;
  }
  expect(Math.max(...timings), report("wheel zoom step", timings)).toBeLessThanOrEqual(BOUND_MS);
});

test("a shuttle step over a 10M-sample history renders in ≤ 100 ms (AC-FENCD9.12)", async ({
  page,
  browserName,
}) => {
  await seeded(page, browserName);
  await blur(page);
  await page.keyboard.press("Home");
  await expect(page.locator(".chart-wrap button.live")).toHaveText("Paused · Go live");
  let previous = (await chartDraws(page)).at(-1) ?? null;
  // The playhead at the view's right edge: every shuttle step moves the view with it.
  const over = await page.locator(".plot .u-over").boundingBox();
  if (!over) throw new Error("no plot area");
  await page.mouse.click(over.x + over.width - 1, over.y + over.height / 2);
  await page.mouse.move(over.x + over.width / 2, over.y - 200);
  await expect.poll(() => chartPlayhead(page)).not.toBeNull();
  await blur(page);
  const timings: number[] = [];
  let from = (await chartDraws(page)).length;
  const pressedAt = await page.evaluate(() => {
    const t = performance.now();
    document.body.dispatchEvent(
      new KeyboardEvent("keydown", { key: "l", bubbles: true, cancelable: true }),
    );
    return t;
  });
  // Each step: from the previous step's draw (the key press for the first) to the next
  // draw of a moved range.
  let last = pressedAt;
  for (let step = 0; step < STEPS; step++) {
    const draw = await drawAfter(page, from, previous);
    timings.push(draw.at - last);
    last = draw.at;
    previous = draw;
    from = (await chartDraws(page)).findIndex((d) => d.at === draw.at) + 1;
  }
  await page.keyboard.press("k");
  expect(Math.max(...timings), report("shuttle step", timings)).toBeLessThanOrEqual(BOUND_MS);
});

test("with the overview strip shown, 1000 SPS live redraws stay at 20–60 per second (AC-FENCD9.12)", async ({
  page,
  browserName,
}) => {
  // Timed on Chromium only, like every timing in this suite: headless WebKit on the CI
  // VMs (no GPU) paints at ~12 Hz whatever the app does. Real-device redraw rates are
  // checked by hand on the meter.
  test.skip(browserName !== "chromium", "timed on Chromium only");
  await open(page);
  await streamDemo(page);
  await expect(page.locator(".chart-wrap .overview")).toBeVisible();
  await blur(page);
  await page.keyboard.press("4");
  await expect(page.getByLabel("Sample rate")).toHaveValue("1000");
  await expect.poll(async () => (await chartSeriesLengths(page)).vbus).toBeGreaterThan(1000);
  const start = await page.evaluate(() => performance.now());
  // The 2 s sleep is the measurement window of a throughput budget, not a settle wait.
  await page.waitForTimeout(2_000);
  const end = await page.evaluate(() => performance.now());
  const live = (await chartDraws(page)).filter(
    (d) => d.mode === "live" && d.at >= start && d.at <= end,
  );
  const perSecond = (live.length * 1000) / (end - start);
  // AC-KQF5GM.9: 20–60 per second locally; on CI only the floor is divided by the CI factor.
  const { minPerSecond, maxPerSecond } = liveRedrawBand();
  const report = `live redraws per second: ${perSecond.toFixed(1)}; band ${minPerSecond.toFixed(1)}–${maxPerSecond}`;
  expect(perSecond, report).toBeGreaterThanOrEqual(minPerSecond);
  expect(perSecond, report).toBeLessThanOrEqual(maxPerSecond);
});
