/**
 * FR SX3EDV: M2 final polish.
 *
 * - AC-SX3EDV.1: starting an export moves no top-bar item at 900–1600 px; the progress
 *   fills the disabled Export button's own box and carries the count in its tooltip
 *   and its status text.
 * - AC-SX3EDV.2: with Export in the "⋯" menu, a running export is a thin line along the
 *   bar's bottom edge, and the bar's items keep their places.
 * - AC-SX3EDV.3: below 1200 px, notices sit between the chart card and the Stats panel,
 *   cover no reading, chart or statistic, and leave the chart card where it was.
 * - AC-SX3EDV.4: at 1200 px and wider, notices float over the side panel (AC-AKNVND.3).
 * - AC-SX3EDV.6: the overview strip draws sparse series as a continuous line at 10 and
 *   50 SPS and after a rate change, leaving every marked gap open.
 * - AC-SX3EDV.7: the disconnect banner sits in the line above the plot, covering no axis
 *   or trace, and the hover readout takes that line while the pointer is on the plot.
 *
 * AC-SX3EDV.5 is covered by `chartView.test.ts`, the strip's column bridging also by
 * `stripEnvelope.test.ts`, AC-SX3EDV.8 by the Rust test in `src-tauri/src/activity.rs`.
 */

import { expect, test, type Locator, type Page } from "@playwright/test";
import {
  blur,
  chartSeriesLengths,
  open,
  overviewBreaks,
  streamDemo,
  useTheme,
  type Theme,
} from "./helpers";

interface Box {
  x: number;
  y: number;
  width: number;
  height: number;
}

interface BarItem extends Box {
  label: string;
}

async function streamWithData(page: Page, query = ""): Promise<void> {
  await open(page, query);
  await streamDemo(page);
  await expect.poll(async () => (await chartSeriesLengths(page)).vbus).toBeGreaterThan(20);
}

async function boxOf(locator: Locator, name: string): Promise<Box> {
  const box = await locator.boundingBox();
  if (!box) throw new Error(`no box for ${name}`);
  return box;
}

function intersects(a: Box, b: Box): boolean {
  return (
    a.x < b.x + b.width - 0.5 &&
    b.x < a.x + a.width - 0.5 &&
    a.y < b.y + b.height - 0.5 &&
    b.y < a.y + a.height - 0.5
  );
}

function expectSameBox(a: Box, b: Box, what: string, tol = 0.5): void {
  expect(Math.abs(b.x - a.x), `${what} x`).toBeLessThanOrEqual(tol);
  expect(Math.abs(b.y - a.y), `${what} y`).toBeLessThanOrEqual(tol);
  expect(Math.abs(b.width - a.width), `${what} width`).toBeLessThanOrEqual(tol);
  expect(Math.abs(b.height - a.height), `${what} height`).toBeLessThanOrEqual(tol);
}

/** Every visible item of the top bar outside the "⋯" menu, the export progress excluded. */
function topBarItems(page: Page): Promise<BarItem[]> {
  return page.evaluate(() => {
    const bar = document.querySelector(".top-bar");
    if (!bar) return [];
    const selector = ".brand, select, button, .badge, .rate-note, .rec-indicator";
    return [...bar.querySelectorAll<HTMLElement>(selector)]
      .filter((el) => !el.closest(".menu") && !el.closest(".export-progress"))
      .filter((el) => el.getClientRects().length > 0 && el.offsetWidth > 0)
      .map((el) => {
        const r = el.getBoundingClientRect();
        const label = (el.getAttribute("aria-label") ?? el.innerText).replace(/\s+/g, " ").trim();
        return { label, x: r.left, y: r.top, width: r.width, height: r.height };
      });
  });
}

function expectItemsUnmoved(before: BarItem[], during: BarItem[]): void {
  expect(
    during.map((i) => i.label),
    "the same items are visible",
  ).toEqual(before.map((i) => i.label));
  before.forEach((a, i) => expectSameBox(a, during[i]!, `"${a.label}"`));
}

/** The export's status text and the tooltips of the progress and what holds it. */
function exportTexts(page: Page): Promise<{ status: string; tooltips: string[] }> {
  return page.evaluate(() => {
    const bar = document.querySelector(".top-bar");
    const progress = bar?.querySelector<HTMLElement>('[aria-label="Export progress"]') ?? null;
    const statuses = [...(bar?.querySelectorAll<HTMLElement>('[role="status"]') ?? [])]
      .map((el) => (el.textContent ?? "").replace(/\s+/g, " ").trim())
      .filter((t) => /Export/i.test(t));
    const tooltips: string[] = [];
    for (let el: HTMLElement | null = progress; el && el !== bar; el = el.parentElement) {
      if (el.title) tooltips.push(el.title);
    }
    return { status: statuses.join(" | "), tooltips };
  });
}

const WIDTHS = [900, 1000, 1100, 1200, 1300, 1600] as const;

for (const width of WIDTHS) {
  test(`starting an export moves no top-bar item at ${width} px (AC-SX3EDV.1)`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 800 });
    await streamWithData(page, "?mock=slow-export");
    await expect(page.locator(".top-bar")).not.toHaveClass(/export-in-menu/);
    await page.waitForTimeout(300);
    const before = await topBarItems(page);
    expect(before.length).toBeGreaterThan(4);
    const exportButton = page.locator(".top-bar button.export");
    const buttonBefore = await boxOf(exportButton, "Export button");

    await exportButton.click();
    const progress = page.getByLabel("Export progress");
    await expect(progress).toBeVisible();
    await page.waitForTimeout(300);
    expectItemsUnmoved(before, await topBarItems(page));

    // The progress fills the disabled Export button's own box.
    await expect(exportButton).toBeDisabled();
    const button = await boxOf(exportButton, "Export button");
    expectSameBox(buttonBefore, button, "Export button");
    const bar = await boxOf(progress, "export progress");
    expect(bar.x, "progress inside the button (left)").toBeGreaterThanOrEqual(button.x - 0.5);
    expect(bar.y, "progress inside the button (top)").toBeGreaterThanOrEqual(button.y - 0.5);
    expect(bar.x + bar.width, "progress inside the button (right)").toBeLessThanOrEqual(
      button.x + button.width + 0.5,
    );
    expect(bar.y + bar.height, "progress inside the button (bottom)").toBeLessThanOrEqual(
      button.y + button.height + 0.5,
    );
    expect(bar.width, "progress spans the button").toBeGreaterThanOrEqual(button.width * 0.6);

    // The count, in the status text and in the tooltip.
    const { status, tooltips } = await exportTexts(page);
    const m = /(\d[\d,]*)\s*\/\s*(\d[\d,]*)/.exec(status);
    expect(m, `status text gives the count: "${status}"`).not.toBeNull();
    const done = m![1]!;
    expect(Number(done.replace(/,/g, ""))).toBeGreaterThan(0);
    expect(
      tooltips.some((t) => t.includes(done)),
      `a tooltip names ${done}: ${JSON.stringify(tooltips)}`,
    ).toBe(true);
  });
}

test("with Export in the menu, a running export is a thin line along the bar's bottom edge (AC-SX3EDV.2)", async ({
  page,
}) => {
  await page.setViewportSize({ width: 600, height: 800 });
  await streamWithData(page, "?mock=slow-export");
  const topBar = page.locator(".top-bar");
  await expect(topBar).toHaveClass(/export-in-menu/);
  await page.waitForTimeout(300);
  const before = await topBarItems(page);

  await page.getByRole("button", { name: /^More:/ }).click();
  await page.locator("#top-bar-menu").getByRole("menuitem", { name: "Export" }).click();
  const progress = page.getByLabel("Export progress");
  await expect(progress).toBeVisible();
  await expect(page.locator("#top-bar-menu")).toHaveCount(0);
  await page.waitForTimeout(300);

  expectItemsUnmoved(before, await topBarItems(page));
  const bar = await boxOf(topBar, "top bar");
  const line = await boxOf(progress, "export progress");
  expect(line.height, "a thin line").toBeLessThanOrEqual(4);
  expect(line.width, "along the bar").toBeGreaterThanOrEqual(bar.width * 0.5);
  expect(
    Math.abs(line.y + line.height - (bar.y + bar.height)),
    "on the bar's bottom edge",
  ).toBeLessThanOrEqual(2);
});

/** A saved recording's notice, then an export's notice: two at once. */
async function showTwoNotices(page: Page): Promise<void> {
  await blur(page);
  await page.keyboard.press(" ");
  await expect(page.getByRole("timer")).toBeVisible();
  await page.waitForTimeout(500);
  await page.getByRole("button", { name: "Stop", exact: true }).click();
  await expect(page.locator(".notice").filter({ hasText: /Recording saved/ })).toBeVisible();
  await page.getByRole("button", { name: "Export", exact: true }).click();
  await expect(page.locator(".notice").filter({ hasText: /Export/i })).toBeVisible();
  await expect(page.locator(".notice")).toHaveCount(2);
}

const THEMES: readonly Theme[] = ["light", "dark"];

for (const theme of THEMES) {
  for (const height of [900, 1400] as const) {
    test(`below 1200 px, notices sit between the chart card and the Stats panel: 900×${height}, ${theme} (AC-SX3EDV.3)`, async ({
      page,
    }) => {
      await page.setViewportSize({ width: 900, height });
      await useTheme(page, theme);
      await streamWithData(page);
      const readout = page.getByRole("region", { name: "Live readout" });
      const chart = page.getByRole("region", { name: "Chart" });
      const stats = page.getByRole("complementary", { name: "Statistics" });
      await page.waitForTimeout(300);
      const chartBefore = await boxOf(chart, "chart card");

      await showTwoNotices(page);
      await page.waitForTimeout(300);
      expectSameBox(chartBefore, await boxOf(chart, "chart card"), "chart card", 1);

      const covered = {
        readout: await boxOf(readout, "readout"),
        chart: await boxOf(chart, "chart card"),
        stats: await boxOf(stats, "Stats panel"),
      };
      const notices = page.locator(".notice");
      for (let i = 0; i < (await notices.count()); i++) {
        const n = await boxOf(notices.nth(i), `notice ${i}`);
        for (const [name, box] of Object.entries(covered)) {
          expect(intersects(n, box), `notice ${i} covers no part of the ${name}`).toBe(false);
        }
        expect(n.y, `notice ${i} below the chart card`).toBeGreaterThanOrEqual(
          covered.chart.y + covered.chart.height - 0.5,
        );
        expect(n.y + n.height, `notice ${i} above the Stats panel`).toBeLessThanOrEqual(
          covered.stats.y + 0.5,
        );
      }
      const position = await page
        .locator(".notice-stack")
        .evaluate((el) => getComputedStyle(el).position);
      expect(position, "the stack is part of the page, not floating").not.toBe("fixed");
    });
  }
}

test("at 1600 px, notices float over the side panel and move nothing (AC-SX3EDV.4)", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1600, height: 900 });
  await streamWithData(page);
  const chart = page.getByRole("region", { name: "Chart" });
  await page.waitForTimeout(300);
  const chartBefore = await boxOf(chart, "chart card");

  await showTwoNotices(page);
  const stack = page.locator(".notice-stack");
  expect(await stack.evaluate((el) => getComputedStyle(el).position)).toBe("fixed");
  const s = await boxOf(stack, "notice stack");
  const side = await boxOf(page.locator(".side-panel"), "side panel");
  const overlap = Math.min(s.x + s.width, side.x + side.width) - Math.max(s.x, side.x);
  expect(overlap, "the stack sits over the side panel").toBeGreaterThanOrEqual(s.width * 0.8);

  // AC-AKNVND.3 still holds.
  const overview = await boxOf(page.locator(".chart-wrap .overview"), "overview strip");
  expect(overview.y + overview.height, "overview bottom on screen").toBeLessThanOrEqual(900);
  expectSameBox(chartBefore, await boxOf(chart, "chart card"), "chart card", 1);
});

/** The strip's painted pixel columns (CSS px) and how many of the columns from the first
 * to the last painted one are painted, leaving out every column a marked gap covers. */
async function stripCoverage(
  page: Page,
): Promise<{ first: number; last: number; eligible: number; painted: number }> {
  const breaks = await overviewBreaks(page);
  return page.evaluate((gaps) => {
    const canvas = document.querySelector<HTMLCanvasElement>(".chart-wrap .overview-canvas");
    const ctx = canvas?.getContext("2d");
    if (!canvas || !ctx || canvas.clientWidth === 0) {
      return { first: -1, last: -1, eligible: 0, painted: 0 };
    }
    const ratio = canvas.width / canvas.clientWidth;
    const cols = canvas.clientWidth;
    const { data, width, height } = ctx.getImageData(0, 0, canvas.width, canvas.height);
    const painted = new Array<boolean>(cols).fill(false);
    for (let c = 0; c < cols; c++) {
      const x0 = Math.floor(c * ratio);
      const x1 = Math.max(x0 + 1, Math.floor((c + 1) * ratio));
      search: for (let x = x0; x < Math.min(x1, width); x++) {
        for (let y = 0; y < height; y++) {
          // The envelope is painted at 0.75 alpha; gap bands at 0.18.
          if (data[(y * width + x) * 4 + 3]! > 100) {
            painted[c] = true;
            break search;
          }
        }
      }
    }
    const blocked = (c: number): boolean =>
      gaps.some((g) => c >= Math.floor(g.leftPx) - 1 && c <= Math.ceil(g.leftPx + g.widthPx) + 1);
    const first = painted.indexOf(true);
    const last = painted.lastIndexOf(true);
    let eligible = 0;
    let n = 0;
    for (let c = Math.max(0, first); c <= last; c++) {
      if (blocked(c)) continue;
      eligible++;
      if (painted[c]) n++;
    }
    return { first, last, eligible, painted: n };
  }, breaks);
}

async function expectContinuousStrip(page: Page): Promise<void> {
  await expect
    .poll(
      async () => {
        const c = await stripCoverage(page);
        return c.last - c.first;
      },
      { timeout: 10_000, message: "the strip spans many columns" },
    )
    .toBeGreaterThan(200);
  // Let the strip repaint (at most 4 per second) with the newest samples.
  await page.waitForTimeout(600);
  const c = await stripCoverage(page);
  expect(
    c.painted / c.eligible,
    `painted ${c.painted} of ${c.eligible} columns from ${c.first} to ${c.last}`,
  ).toBeGreaterThanOrEqual(0.98);
}

async function clearChartHistory(page: Page): Promise<void> {
  const panel = page.getByRole("complementary", { name: "Statistics" });
  await panel.getByRole("button", { name: "Clear chart history…", exact: true }).click();
  await panel.getByRole("button", { name: "Clear", exact: true }).click();
  await expect(panel.getByRole("button", { name: "Cancel", exact: true })).toHaveCount(0);
}

test.describe("the overview strip draws sparse series as a line (AC-SX3EDV.6)", () => {
  test.use({ viewport: { width: 1600, height: 900 }, deviceScaleFactor: 1 });

  test("at 50 SPS", async ({ page }) => {
    await streamWithData(page);
    await expect(page.getByLabel("Sample rate")).toHaveValue("50");
    await page.waitForTimeout(4_000);
    await expectContinuousStrip(page);
  });

  test("at 10 SPS", async ({ page }) => {
    await streamWithData(page);
    await page.getByLabel("Sample rate").selectOption("10");
    await expect(page.getByLabel("Sample rate")).toHaveValue("10");
    // A session that holds 10 SPS samples only.
    await clearChartHistory(page);
    await page.waitForTimeout(5_000);
    await expectContinuousStrip(page);
  });

  test("after a rate change from 50 to 10 SPS", async ({ page }) => {
    await streamWithData(page);
    await page.waitForTimeout(2_000);
    await page.getByLabel("Sample rate").selectOption("10");
    await expect(page.getByLabel("Sample rate")).toHaveValue("10");
    await page.waitForTimeout(5_000);
    await expectContinuousStrip(page);
  });
});

test("the disconnect banner sits in the line above the plot (AC-SX3EDV.7)", async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 900 });
  await streamWithData(page);
  await page.getByRole("button", { name: "Disconnect", exact: true }).click();
  const chart = page.getByRole("region", { name: "Chart" });
  const banner = chart.getByText("showing the last session");
  await expect(banner).toBeVisible();
  await page.waitForTimeout(300);

  const b = await boxOf(banner, "banner");
  const plotHost = await boxOf(page.locator(".chart-wrap .plot"), "plot");
  const over = await boxOf(page.locator(".plot .u-over"), "plot area");
  const legend = await boxOf(page.locator(".chart-wrap .legend"), "legend");
  // The plot holds the traces, the axes and their tick labels (uPlot paints them there).
  expect(intersects(b, over), "the banner covers no trace").toBe(false);
  expect(intersects(b, plotHost), "the banner covers no axis or tick label").toBe(false);
  expect(b.y + b.height, "above the plot").toBeLessThanOrEqual(plotHost.y + 0.5);
  expect(b.y, "below the legend, in the line above the plot").toBeGreaterThanOrEqual(
    legend.y + legend.height - 0.5,
  );

  // Over the plot, the hover readout takes that line.
  await page.mouse.move(over.x + over.width * 0.6, over.y + over.height / 2);
  const value = page.locator(".chart-wrap .hover .hover-value").first();
  await expect(value).toBeVisible();
  await expect(banner).toBeHidden();
  const v = await boxOf(value, "hover value");
  expect(v.y + v.height, "the hover readout sits above the plot").toBeLessThanOrEqual(
    plotHost.y + 0.5,
  );
  expect(v.y, "in the banner's line").toBeGreaterThanOrEqual(legend.y + legend.height - 0.5);

  // Off the plot, the banner comes back.
  await page.mouse.move(1, 1);
  await expect(banner).toBeVisible();
});
