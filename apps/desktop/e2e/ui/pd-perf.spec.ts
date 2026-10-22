/**
 * AC-TGE83C.6: with 100,000 records (`?pdsynthetic=100000` on the mocked backend) the
 * PD table scrolls and filters without a frame longer than 50 ms, using a virtual
 * list. Frames are timed in the page with requestAnimationFrame while the steps run;
 * the longest gap between two frames is the longest frame.
 *
 * Timed on Chromium only, like the other timings of this suite.
 */

import { expect, test, type Page } from "@playwright/test";
import { ciLatencyBudgetMs } from "./budget";
import { dataRows, grid, openPd, pdView, rowCount, startButton, stopCapture } from "./pd";

const RECORDS = 100_000;
/** 50 ms, times the CI factor when CI is set. */
const FRAME_BUDGET_MS = ciLatencyBudgetMs(50);

test.use({ viewport: { width: 1600, height: 900 } });

/** Starts timing frames in the page. */
async function startFrames(page: Page): Promise<void> {
  await page.evaluate(() => {
    const w = window as unknown as { __pdFrames?: { gaps: number[]; last: number; on: boolean } };
    const f = { gaps: [] as number[], last: performance.now(), on: true };
    w.__pdFrames = f;
    const tick = (t: number) => {
      if (!f.on) return;
      f.gaps.push(t - f.last);
      f.last = t;
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  });
}

/** Stops timing and returns every frame gap, ms. */
async function stopFrames(page: Page): Promise<number[]> {
  return page.evaluate(() => {
    const f = (window as unknown as { __pdFrames: { gaps: number[]; on: boolean } }).__pdFrames;
    f.on = false;
    return f.gaps;
  });
}

function report(step: string, gaps: number[]): string {
  const worst = [...gaps].sort((a, b) => b - a).slice(0, 5);
  return `${step}: ${gaps.length} frames, longest (ms) ${worst.map((g) => g.toFixed(1)).join(", ")}; budget ${FRAME_BUDGET_MS} ms`;
}

async function seeded(page: Page, browserName: string): Promise<void> {
  test.skip(browserName !== "chromium", "timed on Chromium only");
  test.setTimeout(180_000);
  await openPd(page, `?pdsynthetic=${RECORDS}`);
  await startButton(page).click();
  // GoodCRC is hidden by default, so the table shows part of the seed.
  await expect.poll(() => rowCount(page), { timeout: 60_000 }).toBeGreaterThan(RECORDS / 4);
  await stopCapture(page);
  await pdView(page).getByRole("checkbox", { name: "Show GoodCRC" }).check();
  await expect.poll(() => rowCount(page), { timeout: 30_000 }).toBeGreaterThanOrEqual(RECORDS);
}

test("100,000 records render as a virtual list (AC-TGE83C.6)", async ({ page, browserName }) => {
  await seeded(page, browserName);
  const rendered = await dataRows(page).count();
  expect(rendered).toBeGreaterThan(0);
  expect(rendered, "rendered rows").toBeLessThan(300);
  // Jumping to the middle renders the rows there.
  await grid(page).evaluate((g) => {
    g.scrollTop = g.scrollHeight / 2;
  });
  await expect
    .poll(async () => {
      const idx = await dataRows(page).evaluateAll((rows) =>
        rows.map((r) => Number(r.getAttribute("data-index"))),
      );
      return idx.some((i) => i > RECORDS / 4 && i < (RECORDS * 3) / 4);
    })
    .toBe(true);
  expect(await dataRows(page).count()).toBeLessThan(300);
});

test("scrolling 100,000 records keeps every frame within 50 ms (AC-TGE83C.6)", async ({
  page,
  browserName,
}) => {
  await seeded(page, browserName);
  const box = await grid(page).boundingBox();
  if (!box) throw new Error("no grid");
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await startFrames(page);
  for (let i = 0; i < 20; i++) await page.mouse.wheel(0, 3_000);
  for (const at of [0.5, 0.9, 0.1, 1]) {
    await grid(page).evaluate((g, f) => {
      g.scrollTop = (g.scrollHeight - g.clientHeight) * f;
    }, at);
    await page.evaluate(
      () => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))),
    );
  }
  for (let i = 0; i < 20; i++) await page.mouse.wheel(0, -3_000);
  const gaps = await stopFrames(page);
  expect(gaps.length).toBeGreaterThan(10);
  expect(Math.max(...gaps), report("scroll", gaps)).toBeLessThanOrEqual(FRAME_BUDGET_MS);
});

test("filtering 100,000 records keeps every frame within 50 ms (AC-TGE83C.6)", async ({
  page,
  browserName,
}) => {
  await seeded(page, browserName);
  const all = await rowCount(page);
  const toggle = pdView(page).getByRole("checkbox", { name: "Show GoodCRC" });
  const type = pdView(page).getByLabel("Message type", { exact: true });
  // Each step is followed by two frames, so the timed window always holds
  // the frames that render the filtered table, however fast filtering is.
  const twoFrames = () =>
    page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
  await startFrames(page);
  await toggle.uncheck();
  await expect.poll(() => rowCount(page)).toBeLessThan(all);
  await twoFrames();
  await type.selectOption({ label: "Request" });
  await expect.poll(async () => (await rowCount(page)) < all / 4).toBe(true);
  await twoFrames();
  await type.selectOption({ label: "All" });
  await twoFrames();
  await toggle.check();
  await expect.poll(() => rowCount(page)).toBe(all);
  await twoFrames();
  const gaps = await stopFrames(page);
  expect(gaps.length).toBeGreaterThan(5);
  expect(Math.max(...gaps), report("filter", gaps)).toBeLessThanOrEqual(FRAME_BUDGET_MS);
});
