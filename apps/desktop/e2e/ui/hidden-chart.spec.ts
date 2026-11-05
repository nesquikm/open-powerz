/**
 * FR C6VR9K: a hidden chart's draw loop hung the window (AC .1, .3).
 *
 * App.svelte keeps the Meter tab's chart mounted while another tab shows, where uPlot
 * sizes it 1 × 1. The plot-inset alignment padded it anyway, its points landed ~1.3
 * million px off the plot, and the dash builder walked them one dash at a time: on the
 * demo device the PD tab issued ~19 million canvas path calls a second and fell from
 * 43 to 18 frames a second within a minute.
 *
 * Path calls are counted by wrapping `CanvasRenderingContext2D.prototype.moveTo` /
 * `lineTo` before the app loads; frames are counted with requestAnimationFrame over a
 * 2 s window. Both are measured in the page.
 */

import { expect, test, type Locator, type Page } from "@playwright/test";
import { HOOK } from "../hook";
import {
  chartSeriesLengths,
  expectHook,
  interleavedFps,
  type InterleavedFps,
  open,
  PERF_TAG,
  streamDemo,
} from "./helpers";
import { capture, showPd, stopCapture } from "./pd";

test.use({ viewport: { width: 1600, height: 900 } });

/** AC-C6VR9K.1: the most canvas path calls a second any tab may cost. */
const MAX_PATH_CALLS_PER_S = 100_000;
/** AC-C6VR9K.1: the fewest frames a second the window may fall to. */
const MIN_FPS = 50;
/** One measurement window, ms. */
const WINDOW_MS = 2_000;

interface Sample {
  callsPerS: number;
  fps: number;
}

/** Counts every `moveTo` / `lineTo` on any 2D canvas context of the page. */
async function countPathCalls(page: Page): Promise<void> {
  await page.addInitScript(() => {
    const w = window as unknown as { __c6vr9kPathCalls: number };
    w.__c6vr9kPathCalls = 0;
    const proto = CanvasRenderingContext2D.prototype as unknown as Record<
      string,
      (...args: unknown[]) => unknown
    >;
    for (const name of ["moveTo", "lineTo"]) {
      const original = proto[name]!;
      proto[name] = function (this: unknown, ...args: unknown[]) {
        w.__c6vr9kPathCalls++;
        return original.apply(this, args);
      };
    }
  });
}

/** Path calls a second and frames a second over the next {@link WINDOW_MS}. */
async function measure(page: Page): Promise<Sample> {
  return page.evaluate(
    (ms) =>
      new Promise<Sample>((resolve) => {
        const w = window as unknown as { __c6vr9kPathCalls: number };
        const calls0 = w.__c6vr9kPathCalls;
        let frames = 0;
        let t0 = -1;
        const tick = (t: number): void => {
          if (t0 < 0) t0 = t;
          else frames++;
          if (t - t0 < ms) {
            requestAnimationFrame(tick);
            return;
          }
          const seconds = (t - t0) / 1000;
          resolve({ callsPerS: (w.__c6vr9kPathCalls - calls0) / seconds, fps: frames / seconds });
        };
        requestAnimationFrame((t) => {
          // Start both counts on a frame boundary.
          t0 = t;
          frames = 0;
          requestAnimationFrame(tick);
        });
      }),
    WINDOW_MS,
  );
}

/** The path-call bound always, over a streaming window; the frame-rate bound on the
 * median of interleaved streaming windows, only where the median of the same page's idle
 * windows reaches {@link MIN_FPS}. A CI runner without a GPU can pace an idle page below
 * it, and then only the frame-rate half is skipped, said in an annotation and in the log
 * (AC-9QTPNQ.4). */
async function expectSmooth(page: Page, where: string): Promise<void> {
  const s = await measure(page);
  const text = `${where}: ${Math.round(s.callsPerS)} path calls/s`;
  console.log(`AC-C6VR9K.1 ${text}`);
  expect(s.callsPerS, text).toBeLessThanOrEqual(MAX_PATH_CALLS_PER_S);
  const m: InterleavedFps = await interleavedFps(page, `AC-C6VR9K.1 ${where}`);
  if (m.idleMedian < MIN_FPS) {
    const description = `${where}: frame-rate half skipped, the idle median was ${m.idleMedian.toFixed(1)} fps (< ${MIN_FPS})`;
    test.info().annotations.push({ type: "fps-skipped", description });
    // The list reporter prints no annotations: say it in the CI log too.
    console.log(`AC-C6VR9K.1 fps-skipped: ${description}`);
    return;
  }
  expect(
    m.streamMedian,
    `${where}: stream median ${m.streamMedian.toFixed(1)} fps, idle median ${m.idleMedian.toFixed(1)}`,
  ).toBeGreaterThanOrEqual(MIN_FPS);
}

function tab(page: Page, name: string): Locator {
  return page.getByRole("tab", { name, exact: true });
}

async function showTab(page: Page, name: string): Promise<void> {
  await tab(page, name).click();
  await expect(tab(page, name)).toHaveAttribute("aria-selected", "true");
}

/** Waits until `ms` have passed since `since` (a `Date.now()`). */
async function waitUntil(page: Page, since: number, ms: number): Promise<void> {
  const left = since + ms - Date.now();
  if (left > 0) await page.waitForTimeout(left);
}

test(
  "the PD analyzer tab stays smooth for a minute of streaming (AC-C6VR9K.1)",
  { tag: PERF_TAG },
  async ({ page }) => {
    test.setTimeout(200_000);
    await countPathCalls(page);
    await open(page);
    await streamDemo(page);
    await expect.poll(async () => (await chartSeriesLengths(page)).vbus).toBeGreaterThan(50);
    await showPd(page);
    const shown = Date.now();

    // Early, and again a minute of streaming in: the cost grew with the session's
    // length. The idle windows hold the stream for about 6 s, so the second look waits
    // that much longer.
    await waitUntil(page, shown, 8_000);
    await expectSmooth(page, "PD tab at ~10 s");
    await waitUntil(page, shown, 66_000);
    await expectSmooth(page, "PD tab at ~60 s");
  },
);

test(
  "the Records and Trigger tabs stay smooth while streaming (AC-C6VR9K.1)",
  { tag: PERF_TAG },
  async ({ page }) => {
    test.setTimeout(120_000);
    await countPathCalls(page);
    await open(page);
    await streamDemo(page);
    await expect.poll(async () => (await chartSeriesLengths(page)).vbus).toBeGreaterThan(50);
    for (const name of ["Records", "Trigger"]) {
      await showTab(page, name);
      const shown = Date.now();
      await waitUntil(page, shown, 8_000);
      await expectSmooth(page, `${name} tab at ~10 s`);
    }
  },
);

interface PdMarkerDraw {
  leftPx: number;
  topPx: number;
  count: number;
  ms: number;
}

async function chartPdMarkers(page: Page): Promise<PdMarkerDraw[]> {
  await expectHook(page);
  return page.evaluate((name) => {
    const h = (window as unknown as Record<string, Record<string, unknown>>)[name]!;
    const fn = h.chartPdMarkers;
    if (typeof fn !== "function") throw new Error("the test hook has no chartPdMarkers()");
    return (fn as () => PdMarkerDraw[]).call(h);
  }, HOOK);
}

function chart(page: Page): Locator {
  return page.getByRole("region", { name: "Chart", exact: true });
}

/** Turns on chart option `name` (a toggle button in the "Chart options" group). */
async function turnOn(scope: Locator, name: string): Promise<void> {
  const toggle = scope.getByRole("button", { name: "Chart options", exact: true });
  if ((await toggle.getAttribute("aria-expanded")) !== "true") await toggle.click();
  await expect(toggle).toHaveAttribute("aria-expanded", "true");
  const group = scope.getByRole("group", { name: "Chart options", exact: true });
  const b = group.getByRole("button", { name, exact: true });
  if ((await b.getAttribute("aria-pressed")) !== "true") await b.click();
  await expect(b).toHaveAttribute("aria-pressed", "true");
}

/** The left and right x of a plot area, page CSS px. */
async function edges(over: Locator, what: string): Promise<[number, number]> {
  const box = await over.boundingBox();
  if (box === null) throw new Error(`no ${what}`);
  return [box.x, box.x + box.width];
}

test("back on the Meter tab, the plot matches the sub-chart and PD markers sit on it (AC-C6VR9K.3)", async ({
  page,
}) => {
  test.setTimeout(90_000);
  await open(page);
  await streamDemo(page);
  await expect.poll(async () => (await chartSeriesLengths(page)).vbus).toBeGreaterThan(50);
  const c = chart(page);
  await turnOn(c, "Sub-chart");
  const sub = c.getByRole("region", { name: "Sub-chart" });
  await expect(sub.locator(".u-over").first()).toBeVisible();

  // A PD capture on the PD tab, a while there, then back to the Meter tab.
  await showPd(page);
  await capture(page);
  await page.waitForTimeout(2_500);
  await stopCapture(page);
  await page.waitForTimeout(3_000);
  await showTab(page, "Meter");
  await expect(c).toBeVisible();

  const over = c.locator(".plot .u-over").first();
  const subOver = sub.locator(".u-over").first();
  // Shown again, the main plot draws at its real size with the sub-chart's insets.
  await expect
    .poll(
      async () => {
        const [a, b] = [await edges(over, "plot area"), await edges(subOver, "sub-chart area")];
        return Math.max(Math.abs(a[0] - b[0]), Math.abs(a[1] - b[1]));
      },
      { message: "main plot and sub-chart plot areas start and end together", timeout: 5_000 },
    )
    .toBeLessThanOrEqual(1);
  const [overLeft, overRight] = await edges(over, "plot area");
  expect(overRight - overLeft).toBeGreaterThan(200);

  // The PD markers of the capture are drawn, each inside the plot area.
  await expect.poll(async () => (await chartPdMarkers(page)).length).toBeGreaterThan(0);
  const layer = await c.locator('canvas[data-layer="pd-markers"]').boundingBox();
  if (layer === null) throw new Error("no PD marker layer");
  for (const m of await chartPdMarkers(page)) {
    const x = layer.x + m.leftPx;
    expect(x, `marker at ${m.ms} ms`).toBeGreaterThanOrEqual(overLeft - 1);
    expect(x, `marker at ${m.ms} ms`).toBeLessThanOrEqual(overRight + 1);
  }
});
