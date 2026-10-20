/** Window width (CSS px) at which the stats panel moves from beside the chart to under it. */
export const LAYOUT_BREAKPOINT_PX = 1200;

/** `wide`: stats panel beside the chart; `stacked`: stats panel under the chart. */
export type Layout = "wide" | "stacked";

/** The layout used for a window `width` CSS px wide. Mirrors the CSS media query in App.svelte. */
export function layoutFor(width: number): Layout {
  return width < LAYOUT_BREAKPOINT_PX ? "stacked" : "wide";
}
