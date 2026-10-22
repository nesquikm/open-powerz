/**
 * FR JFD9XR: regressions from the QA click-through of build 9787adb.
 *
 * - AC-JFD9XR.1 / .2: while recording, the rate keys 1–4 follow the rate
 *   picker's lock: the rate stays put and the notice "Stop the recording to
 *   change the rate." shows; the mock refuses the change as the backend does.
 * - AC-JFD9XR.3 / .4: a paused chart still draws Power (from the history's
 *   Power series), and the hover readout lists it.
 * - AC-JFD9XR.5 / .6: the chart's screen-reader status follows every view
 *   change: a live window change, pause, wheel zoom, +, a paused preset and an
 *   overview drag each announce the view now on screen.
 */

import { expect, test, type Page } from "@playwright/test";
import type { ChartDraw } from "../hook";
import {
  blur,
  chartDraws,
  chartSeriesLengths,
  expectHook,
  open,
  streamDemo,
  streamOn,
} from "./helpers";

test.use({ viewport: { width: 1600, height: 900 } });

const RATE_NOTICE = "Stop the recording to change the rate.";

function chartStatus(page: Page) {
  return page.locator('.chart-wrap p[role="status"][aria-live="polite"]');
}

function liveButton(page: Page) {
  return page.locator(".chart-wrap button.live");
}

test("rate keys do nothing while recording", async ({ page }) => {
  await open(page);
  await streamDemo(page);
  const rate = page.getByLabel("Sample rate");
  await expect(rate).toHaveValue("50");
  await blur(page);
  await page.keyboard.press(" ");
  await expect(page.getByRole("timer")).toBeVisible();
  await expect(rate).toBeDisabled();

  for (const key of ["4", "1", "2"]) {
    await blur(page);
    await page.keyboard.press(key);
    await expect(page.getByText(RATE_NOTICE, { exact: true })).toBeVisible();
    await expect(rate).toHaveValue("50");
  }
  // No error banner: the keys never reached the backend.
  await expect(page.getByRole("alert")).toHaveCount(0);
  // The stream keeps its rate for a while after the presses: half a second more of data.
  await streamOn(page, 500);
  await expect(rate).toHaveValue("50");
  await expect(page.locator(".badge")).toHaveText("Streaming");
});

/** Hovers the middle of the plot and returns the hover strip. */
async function hoverPlot(page: Page) {
  const box = await page.locator(".plot .u-over").boundingBox();
  if (!box) throw new Error("no plot area");
  await page.mouse.move(box.x + box.width * 0.5, box.y + box.height / 2);
  return page.locator(".chart-wrap .hover");
}

test("Power stays drawn when the chart is paused", async ({ page }) => {
  await open(page);
  await streamDemo(page);
  await expectHook(page);
  await expect.poll(async () => (await chartSeriesLengths(page)).vbus).toBeGreaterThan(20);
  const power = page.getByRole("group", { name: "Series" }).getByRole("button", { name: "Power" });
  await power.click();
  await expect(power).toHaveAttribute("aria-pressed", "true");

  // Live: the hover lists Power.
  let strip = await hoverPlot(page);
  await expect(strip.locator(".hover-value", { hasText: "Power" })).toHaveText(/Power\s+\S+.*W/);

  await liveButton(page).click();
  await expect(liveButton(page)).toHaveText("Paused · Go live");
  await expect
    .poll(async () => {
      const d = (await chartDraws(page)).at(-1);
      return d?.mode === "history" && d.fromHistory;
    })
    .toBe(true);

  // Paused: still there, drawn from the history's buckets (5 V × 1 A ≈ 5 W).
  await page.mouse.move(0, 0);
  strip = await hoverPlot(page);
  await expect(strip.locator(".hover-value", { hasText: "VBUS" })).toBeVisible();
  const powerValue = strip.locator(".hover-value", { hasText: "Power" });
  await expect(powerValue).toHaveText(/Power\s+\S+.*W/);
  const text = (await powerValue.innerText()).replace(/−/g, "-");
  const watts = Number.parseFloat(text.replace(/^Power\s*/, "").replace(/[^\d.+-].*$/, ""));
  expect(watts).toBeGreaterThan(3);
  expect(watts).toBeLessThan(7);
});

/** Seconds from a session time as the status writes it: `1.5 s`, `1:02.0`, `1:02:03.4`. */
function parseSessionTime(text: string): number {
  const t = text.trim().replace(/−/g, "-").replace(/\s*s$/, "");
  return t.split(":").reduce((acc, part) => acc * 60 + Number.parseFloat(part), 0);
}

/** The range a "Paused: a to b" status names, in seconds, or null for any other text. */
function pausedRange(status: string): { from: number; to: number } | null {
  const m = /^Paused: (.+) to (.+)$/.exec(status.trim());
  if (!m || m[1] === undefined || m[2] === undefined) return null;
  return { from: parseSessionTime(m[1]), to: parseSessionTime(m[2]) };
}

/** The newest draw of a history range from its own history answer. */
async function lastHistoryDraw(page: Page): Promise<ChartDraw | undefined> {
  return (await chartDraws(page)).filter((d) => d.mode === "history" && d.fromHistory).at(-1);
}

/** Slack for the status's one-decimal rounding, s. Not a timing allowance: the status
 * and the draw are compared from the same poll, and the poll waits for them to agree. */
const SLACK_S = 0.11;

/**
 * Waits until the status names the paused range on screen; `origin` is the session
 * start the status counts from (session ms), learnt from the first paused view.
 * Returns the origin and the draw the status was checked against.
 */
async function expectStatusMatchesView(
  page: Page,
  step: string,
  origin: number | null,
  previous: ChartDraw | null,
): Promise<{ origin: number; draw: ChartDraw }> {
  const found: { v: { origin: number; draw: ChartDraw } | null } = { v: null };
  await expect
    .poll(
      async () => {
        const draw = await lastHistoryDraw(page);
        const status = await chartStatus(page).innerText();
        const range = pausedRange(status);
        if (!draw) return `${step}: no history draw yet (status "${status}")`;
        if (previous && draw.t0Ms === previous.t0Ms && draw.t1Ms === previous.t1Ms)
          return `${step}: the view did not change`;
        if (!range) return `${step}: status "${status}" is not "Paused: a to b"`;
        const spanS = (draw.t1Ms - draw.t0Ms) / 1000;
        if (Math.abs(range.to - range.from - spanS) > 2 * SLACK_S)
          return `${step}: status "${status}" spans ${range.to - range.from} s, the view ${spanS} s`;
        const o = draw.t0Ms - range.from * 1000;
        if (origin !== null && Math.abs(o - origin) > SLACK_S * 1000)
          return `${step}: status "${status}" starts at ${range.from} s, the view at ${(draw.t0Ms - origin) / 1000} s`;
        found.v = { origin: origin ?? o, draw };
        return "ok";
      },
      { timeout: 5_000, message: step },
    )
    .toBe("ok");
  if (found.v === null) throw new Error(`${step}: unreachable`);
  return found.v;
}

test("the range status follows every view change", async ({ page }) => {
  test.setTimeout(90_000);
  await open(page, "?synthetic=200000");
  await streamDemo(page);
  await expectHook(page);
  await expect.poll(async () => (await chartSeriesLengths(page)).vbus).toBeGreaterThan(0);
  const status = chartStatus(page);

  // Live window changes: the status names the window now shown.
  await page.getByRole("button", { name: "Whole session" }).click();
  await expect(status).toHaveText("Live: Whole session");
  await expect.poll(async () => (await chartDraws(page)).at(-1)?.t0Ms, { timeout: 30_000 }).toBe(0);
  await page.getByRole("button", { name: "1 min" }).click();
  await expect(status).toHaveText("Live: 1 min");

  // Pause: "Paused: a to b" for the frozen range.
  await liveButton(page).click();
  await expect(liveButton(page)).toHaveText("Paused · Go live");
  const first = await expectStatusMatchesView(page, "pause", null, null);
  const origin = first.origin;
  let draw = first.draw;

  // Wheel zoom over the plot.
  const over = await page.locator(".plot .u-over").boundingBox();
  if (!over) throw new Error("no plot area");
  await page.mouse.move(over.x + over.width / 2, over.y + over.height / 2);
  await page.mouse.wheel(0, -200);
  ({ draw } = await expectStatusMatchesView(page, "wheel zoom", origin, draw));

  // + zooms in.
  await page.mouse.move(over.x + over.width / 2, over.y - 200);
  await blur(page);
  await page.keyboard.press("+");
  ({ draw } = await expectStatusMatchesView(page, "+ key", origin, draw));

  // A window preset while paused: that span, still paused.
  await page.getByRole("button", { name: "1 min" }).click();
  ({ draw } = await expectStatusMatchesView(page, "1 min preset", origin, draw));

  // An overview drag: the status ends on the final view.
  const w = await page.locator(".chart-wrap .overview-window").boundingBox();
  if (!w) throw new Error("no overview window");
  const y = w.y + w.height / 2;
  let x = w.x + w.width / 2;
  await page.mouse.move(x, y);
  await page.mouse.down();
  for (let step = 0; step < 8; step++) {
    x -= 25;
    await page.mouse.move(x, y);
  }
  await page.mouse.up();
  await expectStatusMatchesView(page, "overview drag", origin, draw);

  // Back to live: the live window again.
  await blur(page);
  await page.keyboard.press("End");
  await expect(liveButton(page)).toHaveText("Live");
  await expect(status).toHaveText(/^Live: \S/);
});
