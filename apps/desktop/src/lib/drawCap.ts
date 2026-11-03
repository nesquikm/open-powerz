/**
 * Iteration caps for the per-frame canvas loops over data (FR C6VR9K, AC .5): a layout
 * fault (a hidden chart laid out at 1 × 1, points a million px off the plot) can never make
 * one frame cost millions of canvas calls.
 */

/** The cap for a tiny or unlaid plot. */
const FLOOR = 4096;
/** Iterations allowed per pixel of the plot's width + height. */
const PER_PX = 16;
/** Sides past this count as this (device px): an absurd layout stays bounded. */
const MAX_SIDE = 16_384;

/** A side's length clamped to `[0, MAX_SIDE]`; NaN and negatives count as zero. */
function side(px: number): number {
  return px > 0 ? Math.min(px, MAX_SIDE) : 0;
}

/** The most iterations one draw loop may take on a plot `widthPx` × `heightPx` (device
 * px): a whole number, never under {@link FLOOR}, growing with the plot's size. */
export function drawLoopCap(widthPx: number, heightPx: number): number {
  return Math.max(FLOOR, Math.ceil(PER_PX * (side(widthPx) + side(heightPx))));
}

/** The step a loop over `count` items takes so it visits at most `cap` of them. */
export function capStride(count: number, cap: number): number {
  if (!(count > 0) || !(cap >= 1)) return 1;
  return Math.max(1, Math.ceil(count / Math.floor(cap)));
}
