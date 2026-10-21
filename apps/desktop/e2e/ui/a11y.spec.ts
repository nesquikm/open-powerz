/**
 * AC-AH2AEN.6: an axe scan of the main view, disconnected and streaming, in
 * both themes, at 900 px and 1600 px wide; any serious or critical violation
 * fails the run.
 */

import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";
import {
  chartSeriesLengths,
  open,
  readoutValue,
  statsElapsedSeconds,
  streamDemo,
  useTheme,
  type Theme,
} from "./helpers";

const THEMES: readonly Theme[] = ["light", "dark"];
const WIDTHS = [900, 1600] as const;
const STATES = ["disconnected", "streaming"] as const;

for (const theme of THEMES) {
  for (const width of WIDTHS) {
    for (const state of STATES) {
      test(`axe: ${state}, ${theme}, ${width} px (AC-AH2AEN.6)`, async ({ page }) => {
        await page.setViewportSize({ width, height: 900 });
        await useTheme(page, theme);
        await open(page);
        await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
        if (state === "streaming") {
          await streamDemo(page);
          // Wait until the readout, stats and chart have filled in.
          await expect
            .poll(async () => {
              const n = await chartSeriesLengths(page);
              return n.vbus > 0 && n.ibus > 0;
            })
            .toBe(true);
          await expect.poll(() => readoutValue(page, "VBUS")).not.toBeNaN();
          await expect.poll(() => statsElapsedSeconds(page)).toBeGreaterThan(0);
        }
        const results = await new AxeBuilder({ page })
          .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa", "best-practice"])
          .analyze();
        const blocking = results.violations.filter(
          (v) => v.impact === "serious" || v.impact === "critical",
        );
        expect(
          blocking.map(
            (v) =>
              `${v.impact} ${v.id}: ${v.help} at ${v.nodes.map((n) => n.target.join(" ")).join("; ")}`,
          ),
          "no serious or critical axe violations",
        ).toEqual([]);
      });
    }
  }
}
