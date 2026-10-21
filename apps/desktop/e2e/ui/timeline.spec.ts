/**
 * FR FENCD9, timeline navigation in the browser: the overview strip and its
 * viewport window (AC .1 / .2 / .13), wheel and trackpad zoom and pan (AC .3),
 * click / scrub / middle-drag / Alt-drag / double-click on the plot (AC .4),
 * the playhead readout (AC .5), the editor keys (AC .6 / .7 / .8), the zoom
 * slider (AC .9), the window presets (AC .10) and collapsed long gaps on the
 * outage mock (AC .11).
 *
 * `?synthetic=200000` seeds a 200 s history (one sample a millisecond from
 * t = 0) so the views have somewhere to go; the demo device streams on after it.
 */

import { expect, test, type Page } from "@playwright/test";
import type { ChartDraw } from "../hook";
import { formatSpan } from "../../src/lib/chartSeries";
import { MOCK_OUTAGE } from "../../src/lib/mockBackend";
import { minSpanMs, sliderToSpan } from "../../src/lib/timeline";
import {
  blur,
  chartBreaks,
  chartDraws,
  chartGapBands,
  chartPlayhead,
  chartSeriesLengths,
  chartShuttle,
  lastDraw,
  open,
  overviewBreaks,
  overviewWindow,
  streamDemo,
} from "./helpers";

test.use({ viewport: { width: 1600, height: 900 } });

const SEEDED = 200_000;

function span(d: { t0Ms: number; t1Ms: number }): number {
  return d.t1Ms - d.t0Ms;
}

function liveButton(page: Page) {
  return page.locator(".chart-wrap button.live");
}

/** "Whole session" while live, once its draw reaches back to the seeded start; its last draw. */
async function wholeSessionLive(page: Page): Promise<ChartDraw> {
  await page.getByRole("button", { name: "Whole session" }).click();
  await expect
    .poll(async () => {
      const d = (await chartDraws(page)).at(-1);
      return d?.mode === "live" && d.t0Ms === 0;
    })
    .toBe(true);
  return (await chartDraws(page)).at(-1) as ChartDraw;
}

async function expectPaused(page: Page): Promise<void> {
  await expect(liveButton(page)).toHaveText("Paused · Go live");
  await expect.poll(async () => (await lastDraw(page)).mode).toBe("history");
}

async function expectLive(page: Page): Promise<void> {
  await expect(liveButton(page)).toHaveText("Live");
  await expect.poll(async () => (await lastDraw(page)).mode).toBe("live");
}

/** Streams the demo device on top of a seeded history and waits for live draws. */
async function streamSeeded(page: Page, n = SEEDED): Promise<void> {
  await open(page, `?synthetic=${n}`);
  await streamDemo(page);
  await expect.poll(async () => (await chartSeriesLengths(page)).vbus).toBeGreaterThan(0);
  await expect
    .poll(async () => (await chartDraws(page)).filter((d) => d.mode === "live").length)
    .toBeGreaterThan(2);
}

/** Pauses with the Live button and returns the frozen range once its history draw is in. */
async function pause(page: Page): Promise<ChartDraw> {
  // A view a couple of seconds wide first: a view paused a few ms after connecting is
  // too narrow to drag its edges or zoom into (the narrowest view is 10 ms).
  await expect
    .poll(async () => span(await lastDraw(page)), { timeout: 10_000 })
    .toBeGreaterThan(2_000);
  await liveButton(page).click();
  await expectPaused(page);
  await expect.poll(async () => (await lastDraw(page)).fromHistory).toBe(true);
  return lastDraw(page);
}

async function box(page: Page, selector: string) {
  const b = await page.locator(selector).first().boundingBox();
  if (!b) throw new Error(`no box for ${selector}`);
  return b;
}

/** The plot area (uPlot's overlay). */
function plotBox(page: Page) {
  return box(page, ".plot .u-over");
}

/** Waits for a history draw whose range differs from `prev`, and returns it. */
async function nextRange(page: Page, prev: { t0Ms: number; t1Ms: number }): Promise<ChartDraw> {
  await expect
    .poll(async () => {
      const d = await lastDraw(page);
      return (
        d.mode === "history" && d.fromHistory && (d.t0Ms !== prev.t0Ms || d.t1Ms !== prev.t1Ms)
      );
    })
    .toBe(true);
  return lastDraw(page);
}

/** Session ms under a fraction of the plot width for a gap-free range. */
function timeAt(d: { t0Ms: number; t1Ms: number }, frac: number): number {
  return d.t0Ms + frac * span(d);
}

test.describe("overview strip (AC-FENCD9.1 / AC-FENCD9.2)", () => {
  test("AC-FENCD9.1: the strip sits under the plot and its window follows the live view", async ({
    page,
  }) => {
    await streamSeeded(page);
    const strip = page.locator(".chart-wrap .overview");
    await expect(strip).toBeVisible();
    const plot = await plotBox(page);
    const s = await box(page, ".chart-wrap .overview");
    expect(s.y, "the strip is below the plot").toBeGreaterThanOrEqual(plot.y + plot.height - 1);
    const win = page.locator(".chart-wrap .overview-window");
    await expect(win).toBeVisible();
    // Live on the newest 10 s of a 200 s session: a narrow window at the strip's right end.
    await expect.poll(async () => (await overviewWindow(page)) !== null).toBe(true);
    const first = await overviewWindow(page);
    expect(first!.widthPx).toBeLessThan(s.width / 4);
    expect(first!.leftPx + first!.widthPx).toBeGreaterThan(s.width - 4);
    const firstText = await win.getAttribute("aria-valuetext");
    // New data keeps arriving: the window keeps marking the newest data.
    await expect
      .poll(async () => await win.getAttribute("aria-valuetext"), { timeout: 5_000 })
      .not.toBe(firstText);
    const later = await overviewWindow(page);
    expect(later!.leftPx + later!.widthPx).toBeGreaterThan(s.width - 4);
  });

  test("AC-FENCD9.2: dragging the window pans the chart and pauses it", async ({ page }) => {
    await streamSeeded(page);
    const before = await lastDraw(page);
    const w = await box(page, ".chart-wrap .overview-window");
    await page.mouse.move(w.x + w.width / 2, w.y + w.height / 2);
    await page.mouse.down();
    await page.mouse.move(w.x + w.width / 2 - 300, w.y + w.height / 2, { steps: 5 });
    await page.mouse.up();
    await expectPaused(page);
    const after = await nextRange(page, before);
    expect(after.t1Ms).toBeLessThan(before.t1Ms - 10_000);
    // The span is the one on screen when the drag began: the live window may have
    // grown a little since "before" was read, never shrunk.
    expect(span(after)).toBeGreaterThanOrEqual(span(before) - 1);
    expect(span(after)).toBeLessThan(span(before) + 500);
  });

  test("AC-FENCD9.2: dragging either edge resizes the range", async ({ page }) => {
    await streamSeeded(page);
    const before = await pause(page);
    const left = await box(page, ".chart-wrap .overview-handle.left");
    await page.mouse.move(left.x + left.width / 2, left.y + left.height / 2);
    await page.mouse.down();
    await page.mouse.move(left.x + left.width / 2 - 200, left.y + left.height / 2, { steps: 5 });
    await page.mouse.up();
    const grown = await nextRange(page, before);
    expect(grown.t0Ms).toBeLessThan(before.t0Ms);
    expect(Math.abs(grown.t1Ms - before.t1Ms)).toBeLessThan(span(before) / 20);
    expect(span(grown)).toBeGreaterThan(span(before));

    const right = await box(page, ".chart-wrap .overview-handle.right");
    await page.mouse.move(right.x + right.width / 2, right.y + right.height / 2);
    await page.mouse.down();
    await page.mouse.move(right.x + right.width / 2 - 100, right.y + right.height / 2, {
      steps: 5,
    });
    await page.mouse.up();
    const shrunk = await nextRange(page, grown);
    expect(Math.abs(shrunk.t0Ms - grown.t0Ms)).toBeLessThan(span(grown) / 20);
    expect(shrunk.t1Ms).toBeLessThan(grown.t1Ms);
    await expectPaused(page);
  });

  test("AC-FENCD9.2: a click on the strip outside the window centres the view there", async ({
    page,
  }) => {
    await streamSeeded(page);
    const before = await lastDraw(page);
    const s = await box(page, ".chart-wrap .overview");
    await page.mouse.click(s.x + s.width * 0.25, s.y + s.height / 2);
    await expectPaused(page);
    const after = await nextRange(page, before);
    const centre = (after.t0Ms + after.t1Ms) / 2;
    const session = before.t1Ms;
    expect(centre / session).toBeGreaterThan(0.15);
    expect(centre / session).toBeLessThan(0.35);
    // The span on screen when the click landed: the live window may have grown a
    // little since "before" was read, never shrunk.
    expect(span(after)).toBeGreaterThanOrEqual(span(before) - 1);
    expect(span(after)).toBeLessThan(span(before) + 500);
  });

  test("AC-FENCD9.13: the window is a keyboard slider: arrows move it, Shift+arrows resize it", async ({
    page,
  }) => {
    await streamSeeded(page);
    const win = page.getByRole("slider", { name: "Visible range" });
    await expect(win).toHaveAttribute("tabindex", "0");
    await expect(win).toHaveAttribute("aria-valuetext", /^\d+:\d{2}(\.\d+)? to \d+:\d{2}(\.\d+)?$/);
    const before = await lastDraw(page);
    await win.focus();
    await page.keyboard.press("ArrowLeft");
    await expectPaused(page);
    const moved = await nextRange(page, before);
    expect(moved.t0Ms).toBeLessThan(before.t0Ms);
    // The span on screen when the key landed: the live window may have grown a little
    // since "before" was read, never shrunk.
    expect(span(moved)).toBeGreaterThanOrEqual(span(before) - 1);
    expect(span(moved)).toBeLessThan(span(before) + 500);
    const text = await win.getAttribute("aria-valuetext");
    await page.keyboard.press("Shift+ArrowRight");
    const resized = await nextRange(page, moved);
    expect(Math.abs(span(resized) - span(moved))).toBeGreaterThan(1);
    await expect(win).not.toHaveAttribute("aria-valuetext", text ?? "");
  });
});

test.describe("wheel and trackpad over the plot (AC-FENCD9.3)", () => {
  test("the wheel zooms around the pointer and pauses", async ({ page }) => {
    await streamSeeded(page);
    const before = await pause(page);
    const p = await plotBox(page);
    await page.getByRole("application").focus();
    await page.mouse.move(p.x + p.width * 0.25, p.y + p.height / 2);
    const at = timeAt(before, 0.25);
    await page.mouse.wheel(0, -200);
    const after = await nextRange(page, before);
    expect(span(after)).toBeLessThan(span(before));
    expect((at - after.t0Ms) / span(after)).toBeCloseTo(0.25, 1);
  });

  test("a pinch (Ctrl+wheel) zooms from live and pauses", async ({ page }) => {
    await streamSeeded(page);
    // From the whole-session live view, whose span barely grows while the wheel lands.
    const before = await wholeSessionLive(page);
    const p = await plotBox(page);
    await blur(page);
    await page.mouse.move(p.x + p.width / 2, p.y + p.height / 2);
    await page.keyboard.down("Control");
    await page.mouse.wheel(0, -100);
    await page.keyboard.up("Control");
    await expectPaused(page);
    const after = await nextRange(page, before);
    expect(span(after)).toBeLessThan(span(before));
  });

  test("Shift+wheel pans, keeping the span", async ({ page }) => {
    await streamSeeded(page);
    const before = await pause(page);
    const p = await plotBox(page);
    await page.getByRole("application").focus();
    await page.mouse.move(p.x + p.width / 2, p.y + p.height / 2);
    await page.keyboard.down("Shift");
    await page.mouse.wheel(0, -200);
    await page.keyboard.up("Shift");
    const after = await nextRange(page, before);
    expect(after.t0Ms).toBeLessThan(before.t0Ms);
    expect(span(after)).toBeCloseTo(span(before), -1);
  });

  test("a horizontal trackpad scroll pans from live and pauses", async ({ page }) => {
    await streamSeeded(page);
    // Learn where the session starts (so the pan can reach back), then the 10 s window.
    await wholeSessionLive(page);
    await page.getByRole("button", { name: "10 s" }).click();
    await expect.poll(async () => (await chartDraws(page)).at(-1)?.t0Ms).toBeGreaterThan(0);
    const before = await lastDraw(page);
    const p = await plotBox(page);
    await page.getByRole("application").focus();
    await page.mouse.move(p.x + p.width / 2, p.y + p.height / 2);
    await page.mouse.wheel(-200, 0);
    await expectPaused(page);
    const after = await nextRange(page, before);
    expect(after.t0Ms).toBeLessThan(before.t0Ms);
    // The span is the one on screen when the scroll landed: the live window may have
    // grown since "before" was read (up to a couple of seconds on a slow runner),
    // never shrunk.
    expect(span(after)).toBeGreaterThanOrEqual(span(before) - 1);
    expect(span(after)).toBeLessThan(span(before) + 2_500);
  });
});

test.describe("pointer on the plot (AC-FENCD9.4 / AC-FENCD9.5)", () => {
  test("AC-FENCD9.4: a click places the playhead and a plain drag scrubs it, without zooming", async ({
    page,
  }) => {
    await streamSeeded(page);
    const view = await pause(page);
    const p = await plotBox(page);
    await page.mouse.click(p.x + p.width * 0.4, p.y + p.height / 2);
    await expect.poll(() => chartPlayhead(page)).not.toBeNull();
    expect(((await chartPlayhead(page))! - timeAt(view, 0.4)) / span(view)).toBeCloseTo(0, 1);

    await page.mouse.move(p.x + p.width * 0.3, p.y + p.height / 2);
    await page.mouse.down();
    await page.mouse.move(p.x + p.width * 0.7, p.y + p.height / 2, { steps: 8 });
    await page.mouse.up();
    expect(((await chartPlayhead(page))! - timeAt(view, 0.7)) / span(view)).toBeCloseTo(0, 1);
    const still = await lastDraw(page);
    expect(still.t0Ms, "a plain drag no longer zooms").toBe(view.t0Ms);
    expect(still.t1Ms).toBe(view.t1Ms);
  });

  test("AC-FENCD9.4: a middle-button drag pans", async ({ page }) => {
    await streamSeeded(page);
    const before = await pause(page);
    const p = await plotBox(page);
    await page.mouse.move(p.x + p.width * 0.3, p.y + p.height / 2);
    await page.mouse.down({ button: "middle" });
    await page.mouse.move(p.x + p.width * 0.6, p.y + p.height / 2, { steps: 5 });
    await page.mouse.up({ button: "middle" });
    const after = await nextRange(page, before);
    expect(after.t0Ms).toBeLessThan(before.t0Ms);
    expect(span(after)).toBeCloseTo(span(before), -1);
  });

  test("AC-FENCD9.4: an Alt-drag zooms to the dragged range", async ({ page }) => {
    await streamSeeded(page);
    // A view wide enough that a fifth of it is above the narrowest zoom (10 ms).
    await expect
      .poll(async () => span(await lastDraw(page)), { timeout: 10_000 })
      .toBeGreaterThan(2_000);
    const before = await pause(page);
    const p = await plotBox(page);
    await page.keyboard.down("Alt");
    await page.mouse.move(p.x + p.width * 0.2, p.y + p.height / 2);
    await page.mouse.down();
    await page.mouse.move(p.x + p.width * 0.4, p.y + p.height / 2, { steps: 5 });
    await page.mouse.up();
    await page.keyboard.up("Alt");
    const after = await nextRange(page, before);
    expect((after.t0Ms - timeAt(before, 0.2)) / span(before)).toBeCloseTo(0, 1);
    expect((after.t1Ms - timeAt(before, 0.4)) / span(before)).toBeCloseTo(0, 1);
  });

  test("AC-FENCD9.4: a double-click fits the whole session", async ({ page }) => {
    await streamSeeded(page);
    await pause(page);
    const p = await plotBox(page);
    await page.mouse.dblclick(p.x + p.width / 2, p.y + p.height / 2);
    await expectLive(page);
    await expect(page.getByRole("button", { name: "Whole session" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    await expect.poll(async () => (await lastDraw(page)).t0Ms).toBeLessThanOrEqual(1);
  });

  test("AC-FENCD9.5: the readout shows the pointer over the plot, else the playhead", async ({
    page,
  }) => {
    await streamSeeded(page);
    await pause(page);
    const p = await plotBox(page);
    const time = page.locator(".chart-wrap .hover .hover-time");
    await page.mouse.click(p.x + p.width * 0.3, p.y + p.height / 2);
    await expect.poll(() => chartPlayhead(page)).not.toBeNull();
    const atPlayhead = await time.innerText();
    await page.mouse.move(p.x + p.width * 0.8, p.y + p.height / 2);
    await expect(time).not.toHaveText(atPlayhead);
    // Off the plot: the playhead's time and every visible series' value.
    await page.mouse.move(p.x + p.width / 2, p.y - 200);
    await expect(time).toHaveText(atPlayhead);
    await expect(page.locator(".chart-wrap .hover .hover-value", { hasText: "VBUS" })).toHaveCount(
      1,
    );
    await expect(page.locator(".chart-wrap .hover .hover-value", { hasText: "IBUS" })).toHaveCount(
      1,
    );
  });
});

test.describe("editor keys (AC-FENCD9.6 / .7 / .8)", () => {
  test("AC-FENCD9.6: ← / → step the playhead a pixel, Shift a tenth of the view, and the view follows", async ({
    page,
  }) => {
    await streamSeeded(page);
    const view = await pause(page);
    const p = await plotBox(page);
    await page.mouse.click(p.x + p.width * 0.5, p.y + p.height / 2);
    await page.mouse.move(p.x + p.width / 2, p.y - 200);
    await expect.poll(() => chartPlayhead(page)).not.toBeNull();
    const start = (await chartPlayhead(page))!;
    await blur(page);
    await page.keyboard.press("ArrowRight");
    const onePx = (await chartPlayhead(page))! - start;
    expect(onePx).toBeGreaterThan(0);
    expect(onePx).toBeLessThanOrEqual((span(view) / p.width) * 1.5);
    await page.keyboard.press("Shift+ArrowLeft");
    expect(start + onePx - (await chartPlayhead(page))!).toBeCloseTo(span(view) / 10, -1);
    // Ten coarse steps back from the middle leave the view: it follows.
    for (let i = 0; i < 10; i++) await page.keyboard.press("Shift+ArrowLeft");
    const followed = await nextRange(page, view);
    const head = (await chartPlayhead(page))!;
    expect(head).toBeGreaterThanOrEqual(followed.t0Ms);
    expect(head).toBeLessThanOrEqual(followed.t1Ms);
    expect(followed.t0Ms).toBeLessThan(view.t0Ms);
  });

  test("AC-FENCD9.7: J / K / L shuttle at 1×, 2×, 4×, 8×, reverse at 1×, and K stops", async ({
    page,
  }) => {
    await streamSeeded(page);
    // Home goes to the session start only once the chart knows it (the seeded history).
    await wholeSessionLive(page);
    await blur(page);
    await page.keyboard.press("Home");
    await expectPaused(page);
    await blur(page);
    await page.keyboard.press("l");
    expect(await chartShuttle(page)).toBe(1);
    const p0 = await chartPlayhead(page);
    await expect
      .poll(async () => (await chartPlayhead(page)) ?? -1)
      .toBeGreaterThan((p0 ?? 0) + 200);
    await page.keyboard.press("l");
    expect(await chartShuttle(page)).toBe(2);
    await page.keyboard.press("l");
    await page.keyboard.press("l");
    expect(await chartShuttle(page)).toBe(8);
    await page.keyboard.press("l");
    expect(await chartShuttle(page)).toBe(8);
    await page.keyboard.press("j");
    expect(await chartShuttle(page)).toBe(-1);
    await page.keyboard.press("j");
    expect(await chartShuttle(page)).toBe(-2);
    await page.keyboard.press("k");
    expect(await chartShuttle(page)).toBe(0);
    const stopped = await chartPlayhead(page);
    await page.waitForTimeout(300);
    expect(await chartPlayhead(page)).toBe(stopped);
    await expectPaused(page);
  });

  test("AC-FENCD9.7: playing forwards into the newest data returns to live; L alone does not", async ({
    page,
  }) => {
    await streamSeeded(page);
    // A view a couple of seconds wide, so the playhead starts well behind the newest
    // sample (batches land up to 50 ms apart; a 1× playhead must not overtake them).
    await expect
      .poll(async () => span(await lastDraw(page)), { timeout: 10_000 })
      .toBeGreaterThan(2_000);
    await pause(page);
    const p = await plotBox(page);
    await page.mouse.click(p.x + p.width * 0.5, p.y + p.height / 2);
    await page.mouse.move(p.x + p.width / 2, p.y - 200);
    await expect.poll(() => chartPlayhead(page)).not.toBeNull();
    await blur(page);
    // At 1× the playhead keeps pace with the stream: still paused a moment later.
    await page.keyboard.press("l");
    await page.waitForTimeout(500);
    await expect(liveButton(page)).toHaveText("Paused · Go live");
    // At 8× it catches up with the newest data and the chart goes live.
    await page.keyboard.press("l");
    await page.keyboard.press("l");
    await page.keyboard.press("l");
    await expect(liveButton(page)).toHaveText("Live", { timeout: 10_000 });
    expect(await chartShuttle(page)).toBe(0);
  });

  test("AC-FENCD9.8: + / = / − zoom around the playhead; Home and Shift+Z", async ({ page }) => {
    await streamSeeded(page);
    const view = await pause(page);
    const p = await plotBox(page);
    await page.mouse.click(p.x + p.width * 0.25, p.y + p.height / 2);
    await page.mouse.move(p.x + p.width / 2, p.y - 200);
    await expect.poll(() => chartPlayhead(page)).not.toBeNull();
    const head = (await chartPlayhead(page))!;
    const frac = (head - view.t0Ms) / span(view);
    await blur(page);
    await page.keyboard.press("+");
    const zin = await nextRange(page, view);
    expect(span(zin) / span(view)).toBeCloseTo(0.8, 2);
    expect((head - zin.t0Ms) / span(zin)).toBeCloseTo(frac, 2);
    await page.keyboard.press("=");
    const zin2 = await nextRange(page, zin);
    expect(span(zin2) / span(zin)).toBeCloseTo(0.8, 2);
    await page.keyboard.press("-");
    const zout = await nextRange(page, zin2);
    expect(span(zout) / span(zin2)).toBeCloseTo(1 / 0.8, 2);

    await page.keyboard.press("Home");
    const home = await nextRange(page, zout);
    expect(home.t0Ms).toBeLessThanOrEqual(1);
    expect(span(home)).toBeCloseTo(span(zout), -1);
    await expectPaused(page);

    await page.keyboard.press("Shift+Z");
    await expectLive(page);
    await expect(page.getByRole("button", { name: "Whole session" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    await expect.poll(async () => (await lastDraw(page)).t0Ms).toBeLessThanOrEqual(1);
  });

  test("AC-FENCD9.8: + zooms around the view centre without a playhead", async ({ page }) => {
    await streamSeeded(page);
    const view = await pause(page);
    await blur(page);
    await page.keyboard.press("+");
    const zin = await nextRange(page, view);
    expect(((zin.t0Ms + zin.t1Ms) / 2 - (view.t0Ms + view.t1Ms) / 2) / span(view)).toBeCloseTo(
      0,
      2,
    );
  });
});

test.describe("zoom slider and presets (AC-FENCD9.9 / AC-FENCD9.10)", () => {
  test("AC-FENCD9.9: the zoom slider shows and sets the span on a log scale", async ({ page }) => {
    await streamSeeded(page);
    const slider = page.getByRole("slider", { name: "Zoom" });
    await expect(slider).toHaveClass(/zoom-slider/);
    await expect(slider).toHaveAttribute("type", "range");
    await expect(slider).toHaveAttribute("min", "0");
    await expect(slider).toHaveAttribute("max", "1000");
    // The slider's top end is the whole session: learn it first, then back to 10 s.
    await wholeSessionLive(page);
    await page.getByRole("button", { name: "10 s" }).click();
    await expect.poll(async () => (await lastDraw(page)).t0Ms).toBeGreaterThan(0);
    const sessionMs = (await lastDraw(page)).t1Ms;
    // The slider's bottom end: two sample periods at the demo's 50 SPS (AC-9CFMD3.4).
    const floor = minSpanMs(50);
    // 10 s of a ~200 s session.
    const expected = (1000 * Math.log(10_000 / floor)) / Math.log(sessionMs / floor);
    expect(Math.abs(Number(await slider.inputValue()) - expected)).toBeLessThanOrEqual(15);

    const before = await lastDraw(page);
    await slider.fill("300");
    await expectPaused(page);
    const after = await nextRange(page, before);
    const want = floor * Math.pow(sessionMs / floor, 0.3);
    expect(span(after) / want).toBeGreaterThan(0.9);
    expect(span(after) / want).toBeLessThan(1.1);
  });

  test("AC-FENCD9.10: live presets follow the newest data", async ({ page }) => {
    await streamSeeded(page);
    await page.getByRole("button", { name: "1 min" }).click();
    await expectLive(page);
    await expect(page.getByRole("button", { name: "1 min" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    // The live buffer holds what streamed since connect, at most the preset's span.
    expect(span(await lastDraw(page))).toBeLessThanOrEqual(61_000);
    const t1 = (await lastDraw(page)).t1Ms;
    await expect.poll(async () => (await lastDraw(page)).t1Ms).toBeGreaterThan(t1 + 300);
  });

  test("AC-FENCD9.10: paused presets keep the playhead in view and stay paused", async ({
    page,
  }) => {
    await streamSeeded(page);
    await blur(page);
    await page.keyboard.press("Home");
    await expectPaused(page);
    const view = await lastDraw(page);
    const p = await plotBox(page);
    await page.mouse.click(p.x + p.width * 0.8, p.y + p.height / 2);
    await expect.poll(() => chartPlayhead(page)).not.toBeNull();
    const head = (await chartPlayhead(page))!;

    await page.getByRole("button", { name: "1 min" }).click();
    const oneMin = await nextRange(page, view);
    await expect(liveButton(page)).toHaveText("Paused · Go live");
    expect(span(oneMin)).toBeCloseTo(60_000, -2);
    expect(head).toBeGreaterThanOrEqual(oneMin.t0Ms);
    expect(head).toBeLessThanOrEqual(oneMin.t1Ms);

    await page.getByRole("button", { name: "10 s" }).click();
    const tenS = await nextRange(page, oneMin);
    await expect(liveButton(page)).toHaveText("Paused · Go live");
    expect(span(tenS)).toBeCloseTo(10_000, -2);
    expect(head).toBeGreaterThanOrEqual(tenS.t0Ms);
    expect(head).toBeLessThanOrEqual(tenS.t1Ms);

    await page.getByRole("button", { name: "Whole session" }).click();
    const whole = await nextRange(page, tenS);
    await expect(liveButton(page)).toHaveText("Paused · Go live");
    expect(whole.t0Ms).toBeLessThanOrEqual(1);
    expect(span(whole)).toBeGreaterThan(SEEDED - 1_000);
  });
});

test("AC-FENCD9.11: the 40-minute outage collapses to a labelled break of at most 32 px", async ({
  page,
}) => {
  await open(page, "?mock=outage");
  await streamDemo(page);
  await page.getByRole("button", { name: "Whole session" }).click();
  await expect
    .poll(async () => (await lastDraw(page)).t0Ms, { timeout: 5_000 })
    .toBe(MOCK_OUTAGE.startMs);
  await expect.poll(() => chartBreaks(page)).toHaveLength(1);
  const [brk] = await chartBreaks(page);
  const gapTo = MOCK_OUTAGE.startMs + MOCK_OUTAGE.beforeMs + MOCK_OUTAGE.gapMs;
  expect(brk!.toMs).toBe(gapTo);
  // The gap starts at the last sample before the outage: one 50 SPS step (20 ms) early.
  expect(brk!.fromMs).toBeGreaterThanOrEqual(MOCK_OUTAGE.startMs + MOCK_OUTAGE.beforeMs - 25);
  expect(brk!.widthPx).toBeGreaterThan(0);
  expect(brk!.widthPx).toBeLessThanOrEqual(32);
  // Still labelled with its true length.
  const [band] = await chartGapBands(page);
  expect(band?.label).toBe("No data · 40:00");
  // The data before the outage fills a real share of the plot, not a sliver.
  const p = await plotBox(page);
  await page.mouse.move(p.x + p.width * 0.25, p.y + p.height / 2);
  await expect(page.locator(".chart-wrap .hover .hover-time")).toHaveText(/^\d+\.\d{3} s$/);
});

test("AC-FENCD9.11: the overview strip collapses the outage too, marked and labelled", async ({
  page,
}) => {
  await open(page, "?mock=outage");
  await streamDemo(page);
  await expect.poll(() => overviewBreaks(page), { timeout: 5_000 }).toHaveLength(1);
  const [brk] = await overviewBreaks(page);
  expect(brk!.widthPx).toBeGreaterThan(0);
  expect(brk!.widthPx).toBeLessThanOrEqual(32);
  expect(brk!.label).toContain("40:00");
  // The data on both sides keeps most of the strip.
  const strip = await box(page, ".chart-wrap .overview");
  expect(brk!.widthPx).toBeLessThan(strip.width / 4);
});

test.describe("review fixes (FR FENCD9)", () => {
  test("AC-FENCD9.7: a backward shuttle stops at the session start", async ({ page }) => {
    await streamSeeded(page);
    // Learn the session start, then a 10 s view, so Home shows the first 10 s.
    await wholeSessionLive(page);
    await page.getByRole("button", { name: "10 s" }).click();
    // A live view a couple of seconds wide first: Home keeps the span on screen, and a
    // view a few ms after connecting is too narrow to click a playhead into.
    await expect
      .poll(
        async () => {
          // The 10 s window's own draw: the whole-session draw before it also starts at 0.
          const d = await lastDraw(page);
          return d.mode === "live" && d.t0Ms > 0 ? span(d) : 0;
        },
        { timeout: 10_000 },
      )
      .toBeGreaterThan(2_000);
    await blur(page);
    await page.keyboard.press("Home");
    await expectPaused(page);
    const p = await plotBox(page);
    // The playhead a few hundred ms after the session start, whatever the view's span.
    const home = await lastDraw(page);
    await page.mouse.click(p.x + p.width * (300 / span(home)), p.y + p.height / 2);
    await page.mouse.move(p.x + p.width / 2, p.y - 200);
    await expect.poll(() => chartPlayhead(page)).not.toBeNull();
    await blur(page);
    await page.keyboard.press("j");
    expect(await chartShuttle(page)).toBe(-1);
    await expect.poll(() => chartShuttle(page), { timeout: 5_000 }).toBe(0);
    expect(await chartPlayhead(page)).toBeLessThanOrEqual(1);
    await expectPaused(page);
  });

  test("a new session forgets the playhead and the readout", async ({ page }) => {
    await streamSeeded(page);
    await pause(page);
    const p = await plotBox(page);
    await page.mouse.click(p.x + p.width * 0.4, p.y + p.height / 2);
    await page.mouse.move(p.x + p.width / 2, p.y - 200);
    await expect.poll(() => chartPlayhead(page)).not.toBeNull();
    await page.getByRole("button", { name: "Disconnect" }).click();
    await expect(page.locator(".badge")).toHaveText("Disconnected");
    await streamDemo(page);
    await expect.poll(() => chartPlayhead(page)).toBeNull();
    await expect(page.locator(".chart-wrap .hover .hover-time")).toHaveCount(0);
  });

  test("AC-FENCD9.4: a scrub ends when the window loses focus mid-drag", async ({ page }) => {
    await streamSeeded(page);
    const view = await pause(page);
    const p = await plotBox(page);
    await page.mouse.move(p.x + p.width * 0.3, p.y + p.height / 2);
    await page.mouse.down();
    await expect.poll(() => chartPlayhead(page)).not.toBeNull();
    const at = (await chartPlayhead(page))!;
    expect((at - timeAt(view, 0.3)) / span(view)).toBeCloseTo(0, 1);
    await page.evaluate(() => window.dispatchEvent(new Event("blur")));
    await page.mouse.move(p.x + p.width * 0.7, p.y + p.height / 2, { steps: 4 });
    expect(await chartPlayhead(page)).toBe(at);
    await page.mouse.up();
  });

  test("AC-FENCD9.4: a pan ends when a move arrives with no button held (a lost mouseup)", async ({
    page,
  }) => {
    await streamSeeded(page);
    const before = await pause(page);
    const p = await plotBox(page);
    const y = p.y + p.height / 2;
    await page.mouse.move(p.x + p.width * 0.3, y);
    await page.mouse.down({ button: "middle" });
    await page.evaluate(
      ([x, cy]) =>
        window.dispatchEvent(
          new MouseEvent("mousemove", { clientX: x, clientY: cy, buttons: 0, bubbles: true }),
        ),
      [p.x + p.width * 0.31, y] as const,
    );
    await page.mouse.move(p.x + p.width * 0.6, y, { steps: 5 });
    await page.waitForTimeout(300);
    const after = await lastDraw(page);
    expect(after.t0Ms).toBe(before.t0Ms);
    expect(after.t1Ms).toBe(before.t1Ms);
    await page.mouse.up({ button: "middle" });
  });

  test("AC-FENCD9.3: the wheel zooms around the pointer even over a cursor element", async ({
    page,
  }) => {
    await streamSeeded(page);
    const before = await pause(page);
    const p = await plotBox(page);
    const x = p.x + p.width * 0.75;
    const y = p.y + p.height / 2;
    await page.mouse.move(x, y);
    // The event lands on uPlot's cursor line, a child of the plot area, not on the area.
    await page.locator(".plot .u-over .u-cursor-x").dispatchEvent("wheel", {
      clientX: x,
      clientY: y,
      deltaY: -200,
      bubbles: true,
      cancelable: true,
    });
    const after = await nextRange(page, before);
    expect(span(after)).toBeLessThan(span(before));
    expect((timeAt(before, 0.75) - after.t0Ms) / span(after)).toBeCloseTo(0.75, 1);
  });

  test("AC-FENCD9.9: the zoom slider says the span it shows", async ({ page }) => {
    await streamSeeded(page);
    const slider = page.getByRole("slider", { name: "Zoom" });
    await expect(slider).toHaveAttribute("aria-valuetext", "10 s");
    await page.getByRole("button", { name: "1 min" }).click();
    await expect(slider).toHaveAttribute("aria-valuetext", "1:00");
  });

  test("AC-FENCD9.9: a visible readout beside the zoom slider shows the span, with a tooltip", async ({
    page,
  }) => {
    await streamSeeded(page);
    const slider = page.getByRole("slider", { name: "Zoom" });
    const readout = page.locator(".chart-wrap .zoom-span");
    await expect(readout).toBeVisible();
    await expect(readout).toHaveText("10 s");
    await expect(slider).toHaveAttribute("title", "Zoom: visible span");
    await expect(readout).toHaveAttribute("title", "Zoom: visible span");
    // It sits right next to the slider.
    const r = await readout.boundingBox();
    const sl = await slider.boundingBox();
    if (!r || !sl) throw new Error("no box");
    expect(Math.abs(r.y + r.height / 2 - (sl.y + sl.height / 2))).toBeLessThan(sl.height);
    expect(
      Math.min(Math.abs(r.x - (sl.x + sl.width)), Math.abs(sl.x - (r.x + r.width))),
    ).toBeLessThan(24);
    // It follows the view and matches what a screen reader hears.
    await page.getByRole("button", { name: "Whole session" }).click();
    await expect(readout).toHaveText(/^\d+:\d{2}$/);
    await expect(slider).toHaveAttribute("aria-valuetext", (await readout.textContent()) ?? "");
  });

  test("a mouse click focuses the plot or the strip window without a ring; Tab focus shows it", async ({
    page,
  }) => {
    await streamSeeded(page);
    await pause(page);
    const plot = page.getByRole("application");
    const p = await plotBox(page);
    await blur(page);
    await page.mouse.click(p.x + p.width * 0.5, p.y + p.height / 2);
    await expect(plot).toBeFocused();
    await expect(plot).toHaveCSS("outline-style", "none");
    // Keys pressed after the click (zoom, step) still show no ring: the focus came
    // from the mouse.
    await page.keyboard.press("+");
    await page.keyboard.press("ArrowRight");
    await expect(plot).toHaveCSS("outline-style", "none");
    // Tabbing into it shows the ring at once.
    await blur(page);
    await page.getByRole("button", { name: /CC2/ }).focus();
    await page.keyboard.press("Tab");
    await expect(plot).toBeFocused();
    await expect(plot).not.toHaveCSS("outline-style", "none");
    // The strip's window the same way.
    const win = page.getByRole("slider", { name: "Visible range" });
    const w = await box(page, ".chart-wrap .overview-window");
    await page.mouse.click(w.x + w.width / 2, w.y + w.height / 2);
    await expect(win).toBeFocused();
    await page.keyboard.press("ArrowLeft");
    await expect(win).toHaveCSS("outline-style", "none");
  });

  test("a pointer press on the plot or the strip moves focus to its keyboard control", async ({
    page,
  }) => {
    await streamSeeded(page);
    await pause(page);
    const p = await plotBox(page);
    await blur(page);
    await page.mouse.click(p.x + p.width * 0.5, p.y + p.height / 2);
    await expect(page.getByRole("application")).toBeFocused();
    const win = page.getByRole("slider", { name: "Visible range" });
    await blur(page);
    const w = await box(page, ".chart-wrap .overview-window");
    await page.mouse.click(w.x + w.width / 2, w.y + w.height / 2);
    await expect(win).toBeFocused();
    await blur(page);
    const s = await box(page, ".chart-wrap .overview");
    await page.mouse.click(s.x + s.width * 0.1, s.y + s.height / 2);
    await expect(win).toBeFocused();
  });

  test("AC-FENCD9.13: the window slider's value runs from 0 to the last start it can take", async ({
    page,
  }) => {
    await streamSeeded(page);
    const win = page.getByRole("slider", { name: "Visible range" });
    // Live, the window ends at the newest data: its start is as late as it can be.
    const read = async () => ({
      now: Number(await win.getAttribute("aria-valuenow")),
      max: Number(await win.getAttribute("aria-valuemax")),
    });
    await expect
      .poll(async () => span(await lastDraw(page)), { timeout: 10_000 })
      .toBeGreaterThan(2_000);
    const live = await read();
    expect(Math.abs(live.max - live.now)).toBeLessThanOrEqual(0.3);
    // Paused, the start in tenths of a second.
    const view = await pause(page);
    await expect
      .poll(async () => Math.abs((await read()).now - view.t0Ms / 1000))
      .toBeLessThanOrEqual(0.051);
  });
});

test.describe("zoom on the chart as drawn (AC-FENCD9.14)", () => {
  /** The outage mock, its whole session paused; the draw and the outage's exact span. */
  async function wholeOutagePaused(
    page: Page,
  ): Promise<{ whole: ChartDraw; gap: { fromMs: number; toMs: number } }> {
    await open(page, "?mock=outage");
    await streamDemo(page);
    await page.getByRole("button", { name: "Whole session" }).click();
    await expect
      .poll(async () => (await lastDraw(page)).t0Ms, { timeout: 5_000 })
      .toBe(MOCK_OUTAGE.startMs);
    await expect.poll(() => chartBreaks(page)).toHaveLength(1);
    // Some data after the outage too, so the right end of the plot is data, not the gap.
    const gapTo = MOCK_OUTAGE.startMs + MOCK_OUTAGE.beforeMs + MOCK_OUTAGE.gapMs;
    await expect
      .poll(async () => (await lastDraw(page)).t1Ms, { timeout: 15_000 })
      .toBeGreaterThan(gapTo + 6_000);
    const whole = await pause(page);
    expect(whole.t0Ms).toBe(MOCK_OUTAGE.startMs);
    const [brk] = await chartBreaks(page);
    return { whole, gap: { fromMs: brk!.fromMs, toMs: brk!.toMs } };
  }

  /** Data time of a drawn range: its span less the part of the outage inside it. */
  function dataMs(d: { t0Ms: number; t1Ms: number }, gap: { fromMs: number; toMs: number }) {
    const cut = Math.max(0, Math.min(d.t1Ms, gap.toMs) - Math.max(d.t0Ms, gap.fromMs));
    return span(d) - cut;
  }

  async function expectBreaksNarrow(page: Page): Promise<void> {
    for (const b of await chartBreaks(page)) expect(b.widthPx).toBeLessThanOrEqual(32);
  }

  async function expectTrueSpanReadout(page: Page, d: ChartDraw): Promise<void> {
    const text = formatSpan(span(d));
    await expect(page.locator(".chart-wrap .zoom-span")).toHaveText(text);
    await expect(page.getByRole("slider", { name: "Zoom" })).toHaveAttribute(
      "aria-valuetext",
      text,
    );
  }

  test("two + presses around a playhead near the end keep the data on screen", async ({ page }) => {
    const { whole, gap } = await wholeOutagePaused(page);
    const p = await plotBox(page);
    await page.mouse.click(p.x + p.width * 0.95, p.y + p.height / 2);
    await page.mouse.move(p.x + p.width / 2, p.y - 200);
    await expect.poll(() => chartPlayhead(page)).not.toBeNull();
    const head = (await chartPlayhead(page))!;
    await blur(page);
    await page.keyboard.press("+");
    const zin = await nextRange(page, whole);
    await page.keyboard.press("+");
    const zin2 = await nextRange(page, zin);
    const ratio = dataMs(zin2, gap) / dataMs(whole, gap);
    expect(ratio).toBeGreaterThan(0.64 * 0.85);
    expect(ratio).toBeLessThan(0.64 * 1.15);
    expect(head).toBeGreaterThanOrEqual(zin2.t0Ms);
    expect(head).toBeLessThanOrEqual(zin2.t1Ms);
    await expectBreaksNarrow(page);
    await expectTrueSpanReadout(page, zin2);
  });

  test("a wheel notch near the end scales the data on screen by the step", async ({ page }) => {
    const { whole, gap } = await wholeOutagePaused(page);
    const p = await plotBox(page);
    await page.getByRole("application").focus();
    await page.mouse.move(p.x + p.width * 0.95, p.y + p.height / 2);
    await page.mouse.wheel(0, -100);
    const after = await nextRange(page, whole);
    const ratio = dataMs(after, gap) / dataMs(whole, gap);
    expect(ratio).toBeGreaterThan(0.8 * 0.85);
    expect(ratio).toBeLessThan(0.8 * 1.15);
    await expectBreaksNarrow(page);
    await expectTrueSpanReadout(page, after);
  });

  test("the zoom slider zooms the displayed time, not the collapsed gap", async ({ page }) => {
    const { whole, gap } = await wholeOutagePaused(page);
    const [brk] = await chartBreaks(page);
    const p = await plotBox(page);
    // The whole session as drawn: its data plus the break's share of the plot.
    const displayedMs = dataMs(whole, gap) / (1 - brk!.widthPx / p.width);
    const slider = page.getByRole("slider", { name: "Zoom" });
    await slider.fill("950");
    const after = await nextRange(page, whole);
    const want = sliderToSpan(0.95, displayedMs) / displayedMs;
    const ratio = dataMs(after, gap) / dataMs(whole, gap);
    expect(ratio).toBeGreaterThan(want * 0.8);
    expect(ratio).toBeLessThan(Math.min(1, want * 1.2));
    await expectBreaksNarrow(page);
    await expectTrueSpanReadout(page, after);
  });
});
