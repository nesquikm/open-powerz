/**
 * FR TGE83D: the PD timeline linked to the power chart, on the mocked backend
 * (AC .1, .2, .3, .4, .6).
 *
 * The page contract these specs pin (on top of `pd.ts`):
 *
 * - The chart draws PD markers in a canvas layer of its own above the plot,
 *   `canvas[data-layer="pd-markers"]` inside the "Chart" section.
 * - A legend, a `list` named "PD markers" in the "Chart" section, has one `listitem` per
 *   marker kind ("Negotiation", "Reset", "Error", "Attach / detach", "Other"), each with
 *   `data-kind` (negotiation, reset, error, attach_detach, other) and a `.swatch` element
 *   whose background colour is the kind's marker colour.
 * - The test hook gains `chartPdMarkers()`: the markers and count badges of the last
 *   draw, `{ leftPx, topPx, count, kind, color, ms, indices }`, with `leftPx` / `topPx`
 *   the marker's centre in CSS px from the marker layer's top-left corner, `ms` its
 *   session time (the first record's for a badge) and `indices` its record indices;
 *   and `chartPdStatusLengths()`: the points drawn from the PD status blocks per series,
 *   `{ vbus, ibus, cc1, cc2 }`.
 * - Packet table rows carry `data-session-ms`, the record's session time.
 * - Clicking a marker shows the PD analyzer tab with the marker's row selected and
 *   scrolled into the table's view. Selecting a table row places the chart's playhead
 *   at the row's session time and moves the chart's view to it (keeping the zoom when
 *   the time is visible already); the Meter tab shows it.
 */

import { expect, test, type Locator, type Page } from "@playwright/test";
import { HOOK } from "../hook";
import { blur, chartPlayhead, expectHook, lastDraw } from "./helpers";
import { capture, dataRows, grid, openPd, rowNamed, scrollToTop, stopCapture } from "./pd";

test.use({ viewport: { width: 1600, height: 900 } });

interface PdMarkerDraw {
  leftPx: number;
  topPx: number;
  count: number;
  kind: string;
  color: string;
  ms: number;
  indices: number[];
}

const KINDS = ["negotiation", "reset", "error", "attach_detach", "other"] as const;
const LABELS = ["Negotiation", "Reset", "Error", "Attach / detach", "Other"] as const;

/** Calls an accessor of the test hook that this FR adds. */
async function hook<T>(page: Page, method: string): Promise<T> {
  await expectHook(page);
  return page.evaluate(
    ([name, m]) => {
      const h = (window as unknown as Record<string, Record<string, unknown>>)[name]!;
      const fn = h[m];
      if (typeof fn !== "function") throw new Error(`the test hook has no ${m}()`);
      return (fn as () => unknown).call(h);
    },
    [HOOK, method] as const,
  ) as Promise<T>;
}

function chartPdMarkers(page: Page): Promise<PdMarkerDraw[]> {
  return hook(page, "chartPdMarkers");
}

function chartPdStatusLengths(
  page: Page,
): Promise<{ vbus: number; ibus: number; cc1: number; cc2: number }> {
  return hook(page, "chartPdStatusLengths");
}

function chartSection(page: Page): Locator {
  return page.getByRole("region", { name: "Chart" });
}

function markerLayer(page: Page): Locator {
  return chartSection(page).locator('canvas[data-layer="pd-markers"]');
}

function meterTab(page: Page): Locator {
  return page.getByRole("tab", { name: "Meter" });
}

function pdTab(page: Page): Locator {
  return page.getByRole("tab", { name: "PD analyzer" });
}

function liveButton(page: Page): Locator {
  return page.locator(".chart-wrap button.live");
}

function span(d: { t0Ms: number; t1Ms: number }): number {
  return d.t1Ms - d.t0Ms;
}

/** Connect the demo device, capture ~3 s of PD traffic, stop, and show the Meter tab. */
async function capturedThenMeter(page: Page): Promise<void> {
  await openPd(page);
  await capture(page);
  await page.waitForTimeout(2_500);
  await stopCapture(page);
  await meterTab(page).click();
  await expect(chartSection(page)).toBeVisible();
  await expect.poll(async () => (await chartPdMarkers(page)).length).toBeGreaterThan(0);
}

/** Pause the chart (Live button) and wait for its frozen history draw. */
async function pause(page: Page): Promise<void> {
  await liveButton(page).click();
  await expect(liveButton(page)).toHaveText("Paused · Go live");
  await expect.poll(async () => (await lastDraw(page)).mode).toBe("history");
}

/** Normalises a CSS colour to the browser's computed `rgb(...)` form. */
async function rgb(page: Page, color: string): Promise<string> {
  return page.evaluate((c) => {
    const el = document.createElement("span");
    el.style.color = c;
    document.body.appendChild(el);
    const out = getComputedStyle(el).color;
    el.remove();
    return out;
  }, color);
}

/** Whether `row` lies inside the table's scrolled viewport. */
async function inTableView(page: Page, row: Locator): Promise<boolean> {
  const g = await grid(page).boundingBox();
  const r = await row.boundingBox();
  if (!g || !r) return false;
  return r.y >= g.y - 1 && r.y + r.height <= g.y + g.height + 1;
}

/** Session time of a table row. */
async function sessionMsOf(row: Locator): Promise<number> {
  const v = Number(await row.getAttribute("data-session-ms"));
  if (!Number.isFinite(v)) throw new Error("the row has no data-session-ms");
  return v;
}

// ---- AC-TGE83D.1: markers on the chart, coloured by kind, with a legend ----

test("PD records appear as markers on the chart at their session time (AC-TGE83D.1)", async ({
  page,
}) => {
  await capturedThenMeter(page);
  await expect(markerLayer(page)).toBeVisible();
  await pause(page);
  const draw = await lastDraw(page);
  const markers = await chartPdMarkers(page);
  expect(markers.length).toBeGreaterThan(0);
  const layer = (await markerLayer(page).boundingBox())!;
  for (const m of markers) {
    expect(m.ms).toBeGreaterThanOrEqual(draw.t0Ms);
    expect(m.ms).toBeLessThanOrEqual(draw.t1Ms);
    expect(m.leftPx).toBeGreaterThanOrEqual(0);
    expect(m.leftPx).toBeLessThanOrEqual(layer.width);
    expect(m.count).toBe(m.indices.length);
  }
  // Earlier records sit further left.
  const sorted = [...markers].sort((a, b) => a.ms - b.ms);
  for (let i = 1; i < sorted.length; i++) {
    expect(sorted[i]!.leftPx).toBeGreaterThanOrEqual(sorted[i - 1]!.leftPx);
  }
  // The demo scenario has an attach and a Hard Reset in every cycle, far apart.
  const kinds = new Set(markers.filter((m) => m.count === 1).map((m) => m.kind));
  expect(kinds.has("attach_detach")).toBe(true);
  expect(kinds.has("reset")).toBe(true);
  // The negotiation's messages are a few ms apart: at this zoom they share a badge.
});

test("a legend names the marker colours, and markers use them (AC-TGE83D.1)", async ({ page }) => {
  await capturedThenMeter(page);
  const legend = chartSection(page).getByRole("list", { name: "PD markers" });
  await expect(legend).toBeVisible();
  const items = legend.getByRole("listitem");
  await expect(items).toHaveCount(KINDS.length);
  expect((await items.allInnerTexts()).map((t) => t.trim())).toEqual([...LABELS]);
  const swatch: Record<string, string> = {};
  for (const kind of KINDS) {
    const item = legend.locator(`[data-kind="${kind}"]`);
    await expect(item).toHaveCount(1);
    swatch[kind] = await item
      .locator(".swatch")
      .evaluate((el) => getComputedStyle(el).backgroundColor);
  }
  expect(new Set(Object.values(swatch)).size).toBe(KINDS.length);
  const markers = (await chartPdMarkers(page)).filter((m) => m.count === 1);
  expect(markers.length).toBeGreaterThan(0);
  for (const m of markers) {
    expect(await rgb(page, m.color), `${m.kind} marker colour`).toBe(swatch[m.kind]);
  }
});

// ---- AC-TGE83D.2 / .6: marker → row ----

test("clicking a marker shows its row selected and scrolled into the table (AC-TGE83D.2, AC-TGE83D.6)", async ({
  page,
}) => {
  await capturedThenMeter(page);
  await pause(page);
  // The first attach: the oldest record, far above the followed table bottom.
  const markers = await chartPdMarkers(page);
  const target = markers
    .filter((m) => m.count === 1 && m.kind === "attach_detach")
    .sort((a, b) => a.ms - b.ms)[0];
  expect(target, "a lone attach marker").toBeDefined();
  const layer = (await markerLayer(page).boundingBox())!;
  await page.mouse.click(layer.x + target!.leftPx, layer.y + target!.topPx);

  await expect(pdTab(page)).toHaveAttribute("aria-selected", "true");
  const row = grid(page).locator(`[role="row"][data-index="${target!.indices[0]}"]`);
  await expect(row).toHaveAttribute("aria-selected", "true");
  await expect.poll(() => inTableView(page, row)).toBe(true);
  await expect(dataRows(page).and(page.locator('[aria-selected="true"]'))).toHaveCount(1);
});

// ---- AC-TGE83D.3 / .6: row → chart ----

test("selecting a row moves the playhead there and keeps the zoom when it is visible (AC-TGE83D.3, AC-TGE83D.6)", async ({
  page,
}) => {
  await capturedThenMeter(page);
  await page.getByRole("button", { name: "Whole session" }).click();
  await pause(page);
  const before = await lastDraw(page);

  await pdTab(page).click();
  await scrollToTop(page);
  const caps = rowNamed(page, "Source_Capabilities");
  const ms = await sessionMsOf(caps);
  expect(ms).toBeGreaterThanOrEqual(before.t0Ms);
  expect(ms).toBeLessThanOrEqual(before.t1Ms);
  await caps.click();
  await expect(caps).toHaveAttribute("aria-selected", "true");
  await expect.poll(() => chartPlayhead(page)).toBe(ms);

  await meterTab(page).click();
  const after = await lastDraw(page);
  expect(after.t0Ms).toBe(before.t0Ms);
  expect(after.t1Ms).toBe(before.t1Ms);
  expect(await chartPlayhead(page)).toBe(ms);
});

test("selecting a row outside the view brings it into view at the same zoom (AC-TGE83D.3, AC-TGE83D.6)", async ({
  page,
}) => {
  await capturedThenMeter(page);
  await pause(page);
  // Zoom in around the view centre until the capture start is out of view.
  await blur(page);
  for (let i = 0; i < 10; i++) await page.keyboard.press("+");
  await expect.poll(async () => span(await lastDraw(page))).toBeLessThan(2_000);
  const zoomed = await lastDraw(page);

  await pdTab(page).click();
  await scrollToTop(page);
  const first = dataRows(page).first();
  const ms = await sessionMsOf(first);
  expect(ms < zoomed.t0Ms || ms > zoomed.t1Ms, "the first record is out of view").toBe(true);
  await first.click();
  await expect(first).toHaveAttribute("aria-selected", "true");
  await expect.poll(() => chartPlayhead(page)).toBe(ms);

  await meterTab(page).click();
  await expect
    .poll(async () => {
      const d = await lastDraw(page);
      return d.t0Ms <= ms && ms <= d.t1Ms;
    })
    .toBe(true);
  const after = await lastDraw(page);
  expect(span(after)).toBeCloseTo(span(zoomed), -1);
  expect(after.mode).toBe("history");
});

// ---- AC-TGE83D.4: PD status series ----

test("VBUS, IBUS, CC1 and CC2 from the PD status blocks are plotted; CC1 and CC2 toggle (AC-TGE83D.4)", async ({
  page,
}) => {
  await capturedThenMeter(page);
  await page.getByRole("button", { name: "Whole session" }).click();
  await expect
    .poll(async () => {
      const n = await chartPdStatusLengths(page);
      return Math.min(n.vbus, n.ibus, n.cc1, n.cc2);
    })
    .toBeGreaterThan(0);

  const series = chartSection(page).getByRole("group", { name: "Series" });
  const cc1 = series.getByRole("button", { name: /^CC1/ });
  const cc2 = series.getByRole("button", { name: /^CC2/ });
  await cc1.click();
  await expect(cc1).toHaveAttribute("aria-pressed", "false");
  await expect.poll(async () => (await chartPdStatusLengths(page)).cc1).toBe(0);
  expect((await chartPdStatusLengths(page)).cc2).toBeGreaterThan(0);
  await cc2.click();
  await expect.poll(async () => (await chartPdStatusLengths(page)).cc2).toBe(0);
  await cc1.click();
  await expect.poll(async () => (await chartPdStatusLengths(page)).cc1).toBeGreaterThan(0);
});

// ---- marker clicks and the keyboard ----

/** The lone attach marker the capture started with, in the paused chart. */
async function firstAttach(page: Page): Promise<PdMarkerDraw> {
  const target = (await chartPdMarkers(page))
    .filter((m) => m.count === 1 && m.kind === "attach_detach")
    .sort((a, b) => a.ms - b.ms)[0];
  expect(target, "a lone attach marker").toBeDefined();
  return target!;
}

/** Counts the focus events the chart's plot receives from now on. */
async function countPlotFocus(page: Page): Promise<() => Promise<number>> {
  await page.evaluate(() => {
    const plot = document.querySelector<HTMLElement>('[aria-label="Chart"] .plot');
    if (plot === null) throw new Error("no chart plot");
    const w = window as unknown as { __plotFocus: number };
    w.__plotFocus = 0;
    plot.addEventListener("focus", () => w.__plotFocus++);
  });
  return () => page.evaluate(() => (window as unknown as { __plotFocus: number }).__plotFocus);
}

test("a marker click focuses the plot; on the tabs the arrows switch tabs, not the playhead", async ({
  page,
}) => {
  await capturedThenMeter(page);
  await pause(page);
  const target = await firstAttach(page);
  const layer = (await markerLayer(page).boundingBox())!;
  const plotFocus = await countPlotFocus(page);
  await page.mouse.click(layer.x + target.leftPx, layer.y + target.topPx);
  await expect(pdTab(page)).toHaveAttribute("aria-selected", "true");
  // Like a scrub, the click puts the focus on the plot (it was on the Live button).
  expect(await plotFocus()).toBeGreaterThan(0);

  await meterTab(page).click();
  await expect(meterTab(page)).toHaveAttribute("aria-selected", "true");
  // The focused tab list owns ← / →: the next tab, and the playhead stays put.
  const before = await chartPlayhead(page);
  await page.keyboard.press("ArrowRight");
  await expect(pdTab(page)).toHaveAttribute("aria-selected", "true");
  expect(await chartPlayhead(page)).toBe(before);
});

for (const modifier of ["Control", "Meta"] as const) {
  test(`a ${modifier}-click on a marker does not open its record (the context-menu gesture)`, async ({
    page,
  }) => {
    await capturedThenMeter(page);
    await pause(page);
    const target = await firstAttach(page);
    const layer = (await markerLayer(page).boundingBox())!;
    await page.keyboard.down(modifier);
    await page.mouse.click(layer.x + target.leftPx, layer.y + target.topPx);
    await page.keyboard.up(modifier);
    // Give a tab switch the time it would take.
    await page.waitForTimeout(300);
    await expect(meterTab(page)).toHaveAttribute("aria-selected", "true");
    await expect(pdTab(page)).toHaveAttribute("aria-selected", "false");
  });
}
