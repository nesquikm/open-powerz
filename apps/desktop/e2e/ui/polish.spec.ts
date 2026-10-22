/**
 * FR AKNVND: chart and layout polish from the QA pass.
 *
 * - AC-AKNVND.1: at DPR 1, VBUS reads blue and the dashed IBUS keeps its colour.
 * - AC-AKNVND.2: the export progress moves neither the rate picker nor Record.
 * - AC-AKNVND.3: at 1600×900, notices never push the overview strip off screen or
 *   shift the main layout.
 * - AC-AKNVND.5: the chosen live window survives a reload.
 * - AC-AKNVND.6: WebKit logs no ResizeObserver loop error at 600 px.
 * - AC-AKNVND.7: at 600 px the device picker has its full label as a tooltip, and the
 *   hover readout stays on one line.
 *
 * AC-AKNVND.4 (the readout's single value) is covered by `chartData.test.ts`.
 */

import { expect, test, type Page } from "@playwright/test";
import { CHART_SERIES, SERIES_VISIBILITY_KEY } from "../../src/lib/chartSeries";
import { THEME_COLORS } from "../../src/lib/theme";
import {
  blur,
  chartDrawsAfterNow,
  chartSeriesLengths,
  open,
  settled,
  streamDemo,
  useTheme,
} from "./helpers";

/** Show only `keys` in the legend before the app loads (session storage, as the app keeps it). */
async function onlySeries(page: Page, keys: string[]): Promise<void> {
  const v = Object.fromEntries(CHART_SERIES.map((s) => [s.key, keys.includes(s.key)]));
  await page.addInitScript(([key, value]) => sessionStorage.setItem(key, value), [
    SERIES_VISIBILITY_KEY,
    JSON.stringify(v),
  ] as const);
}

function hexRgb(hex: string): [number, number, number] {
  const n = Number.parseInt(hex.replace("#", ""), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

/** Pixels of the plot canvas within `tol` (per channel) of `rgb`. */
function countPixelsNear(page: Page, rgb: [number, number, number], tol: number) {
  return page.evaluate(
    ([[r, g, b], t]) => {
      const canvas = document.querySelector<HTMLCanvasElement>(".plot canvas");
      const ctx = canvas?.getContext("2d");
      if (!canvas || !ctx) return -1;
      const { data } = ctx.getImageData(0, 0, canvas.width, canvas.height);
      let n = 0;
      for (let i = 0; i < data.length; i += 4) {
        if (
          Math.abs(data[i]! - r) <= t &&
          Math.abs(data[i + 1]! - g) <= t &&
          Math.abs(data[i + 2]! - b) <= t
        )
          n++;
      }
      return n;
    },
    [rgb, tol] as const,
  );
}

async function streamWithData(page: Page, query = ""): Promise<void> {
  await open(page, query);
  await streamDemo(page);
  await expect.poll(async () => (await chartSeriesLengths(page)).vbus).toBeGreaterThan(20);
}

test.describe("at device pixel ratio 1", () => {
  test.use({ deviceScaleFactor: 1, viewport: { width: 1600, height: 900 } });

  test("VBUS on its 5.0 V grid line reads blue (AC-AKNVND.1)", async ({ page }) => {
    await useTheme(page, "light");
    await onlySeries(page, ["vbus"]);
    await streamWithData(page);
    expect(await page.evaluate(() => window.devicePixelRatio)).toBe(1);
    const blue = hexRgb(THEME_COLORS.light.chart.voltage!);
    // A line across most of the plot: hundreds of pixels close to the VBUS blue,
    // not a near-black blend with the grid line under it.
    await expect
      .poll(() => countPixelsNear(page, blue, 40), { timeout: 5_000 })
      .toBeGreaterThan(400);
  });

  test("the dashed IBUS line keeps its colour (AC-AKNVND.1)", async ({ page }) => {
    await useTheme(page, "light");
    await onlySeries(page, ["ibus"]);
    await streamWithData(page);
    const red = hexRgb(THEME_COLORS.light.chart.current!);
    await expect
      .poll(() => countPixelsNear(page, red, 40), { timeout: 5_000 })
      .toBeGreaterThan(300);
  });
});

test.describe("at 1600×900", () => {
  test.use({ viewport: { width: 1600, height: 900 } });

  test("the export progress moves neither the rate picker nor Record (AC-AKNVND.2)", async ({
    page,
  }) => {
    await streamWithData(page, "?mock=slow-export");
    const rate = page.getByLabel("Sample rate", { exact: true });
    const record = page.getByRole("button", { name: "Record", exact: true });
    const before = { rate: await rate.boundingBox(), record: await record.boundingBox() };
    await page.getByRole("button", { name: "Export", exact: true }).click();
    await expect(page.locator(".export-progress")).toBeVisible();
    const during = { rate: await rate.boundingBox(), record: await record.boundingBox() };
    for (const k of ["rate", "record"] as const) {
      const a = before[k];
      const b = during[k];
      if (!a || !b) throw new Error(`no box for ${k}`);
      expect(Math.abs(b.x - a.x), `${k} x`).toBeLessThanOrEqual(0.5);
      expect(Math.abs(b.y - a.y), `${k} y`).toBeLessThanOrEqual(0.5);
      expect(Math.abs(b.width - a.width), `${k} width`).toBeLessThanOrEqual(0.5);
    }
  });

  test("notices never push the overview strip off screen (AC-AKNVND.3)", async ({ page }) => {
    await streamWithData(page);
    const chart = page.getByRole("region", { name: "Chart" });
    const chartTop = (await chart.boundingBox())?.y ?? NaN;
    const overview = page.locator(".chart-wrap .overview");

    // A saved recording's notice, then an export's notice: two at once.
    await blur(page);
    await page.keyboard.press(" ");
    await expect(page.getByRole("timer")).toBeVisible();
    // A recording with rows in it, so stopping it saves one.
    await expect(page.getByRole("timer")).toContainText(/[1-9][\d,]* rows/);
    await page.getByRole("button", { name: "Stop", exact: true }).click();
    await expect(page.locator(".notice").filter({ hasText: /Recording saved/ })).toBeVisible();
    await page.getByRole("button", { name: "Export", exact: true }).click();
    await expect(page.locator(".notice").filter({ hasText: /Export/i })).toBeVisible();
    await expect(page.locator(".notice")).toHaveCount(2);

    const box = await overview.boundingBox();
    if (!box) throw new Error("no overview strip");
    expect(box.y + box.height, "overview bottom").toBeLessThanOrEqual(900);
    const topNow = (await chart.boundingBox())?.y ?? NaN;
    expect(Math.abs(topNow - chartTop), "the chart region stays put").toBeLessThanOrEqual(1);
  });

  test("the chosen live window survives a reload (AC-AKNVND.5)", async ({ page }) => {
    await streamWithData(page);
    const windows = page.getByRole("group", { name: "Live window" });
    await windows.getByRole("button", { name: "1 min", exact: true }).click();
    await expect(windows.getByRole("button", { name: "1 min", exact: true })).toHaveAttribute(
      "aria-pressed",
      "true",
    );

    await page.reload();
    await open(page);
    await streamDemo(page);
    const after = page.getByRole("group", { name: "Live window" });
    await expect(after.getByRole("button", { name: "1 min", exact: true })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    await expect(after.getByRole("button", { name: "10 s", exact: true })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
  });
});

test.describe("at 600 px", () => {
  test.use({ viewport: { width: 600, height: 800 } });

  test("WebKit logs no ResizeObserver loop error (AC-AKNVND.6)", async ({ page, browserName }) => {
    test.skip(browserName !== "webkit", "the warning is WebKit's");
    const errors: string[] = [];
    page.on("pageerror", (e) => errors.push(e.message));
    page.on("console", (m) => {
      if (m.type() === "error") errors.push(m.text());
    });
    await page.setViewportSize({ width: 900, height: 800 });
    await streamWithData(page);
    await page.setViewportSize({ width: 600, height: 800 });
    // The resize has gone through layout and the chart has redrawn at the new width.
    await settled(page, () => page.locator(".chart-wrap .plot").boundingBox());
    await chartDrawsAfterNow(page, 5);
    await page.reload();
    await streamWithData(page);
    // The reloaded page has laid out at 600 px and kept drawing for a while.
    await settled(page, () => page.locator(".chart-wrap .plot").boundingBox());
    await chartDrawsAfterNow(page, 20);
    expect(errors.filter((e) => /ResizeObserver/i.test(e))).toEqual([]);
  });

  test("the device picker carries its full label as a tooltip (AC-AKNVND.7)", async ({ page }) => {
    await open(page);
    const picker = page.getByLabel("Device", { exact: true });
    const { text, title } = await picker.evaluate((el: HTMLSelectElement) => ({
      text: el.selectedOptions[0]?.text.trim() ?? "",
      title: el.title,
    }));
    expect(text.length).toBeGreaterThan(0);
    expect(title).toBe(text);
  });

  test("the hover readout stays on one line (AC-AKNVND.7)", async ({ page }) => {
    await streamWithData(page);
    const over = page.locator(".plot .u-over");
    const box = await over.boundingBox();
    if (!box) throw new Error("no plot area");
    await page.mouse.move(box.x + box.width * 0.6, box.y + box.height / 2);
    const strip = page.locator(".chart-wrap .hover");
    await expect(strip.locator(".hover-value").first()).toBeVisible();
    const { height, line, top } = await strip.evaluate((el) => {
      const values = [...el.querySelectorAll(".hover-time, .hover-value")].map(
        (c) => c.getBoundingClientRect().top,
      );
      const fontSize = parseFloat(getComputedStyle(el).fontSize);
      return {
        height: el.getBoundingClientRect().height,
        line: fontSize * 1.6,
        top: Math.max(...values) - Math.min(...values),
      };
    });
    expect(top, "every readout item on one row").toBeLessThanOrEqual(1);
    expect(height, "one line high").toBeLessThanOrEqual(line);
  });
});

test.describe("notices leave by themselves (AC-AKNVND.8)", () => {
  test.use({ viewport: { width: 1600, height: 900 } });

  test("a saved-recording notice dismisses itself after 15 s, but not while hovered", async ({
    page,
  }) => {
    await page.clock.install();
    await open(page);
    await streamDemo(page);
    await page.getByRole("button", { name: "Record", exact: true }).click();
    await page.clock.runFor(1_000);
    await page.getByRole("button", { name: "Stop", exact: true }).click();
    const notice = page.locator(".notice-stack .notice").filter({ hasText: "Recording saved" });
    await expect(notice).toBeVisible();
    // Hovered: it stays however long the pointer rests on it.
    await notice.hover();
    await page.clock.runFor(20_000);
    await expect(notice).toBeVisible();
    // Pointer away: the full 15 s again, then it goes.
    await page.mouse.move(10, 400);
    await page.clock.runFor(14_000);
    await expect(notice).toBeVisible();
    await page.clock.runFor(2_000);
    await expect(notice).toHaveCount(0);
  });
});
