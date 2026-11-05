/**
 * The chart's fold tick (FR AXHK2W, AC-AXHK2W.11): where the session history's full
 * resolution starts. The data before it is folded into coarser blocks.
 */

import type { LabelBox } from "./chartExtras";
import { THEME_COLORS, type ResolvedTheme } from "./theme";

/** The tick's label. */
export const FOLD_TICK_LABEL = "coarser before here";

/** Where the fold tick goes: `x` in px from the plot's left edge. */
export interface FoldTick {
  x: number;
  label: string;
}

/** The fold tick at `boundaryMs`, linear over `view.t0Ms..view.t1Ms` → `0..widthPx`;
 * `null` with no boundary, a boundary outside the view, an empty view or no width. */
export function foldTick(
  boundaryMs: number | null,
  view: { t0Ms: number; t1Ms: number },
  widthPx: number,
): FoldTick | null {
  if (boundaryMs === null || !(widthPx > 0)) return null;
  const span = view.t1Ms - view.t0Ms;
  if (!(span > 0) || boundaryMs < view.t0Ms || boundaryMs > view.t1Ms) return null;
  return { x: ((boundaryMs - view.t0Ms) / span) * widthPx, label: FOLD_TICK_LABEL };
}

/** The label's size, CSS px: the size the chart draws its other in-plot labels at. */
export const FOLD_CHIP_FONT_PX = 11;

/** Padding around the label, the gap to the tick and the inset from the plot's top,
 * CSS px. */
const CHIP_PAD_X = 6;
const CHIP_PAD_Y = 4;
const CHIP_GAP = 4;
const CHIP_INSET = 4;

/** The fold label's chip, canvas px, for a tick at canvas x `x` and a label `textWidth`
 * canvas px wide, in plot area `plot` (canvas px) at device pixel ratio `ratio`: at the
 * top of the plot, on the coarser (left) side of the tick when it fits there, else on
 * its right, always inside the plot; `null` when the plot is too narrow for it. */
export function foldChip(
  x: number,
  textWidth: number,
  plot: { left: number; top: number; width: number; height: number },
  ratio = 1,
): LabelBox | null {
  const width = textWidth + 2 * CHIP_PAD_X * ratio;
  const height = (FOLD_CHIP_FONT_PX + 2 * CHIP_PAD_Y) * ratio;
  const plotRight = plot.left + plot.width;
  if (!(width <= plot.width) || !(height <= plot.height)) return null;
  const gap = CHIP_GAP * ratio;
  let left: number;
  if (x - gap - width >= plot.left) left = x - gap - width;
  else if (x + gap + width <= plotRight) left = x + gap;
  else left = Math.max(plot.left, Math.min(x - width / 2, plotRight - width));
  const top = plot.top + CHIP_INSET * ratio;
  return { left, top, right: left + width, bottom: top + height };
}

/** The fold label's text colour and its chip's opaque fill in `theme`. */
export function foldChipColors(theme: ResolvedTheme): { text: string; background: string } {
  const palette = THEME_COLORS[theme];
  return { text: palette.muted, background: palette.surface };
}
