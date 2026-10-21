/**
 * AC-AH2AEK.6 (keyboard part): Tab runs top bar → readout → chart → stats, and
 * every control Tab reaches shows a visible focus indicator.
 */

import { expect, test, type Page } from "@playwright/test";
import { open, streamDemo, useTheme, type Theme } from "./helpers";

/** Page regions in their expected Tab order. */
const REGIONS = [
  { name: "top bar", selector: "header.top-bar" },
  { name: "readout", selector: "section.readout" },
  { name: "chart", selector: "section.chart" },
  { name: "stats", selector: "aside.stats" },
] as const;

interface Stop {
  region: string;
  label: string;
  /** Outline or box-shadow drawn on the focused element. */
  indicator: boolean;
}

/** Where focus is now: its region, a label and whether it draws a focus indicator. */
function focusedStop(page: Page): Promise<Stop | null> {
  return page.evaluate(
    (regions) => {
      const el = document.activeElement as HTMLElement | null;
      if (!el || el === document.body) return null;
      const region = regions.find((r) => el.closest(r.selector) !== null)?.name ?? "elsewhere";
      const style = getComputedStyle(el);
      const outline = style.outlineStyle !== "none" && parseFloat(style.outlineWidth) > 0;
      const shadow = style.boxShadow !== "none" && style.boxShadow !== "";
      const label = (el.getAttribute("aria-label") ?? el.innerText ?? el.tagName)
        .replace(/\s+/g, " ")
        .trim()
        .slice(0, 40);
      return { region, label, indicator: outline || shadow };
    },
    REGIONS as unknown as { name: string; selector: string }[],
  );
}

/** Tab from the top of the page until focus leaves the page or comes back round.
 * WebKit (as Safari) tabs only to form fields unless Option is held. */
async function tabStops(page: Page, key: string): Promise<Stop[]> {
  await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
  const stops: Stop[] = [];
  for (let i = 0; i < 80; i++) {
    await page.keyboard.press(key);
    const stop = await focusedStop(page);
    if (stop === null) break;
    if (stops.length > 0 && stop.region === "top bar" && stops.at(-1)?.region !== "top bar") break;
    stops.push(stop);
  }
  return stops;
}

for (const theme of ["light", "dark"] as Theme[]) {
  test(`Tab runs top bar → readout → chart → stats with a visible focus ring, ${theme} (AC-AH2AEK.6)`, async ({
    page,
    browserName,
  }) => {
    await page.setViewportSize({ width: 1600, height: 900 });
    await useTheme(page, theme);
    await open(page);
    await streamDemo(page);
    const stops = await tabStops(page, browserName === "webkit" ? "Alt+Tab" : "Tab");
    const order = REGIONS.map((r) => r.name as string);
    const regions = stops.map((s) => s.region);
    expect(regions, "every Tab stop is in a known region").not.toContain("elsewhere");
    for (const name of ["top bar", "chart", "stats"]) {
      expect(regions, `Tab reaches the ${name}`).toContain(name);
    }
    // Regions come in order and are never revisited.
    const ranks = regions.map((r) => order.indexOf(r));
    expect(ranks, `Tab order: ${stops.map((s) => `${s.region}:${s.label}`).join(" | ")}`).toEqual(
      [...ranks].sort((a, b) => a - b),
    );
    const plain = stops.filter((s) => !s.indicator).map((s) => `${s.region}:${s.label}`);
    expect(plain, "controls without a visible focus indicator").toEqual([]);
  });
}
