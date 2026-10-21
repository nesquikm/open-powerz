/**
 * Layout at every supported window size (AC-AH2AEK.1) and the one-row top bar
 * at 900 px (milestone visual review): no horizontal scroll, no readout value
 * wider than its cell or overlapping a neighbour, and a top bar that keeps all
 * its items on one row in every state, in both themes.
 *
 * Fonts differ by OS (DejaVu Sans on CI Linux, Segoe UI on Windows, SF on a
 * Mac), so nothing here depends on one font's widths; `E2E_WIDE_FONT=1`
 * (helpers.ts) runs the suite with a wide font to check that on any machine.
 */

import { expect, test, type Locator, type Page } from "@playwright/test";
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

interface BarItem {
  label: string;
  top: number;
  centre: number;
  left: number;
  right: number;
}

/** Every visible item of the top bar: its box, its vertical centre and its label. */
function topBarItems(page: Page): Promise<BarItem[]> {
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
        return { label, top: r.top, centre: r.top + r.height / 2, left: r.left, right: r.right };
      });
  });
}

/** Items of different heights (a short progress bar, taller buttons) share a row when
 * their vertical centres line up; a wrapped item sits a whole control height lower. */
function expectOneRow(items: BarItem[]): void {
  const centres = items.map((i) => i.centre);
  const spread = Math.max(...centres) - Math.min(...centres);
  expect(
    spread,
    `items share one row: ${items.map((i) => `${i.label}@${i.centre.toFixed(0)}`).join(", ")}`,
  ).toBeLessThanOrEqual(4);
}

/** Nothing sticks out of the bar, and no two items overlap side by side. */
async function expectNoClipping(page: Page, items: BarItem[]): Promise<void> {
  const bar = await page.locator(".top-bar").evaluate((el) => ({
    scrollWidth: el.scrollWidth,
    clientWidth: el.clientWidth,
    right: el.getBoundingClientRect().right,
  }));
  expect(bar.scrollWidth, "nothing sticks out of the bar").toBeLessThanOrEqual(bar.clientWidth);
  const sorted = [...items].sort((a, b) => a.left - b.left);
  for (let i = 1; i < sorted.length; i++) {
    const [a, b] = [sorted[i - 1]!, sorted[i]!];
    expect(b.left, `"${a.label}" and "${b.label}" do not overlap`).toBeGreaterThanOrEqual(
      a.right - 0.5,
    );
  }
  expect(sorted.at(-1)!.right, "the last item is inside the bar").toBeLessThanOrEqual(bar.right);
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
      expectOneRow(items);
      await expectNoClipping(page, items);
      expect(await barHeight(page), "the bar does not grow").toBeLessThanOrEqual(baseline + 1);
    });
  }
}

/** The picker's selected label, its tooltip, and whether the label fits visibly. */
function pickerLabel(picker: Locator) {
  return picker.evaluate((el: HTMLSelectElement) => {
    const style = getComputedStyle(el);
    const text = el.selectedOptions[0]?.text.trim() ?? "";
    const ctx = document.createElement("canvas").getContext("2d");
    let fits = false;
    if (ctx) {
      ctx.font = `${style.fontWeight} ${style.fontSize} ${style.fontFamily}`;
      const padding = parseFloat(style.paddingLeft) + parseFloat(style.paddingRight);
      // At least as wide as its label's text plus its padding and arrow.
      fits = el.getBoundingClientRect().width >= ctx.measureText(text).width + padding + 12;
    }
    return { text, title: el.title, fits };
  });
}

// pz-ff's review: no scenario name cut short without a tooltip. Where room is short
// (900 px, a wide font) the picker may shrink with an ellipsis, but its tooltip carries
// the full label and its neighbours never cover it; from 1280 px the label fits whole.
for (const width of [900, 1280, 1600] as const) {
  test(`the device picker shows its full label or a tooltip at ${width} px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 800 });
    await open(page);
    const picker = page.getByLabel("Device", { exact: true });
    for (const option of await picker.locator("option").all()) {
      const value = (await option.getAttribute("value")) ?? "";
      const label = (await option.innerText()).trim();
      await picker.selectOption(value);
      await expect(picker).toHaveAttribute("title", label);
      const shown = await pickerLabel(picker);
      expect(shown.text, "the selected option is the full label").toBe(label);
      await expectNoClipping(page, await topBarItems(page));
      if (width >= 1280) expect(shown.fits, `"${label}" fits in the picker`).toBe(true);
    }
  });
}

test("with very large control text the bar keeps one row and every control stays reachable", async ({
  page,
}) => {
  await page.setViewportSize({ width: 900, height: 600 });
  await open(page);
  await page.addStyleTag({
    content: `.top-bar button, .top-bar select { font-size: 1.15rem !important; }`,
  });
  await streamDemo(page);
  await startRecording(page);
  await page.waitForTimeout(300);
  const items = await topBarItems(page);
  expectOneRow(items);
  await expectNoClipping(page, items);
  // Whatever moved into the "⋯" menu is there, and the keyboard reaches it.
  const more = page.getByRole("button", { name: /^More:/ });
  await more.focus();
  await page.keyboard.press("Enter");
  const menu = page.locator("#top-bar-menu");
  await expect(menu).toBeVisible();
  for (const name of ["Sample rate", "Export"] as const) {
    const control =
      name === "Sample rate"
        ? page.getByRole("combobox", { name, exact: true })
        : page.getByRole("button", { name, exact: true });
    await expect(control, `${name} is in the bar or in the menu`).toBeVisible();
  }
  await expect(menu.getByRole("radiogroup", { name: "Theme" })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(menu).toBeHidden();
});
