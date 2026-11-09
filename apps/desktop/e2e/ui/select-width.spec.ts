/**
 * FR 350H0S: the drop-downs are wide enough for their labels now that the app
 * draws its own chevron inside the right padding (FR K1GZTR).
 *
 * - AC-350H0S.1 (picker fits from 1280 px, system font): layout.spec.ts,
 *   "the device picker shows its full label or a tooltip at {1280,1600} px".
 * - AC-350H0S.2 (the same check with a wider font): here.
 * - AC-350H0S.3 (chevron gap 10–12 px): select-arrow.spec.ts, unchanged.
 * - AC-350H0S.4 (every other drop-down fits its longest option): here.
 * - AC-350H0S.5 (900 px: ellipsis + tooltip, one-row bar): layout.spec.ts, the
 *   900 px device picker test and the one-row top bar tests.
 */

import { expect, test, type Locator, type Page } from "@playwright/test";
import { open, streamDemo } from "./helpers";

/** Verdana (else DejaVu Sans) is wider than the system defaults on every CI OS. */
const WIDE_FONT_CSS = `:root, html, body, button, select, input, progress {
  font-family: Verdana, "DejaVu Sans", sans-serif !important;
}`;

/** Forces the wide font before the page's first frame, as on a machine whose system font is wide. */
async function useWideFont(page: Page): Promise<void> {
  await page.addInitScript((css: string) => {
    const add = (): void => {
      const style = document.createElement("style");
      style.textContent = css;
      (document.head ?? document.documentElement).appendChild(style);
    };
    if (document.documentElement) {
      add();
      return;
    }
    const watch = new MutationObserver(() => {
      if (!document.documentElement) return;
      watch.disconnect();
      add();
    });
    watch.observe(document, { childList: true });
  }, WIDE_FONT_CSS);
}

/** The layout spec's check for the selected entry: width >= text + padding + 12 px. */
function selectedFits(picker: Locator) {
  return picker.evaluate((el: HTMLSelectElement) => {
    const style = getComputedStyle(el);
    const text = el.selectedOptions[0]?.text.trim() ?? "";
    const ctx = document.createElement("canvas").getContext("2d");
    if (!ctx) return { text, width: 0, needed: Infinity, family: style.fontFamily };
    ctx.font = `${style.fontWeight} ${style.fontSize} ${style.fontFamily}`;
    const padding = parseFloat(style.paddingLeft) + parseFloat(style.paddingRight);
    return {
      text,
      width: el.getBoundingClientRect().width,
      needed: ctx.measureText(text).width + padding + 12,
      family: style.fontFamily,
    };
  });
}

for (const width of [1280, 1600] as const) {
  test(`with a wide font every device picker entry fits at ${width} px (AC-350H0S.2)`, async ({
    page,
  }) => {
    await useWideFont(page);
    await page.setViewportSize({ width, height: 800 });
    await open(page);
    const picker = page.getByLabel("Device", { exact: true });
    const options = await picker.locator("option").all();
    expect(options.length, "the picker lists entries").toBeGreaterThan(0);
    for (const option of options) {
      const value = (await option.getAttribute("value")) ?? "";
      const label = (await option.innerText()).trim();
      await picker.selectOption(value);
      await expect(picker).toHaveAttribute("title", label);
      const shown = await selectedFits(picker);
      expect(shown.family, "the wide font is in use").toMatch(/Verdana|DejaVu/);
      expect(shown.text).toBe(label);
      expect(
        shown.width,
        `"${label}" fits in the picker (${shown.width.toFixed(1)} >= ${shown.needed.toFixed(1)})`,
      ).toBeGreaterThanOrEqual(shown.needed);
    }
  });
}

/** Every visible select that falls short of its longest option + padding + borders. */
function tooNarrowSelects(page: Page) {
  return page.evaluate(() => {
    const ctx = document.createElement("canvas").getContext("2d");
    const rows: { name: string; longest: string; width: number; needed: number }[] = [];
    let checked = 0;
    for (const el of document.querySelectorAll<HTMLSelectElement>("select")) {
      if (el.getClientRects().length === 0 || el.offsetWidth === 0) continue;
      checked++;
      const s = getComputedStyle(el);
      if (!ctx) {
        rows.push({ name: "no canvas", longest: "", width: 0, needed: Infinity });
        continue;
      }
      ctx.font = `${s.fontWeight} ${s.fontSize} ${s.fontFamily}`;
      let longest = "";
      let longestWidth = 0;
      for (const o of el.options) {
        const w = ctx.measureText(o.text.trim()).width;
        if (w > longestWidth) [longest, longestWidth] = [o.text.trim(), w];
      }
      const box =
        parseFloat(s.paddingLeft) +
        parseFloat(s.paddingRight) +
        parseFloat(s.borderLeftWidth) +
        parseFloat(s.borderRightWidth);
      const width = el.getBoundingClientRect().width;
      const needed = longestWidth + box;
      // The device picker is held to its own, stricter check (AC .1/.2).
      if (width + 0.5 < needed) {
        const name = el.getAttribute("aria-label") ?? el.id ?? el.name ?? "select";
        rows.push({ name, longest, width, needed });
      }
    }
    return { checked, rows };
  });
}

for (const state of ["disconnected", "streaming"] as const) {
  test(`every visible drop-down fits its longest option at 1280 px: ${state} (AC-350H0S.4)`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1280, height: 800 });
    await open(page);
    if (state === "streaming") await streamDemo(page);
    const result = await tooNarrowSelects(page);
    expect(result.checked, "some drop-downs are visible").toBeGreaterThan(0);
    expect(
      result.rows.map(
        (r) => `${r.name}: "${r.longest}" needs ${r.needed.toFixed(1)}, has ${r.width.toFixed(1)}`,
      ),
    ).toEqual([]);
  });
}
