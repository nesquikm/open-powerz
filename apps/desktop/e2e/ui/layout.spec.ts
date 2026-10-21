/**
 * Layout at every supported window size (AC-AH2AEK.1) and the one-row top bar
 * at 900 px (milestone visual review): no horizontal scroll, no readout value
 * wider than its cell or overlapping a neighbour, and a top bar that keeps all
 * its items on one row in every state, in both themes.
 */

import { expect, test, type Page } from "@playwright/test";
import {
  blur,
  chartSeriesLengths,
  connectDemo,
  open,
  readoutValue,
  streamDemo,
  useTheme,
  type Theme,
} from "./helpers";

const THEMES: readonly Theme[] = ["light", "dark"];
const SIZES = [
  { width: 900, height: 600 },
  { width: 1280, height: 800 },
  { width: 1600, height: 900 },
  { width: 1920, height: 1080 },
  { width: 2560, height: 1440 },
] as const;

/** Wait until the readout and chart show live values. */
async function waitForValues(page: Page): Promise<void> {
  await expect.poll(() => readoutValue(page, "VBUS")).not.toBeNaN();
  await expect.poll(async () => (await chartSeriesLengths(page)).vbus).toBeGreaterThan(0);
}

/** Readout values wider than their box, and readout cells that overlap each other. */
function readoutProblems(page: Page): Promise<string[]> {
  return page.evaluate(() => {
    const problems: string[] = [];
    const text = (el: Element) => (el as HTMLElement).innerText.replace(/\s+/g, " ").trim();
    for (const v of document.querySelectorAll<HTMLElement>(
      ".readout .value, .readout .small-value",
    )) {
      if (v.scrollWidth > v.clientWidth + 1) problems.push(`overflows its cell: ${text(v)}`);
    }
    const cells = [...document.querySelectorAll<HTMLElement>(".readout .cell, .readout .line")];
    const rects = cells.map((c) => c.getBoundingClientRect());
    for (let i = 0; i < rects.length; i++) {
      for (let j = i + 1; j < rects.length; j++) {
        const a = rects[i]!;
        const b = rects[j]!;
        const overlap =
          a.left < b.right - 1 &&
          b.left < a.right - 1 &&
          a.top < b.bottom - 1 &&
          b.top < a.bottom - 1;
        if (overlap) problems.push(`overlap: "${text(cells[i]!)}" and "${text(cells[j]!)}"`);
      }
    }
    return problems;
  });
}

for (const theme of THEMES) {
  for (const size of SIZES) {
    for (const state of ["disconnected", "streaming"] as const) {
      test(`no horizontal scroll or readout overlap: ${state}, ${theme}, ${size.width}×${size.height} (AC-AH2AEK.1)`, async ({
        page,
      }) => {
        await page.setViewportSize(size);
        await useTheme(page, theme);
        await open(page);
        if (state === "streaming") {
          await streamDemo(page);
          await waitForValues(page);
        }
        const scroll = await page.evaluate(() => ({
          scrollWidth: document.documentElement.scrollWidth,
          clientWidth: document.documentElement.clientWidth,
        }));
        expect(scroll.scrollWidth, "no horizontal scroll").toBeLessThanOrEqual(scroll.clientWidth);
        expect(await readoutProblems(page)).toEqual([]);
      });
    }
  }
}

/** Every visible item of the top bar: its vertical centre and its label. */
function topBarItems(page: Page): Promise<{ label: string; top: number; centre: number }[]> {
  return page.evaluate(() => {
    const bar = document.querySelector(".top-bar");
    if (!bar) return [];
    const selector = ".brand, select, button, .badge, .rate-note, .rec-indicator, .export-progress";
    return [...bar.querySelectorAll<HTMLElement>(selector)]
      .filter((el) => !el.closest(".menu"))
      .filter((el) => el.getClientRects().length > 0 && el.offsetWidth > 0)
      .map((el) => {
        const r = el.getBoundingClientRect();
        const label = (el.getAttribute("aria-label") ?? el.innerText).replace(/\s+/g, " ").trim();
        return { label, top: r.top, centre: r.top + r.height / 2 };
      });
  });
}

async function barHeight(page: Page): Promise<number> {
  return (await page.locator(".top-bar").boundingBox())?.height ?? Infinity;
}

/** Start a recording from the page (Space on a focused button would press it). */
async function startRecording(page: Page): Promise<void> {
  await blur(page);
  await page.keyboard.press(" ");
  await expect(page.getByRole("timer")).toBeVisible();
}

const BAR_STATES = {
  disconnected: async () => {},
  streaming: async (page: Page) => {
    await streamDemo(page);
  },
  "recording with timer": async (page: Page) => {
    await streamDemo(page);
    await startRecording(page);
  },
  "recording and exporting with progress": async (page: Page) => {
    await streamDemo(page);
    await startRecording(page);
    await page.getByRole("button", { name: "Export", exact: true }).click();
    await expect(page.getByLabel("Export progress")).toBeVisible();
  },
  "rate-locked notice while recording": async (page: Page) => {
    await connectDemo(page);
    await expect(page.locator(".badge")).toHaveText("Rate locked");
    await startRecording(page);
  },
} as const;

const QUERY: Record<keyof typeof BAR_STATES, string> = {
  disconnected: "",
  streaming: "",
  "recording with timer": "",
  "recording and exporting with progress": "?mock=slow-export",
  "rate-locked notice while recording": "?mock=rate-locked",
};

for (const theme of THEMES) {
  for (const [state, enter] of Object.entries(BAR_STATES) as [
    keyof typeof BAR_STATES,
    (page: Page) => Promise<void>,
  ][]) {
    test(`the top bar keeps one row at 900×600: ${state}, ${theme}`, async ({ page }) => {
      await page.setViewportSize({ width: 900, height: 600 });
      await useTheme(page, theme);
      await open(page, QUERY[state]);
      const baseline = await barHeight(page);
      await enter(page);
      // Let the recording timer tick, so its text is at its usual width.
      await page.waitForTimeout(300);
      const items = await topBarItems(page);
      expect(items.length).toBeGreaterThan(4);
      const centres = items.map((i) => i.centre);
      const tops = items.map((i) => i.top);
      const spread = Math.max(...centres) - Math.min(...centres);
      expect(
        spread,
        `items share one row: ${items.map((i) => `${i.label}@${i.top.toFixed(0)}`).join(", ")}`,
      ).toBeLessThanOrEqual(3);
      // Items differ in height (a 13 px progress bar, 31 px buttons); a wrapped row
      // would sit a whole control height (~38 px) lower.
      expect(Math.max(...tops) - Math.min(...tops)).toBeLessThanOrEqual(12);
      expect(await barHeight(page), "the bar does not grow").toBeLessThanOrEqual(baseline + 1);
    });
  }
}

for (const width of [900, 1280, 1600] as const) {
  test(`the device picker shows its full label and a tooltip at ${width} px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 800 });
    await open(page);
    const picker = page.getByLabel("Device", { exact: true });
    for (const option of await picker.locator("option").all()) {
      const value = (await option.getAttribute("value")) ?? "";
      const label = (await option.innerText()).trim();
      await picker.selectOption(value);
      await expect(picker).toHaveAttribute("title", label);
      // The select is at least as wide as its label's text plus its padding and arrow.
      const fits = await picker.evaluate((el: HTMLSelectElement, text) => {
        const style = getComputedStyle(el);
        const ctx = document.createElement("canvas").getContext("2d");
        if (!ctx) return false;
        ctx.font = `${style.fontWeight} ${style.fontSize} ${style.fontFamily}`;
        const padding = parseFloat(style.paddingLeft) + parseFloat(style.paddingRight);
        return el.getBoundingClientRect().width >= ctx.measureText(text).width + padding + 12;
      }, label);
      expect(fits, `"${label}" fits in the picker`).toBe(true);
    }
  });
}
