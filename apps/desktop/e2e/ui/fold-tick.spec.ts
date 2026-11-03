/**
 * FR AXHK2W, AC-AXHK2W.11: the chart marks the fold boundary with a faint tick that says
 * the data before it is coarser, both while live and while paused. The label sits on a
 * small chip at the top of the plot, at the size of the other chart labels, and stays
 * readable over the data in both themes.
 *
 * API pinned here, beyond the FR's Technical Design:
 *
 * - `?foldat=<samples>` in the page URL sets the mock backend's
 *   `MockOptions.historyFullSamples` (mock builds only): past that many samples the
 *   demo reports a fold boundary through `historyFoldBoundary()`;
 * - the test hook's `chartFoldTick()` (`ChartFoldTick` in `e2e/hook.ts`): the fold tick
 *   of the chart's last draw, or null when that draw had none: the boundary it marks,
 *   the draw's range, the tick's x, the label, the chip and plot boxes (viewport CSS
 *   px), the label's font size (CSS px), and the label's text colour and the chip's
 *   fill as opaque `#rrggbb`;
 * - the label is drawn at the size of the chart's other in-plot labels (the gap
 *   labels, {@link CHART_LABEL_PX} CSS px);
 * - the boundary is known in the live view too (live window and "Whole session" while
 *   live), not only once a paused view asks the history.
 */

import { expect, test, type Locator, type Page } from "@playwright/test";
import { contrastRatio } from "../../src/lib/theme";
import { HOOK, type ChartFoldTick, type OpenPowerzE2E } from "../hook";
import {
  chartSeriesLengths,
  expectHook,
  lastDraw,
  open,
  streamDemo,
  streamOn,
  useTheme,
} from "./helpers";

/** The size the chart draws its other in-plot labels at (gap labels), CSS px. */
const CHART_LABEL_PX = 11;

/** The demo keeps this many samples at full resolution, so it folds within seconds. */
const FOLD_AT = 300;

const LABEL = "coarser before here";

function chart(page: Page): Locator {
  return page.getByRole("region", { name: "Chart", exact: true });
}

function button(scope: Page | Locator, name: string): Locator {
  return scope.getByRole("button", { name, exact: true });
}

/** The fold tick of the chart's last draw, read through the test hook. */
async function foldTick(page: Page): Promise<ChartFoldTick | null> {
  await expectHook(page);
  return page.evaluate((name) => {
    const h = (window as unknown as Record<string, OpenPowerzE2E>)[name]!;
    if (typeof h.chartFoldTick !== "function")
      throw new Error("the test hook has no chartFoldTick()");
    return h.chartFoldTick();
  }, HOOK);
}

/** The mock backend's fold boundary now, session ms, or null. */
async function mockBoundary(page: Page): Promise<number | null> {
  return page.evaluate(async () => {
    const mock = (window as unknown as Record<string, unknown>).__OPEN_POWERZ_MOCK__ as
      { historyFoldBoundary(): Promise<number | null> } | undefined;
    if (!mock) throw new Error("no mock backend on the page");
    return mock.historyFoldBoundary();
  });
}

/** Waits until the last draw has a fold tick and returns it. */
async function drawnTick(page: Page): Promise<ChartFoldTick> {
  let tick: ChartFoldTick | null = null;
  await expect
    .poll(
      async () => {
        tick = await foldTick(page);
        return tick !== null;
      },
      { timeout: 10_000, message: "the chart draws a fold tick" },
    )
    .toBe(true);
  return tick!;
}

/** Streams the demo with a small fold limit until it has folded. */
async function streamFolded(page: Page): Promise<void> {
  await open(page, `?foldat=${FOLD_AT}`);
  await streamDemo(page);
  await expect.poll(async () => (await chartSeriesLengths(page)).vbus).toBeGreaterThan(50);
  await expect
    .poll(() => mockBoundary(page), { timeout: 20_000, message: "the demo folds" })
    .not.toBeNull();
  await streamOn(page, 1_000);
}

/** The tick sits at the boundary and where the draw's scale puts it. */
async function expectAtBoundary(page: Page, tick: ChartFoldTick, slackMs: number): Promise<void> {
  expect(tick.label).toBe(LABEL);
  const now = await mockBoundary(page);
  expect(now).not.toBeNull();
  // The boundary only moves forward: the drawn one is the current one, or a recent one.
  expect(tick.ms).toBeLessThanOrEqual(now!);
  expect(tick.ms).toBeGreaterThanOrEqual(now! - slackMs);
  expect(tick.ms).toBeGreaterThanOrEqual(tick.t0Ms);
  expect(tick.ms).toBeLessThanOrEqual(tick.t1Ms);
  // No collapsed gap in the demo: the scale is linear over the plot.
  const width = tick.plot.right - tick.plot.left;
  const expected = tick.plot.left + ((tick.ms - tick.t0Ms) / (tick.t1Ms - tick.t0Ms)) * width;
  expect(Math.abs(tick.x - expected)).toBeLessThanOrEqual(1.5);
}

/** The chip sits at the top of the plot, inside it, around a label of the chart's size. */
function expectChip(tick: ChartFoldTick): void {
  const { chip, plot } = tick;
  expect(chip.left).toBeGreaterThanOrEqual(plot.left - 0.5);
  expect(chip.right).toBeLessThanOrEqual(plot.right + 0.5);
  expect(chip.top).toBeGreaterThanOrEqual(plot.top - 0.5);
  expect(chip.top, "the chip hugs the top of the plot").toBeLessThanOrEqual(plot.top + 8);
  expect(chip.bottom).toBeLessThanOrEqual(plot.top + (plot.bottom - plot.top) / 3);
  expect(tick.fontPx).toBe(CHART_LABEL_PX);
  expect(chip.bottom - chip.top).toBeGreaterThanOrEqual(tick.fontPx);
  // Small: a label plus a little padding, not a banner.
  expect(chip.bottom - chip.top).toBeLessThanOrEqual(tick.fontPx * 2.2);
  expect(chip.right - chip.left).toBeLessThan((plot.right - plot.left) / 2);
  // Beside its tick: it touches or nearly touches the tick's x.
  const gap = Math.min(Math.abs(chip.left - tick.x), Math.abs(chip.right - tick.x));
  const across = chip.left <= tick.x && tick.x <= chip.right;
  expect(across || gap <= 12, "the chip sits beside its tick").toBe(true);
}

test("the fold tick shows in the live window (AC-AXHK2W.11)", async ({ page }) => {
  await streamFolded(page);
  const draw = await lastDraw(page);
  expect(draw.mode).toBe("live");
  const tick = await drawnTick(page);
  await expectAtBoundary(page, tick, 2_000);
  expectChip(tick);
});

test("the fold tick shows in Whole session while live (AC-AXHK2W.11)", async ({ page }) => {
  await streamFolded(page);
  await button(chart(page), "Whole session").click();
  await expect.poll(async () => (await lastDraw(page)).mode).toBe("live");
  const tick = await drawnTick(page);
  await expectAtBoundary(page, tick, 2_000);
  expectChip(tick);
});

test("the fold tick shows in the paused view (AC-AXHK2W.11)", async ({ page }) => {
  await streamFolded(page);
  await button(chart(page), "Live").click();
  await expect(button(chart(page), "Paused · Go live")).toBeVisible();
  await expect.poll(async () => (await lastDraw(page)).mode).toBe("history");
  const tick = await drawnTick(page);
  // A paused view keeps the boundary it was drawn with while the stream folds on.
  await expectAtBoundary(page, tick, 10_000);
  expectChip(tick);
});

for (const theme of ["light", "dark"] as const) {
  test(`the fold label stays readable on its chip in the ${theme} theme (AC-AXHK2W.11)`, async ({
    page,
  }) => {
    await useTheme(page, theme);
    await streamFolded(page);
    const tick = await drawnTick(page);
    expect(tick.color).toMatch(/^#[0-9a-f]{6}$/i);
    expect(tick.background).toMatch(/^#[0-9a-f]{6}$/i);
    expect(contrastRatio(tick.color, tick.background)).toBeGreaterThanOrEqual(4.5);
    expect(tick.fontPx).toBe(CHART_LABEL_PX);
  });
}
