/**
 * FR AXHK2W (AC-AXHK2W.11): the fold tick's label sits on a small chip at the top of the
 * plot, at the size of the other chart labels, readable over the data in both themes.
 *
 * API pinned here, beyond the FR's Technical Design:
 *
 * - `FOLD_CHIP_FONT_PX = 11` in `foldTick.ts`: the label's size in CSS px, the size the
 *   chart draws its other in-plot labels (the gap labels) at;
 * - `foldChip(x, textWidth, plot, ratio = 1)`: the chip's box `{ left, top, right,
 *   bottom }` in canvas px, for a tick at canvas x `x` and a label `textWidth` canvas px
 *   wide, in plot area `plot = { left, top, width, height }` (canvas px) at device pixel
 *   ratio `ratio`; on the coarser (left) side of the tick when it fits there, else on
 *   its right; always inside the plot, at its top; `null` when the plot is too narrow
 *   for it;
 * - `foldChipColors(theme)`: the label's text colour and the chip's opaque fill,
 *   `#rrggbb`, at least 4.5:1 apart.
 * - `foldAtFromUrl(search)` in `mockBackend.ts`: `?foldat=<n>` in the page URL, a
 *   positive whole number of samples, else null; the mock's default options take it
 *   as `historyFullSamples`.
 */

import { describe, expect, it } from "vitest";
import { FOLD_CHIP_FONT_PX, foldChip, foldChipColors } from "./foldTick";
import { foldAtFromUrl } from "./mockBackend";
import { contrastRatio } from "./theme";

const PLOT = { left: 50, top: 20, width: 600, height: 300 };

describe("fold chip size (AC-AXHK2W.11)", () => {
  it("draws the label at the chart's other label size", () => {
    expect(FOLD_CHIP_FONT_PX).toBe(11);
  });
});

describe("foldChip (AC-AXHK2W.11)", () => {
  it("sits at the top of the plot, around the label", () => {
    const chip = foldChip(400, 110, PLOT);
    expect(chip).not.toBeNull();
    const c = chip!;
    expect(c.top).toBeGreaterThanOrEqual(PLOT.top);
    expect(c.top).toBeLessThanOrEqual(PLOT.top + 8);
    expect(c.bottom - c.top).toBeGreaterThanOrEqual(FOLD_CHIP_FONT_PX);
    expect(c.bottom - c.top).toBeLessThanOrEqual(FOLD_CHIP_FONT_PX * 2.2);
    expect(c.right - c.left).toBeGreaterThan(110);
    expect(c.right - c.left).toBeLessThanOrEqual(110 + 24);
  });

  it("goes on the coarser side of the tick when it fits there", () => {
    const c = foldChip(400, 110, PLOT)!;
    expect(c.right).toBeLessThanOrEqual(400);
    expect(400 - c.right).toBeLessThanOrEqual(8);
    expect(c.left).toBeGreaterThanOrEqual(PLOT.left);
  });

  it("flips to the right of a tick near the plot's left edge", () => {
    const c = foldChip(PLOT.left + 30, 110, PLOT)!;
    expect(c.left).toBeGreaterThanOrEqual(PLOT.left + 30);
    expect(c.left - (PLOT.left + 30)).toBeLessThanOrEqual(8);
    expect(c.right).toBeLessThanOrEqual(PLOT.left + PLOT.width);
  });

  it("stays inside the plot for a tick on either edge", () => {
    for (const x of [PLOT.left, PLOT.left + PLOT.width]) {
      const c = foldChip(x, 110, PLOT)!;
      expect(c.left).toBeGreaterThanOrEqual(PLOT.left);
      expect(c.right).toBeLessThanOrEqual(PLOT.left + PLOT.width);
    }
  });

  it("scales with the device pixel ratio", () => {
    const one = foldChip(400, 110, PLOT, 1)!;
    const plot2 = { left: 100, top: 40, width: 1200, height: 600 };
    const two = foldChip(800, 220, plot2, 2)!;
    expect(two.bottom - two.top).toBeCloseTo(2 * (one.bottom - one.top), 6);
    expect(two.right - two.left).toBeCloseTo(2 * (one.right - one.left), 6);
    expect(two.top - plot2.top).toBeCloseTo(2 * (one.top - PLOT.top), 6);
  });

  it("draws no chip in a plot too narrow for it", () => {
    expect(foldChip(60, 110, { left: 50, top: 20, width: 80, height: 300 })).toBeNull();
  });
});

describe("foldChipColors (AC-AXHK2W.11)", () => {
  for (const theme of ["light", "dark"] as const) {
    it(`keeps the label readable on an opaque chip in the ${theme} theme`, () => {
      const { text, background } = foldChipColors(theme);
      expect(text).toMatch(/^#[0-9a-f]{6}$/i);
      expect(background).toMatch(/^#[0-9a-f]{6}$/i);
      expect(contrastRatio(text, background)).toBeGreaterThanOrEqual(4.5);
    });
  }

  it("follows the theme", () => {
    expect(foldChipColors("light").background).not.toBe(foldChipColors("dark").background);
  });
});

describe("foldAtFromUrl (AC-AXHK2W.11)", () => {
  it("reads ?foldat= as the mock's full-resolution limit", () => {
    expect(foldAtFromUrl("?foldat=3000")).toBe(3000);
    expect(foldAtFromUrl("?mock=outage&foldat=12")).toBe(12);
  });

  it("ignores a missing or malformed value", () => {
    expect(foldAtFromUrl(undefined)).toBeNull();
    expect(foldAtFromUrl("")).toBeNull();
    expect(foldAtFromUrl("?foldat=")).toBeNull();
    expect(foldAtFromUrl("?foldat=-5")).toBeNull();
    expect(foldAtFromUrl("?foldat=1.5")).toBeNull();
    expect(foldAtFromUrl("?foldat=abc")).toBeNull();
  });
});
