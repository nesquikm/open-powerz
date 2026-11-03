/**
 * Where a glossary popover (FR CW66T1, AC .7) goes: beside its trigger, never over it when
 * there is room, and always fully inside the window. Pure, so it is tested without a DOM.
 */

export interface Rect {
  left: number;
  top: number;
  width: number;
  height: number;
}

export interface Size {
  width: number;
  height: number;
}

export type Side = "top" | "bottom" | "left" | "right";

export interface Placement {
  left: number;
  top: number;
  side: Side;
}

/** Space kept between the popover and the window edge when it fits. */
const MARGIN = 8;

/** Keeps `size` inside `[0, view]` along one axis, a margin from the edges when it fits,
 * pinned to the start edge when it is larger than the window. */
function clampAxis(pos: number, size: number, view: number): number {
  if (size + 2 * MARGIN <= view) return Math.max(MARGIN, Math.min(pos, view - size - MARGIN));
  return Math.max(0, Math.min(pos, view - size));
}

/**
 * Places a `popover` of the given size next to `trigger` inside `viewport`: below it when
 * that fits, else above, else to the right, else to the left. When no side fits, it goes
 * below, clamped into the window.
 */
export function placePopover(trigger: Rect, popover: Size, viewport: Size, gap = 4): Placement {
  const { width: w, height: h } = popover;
  const { width: vw, height: vh } = viewport;
  const right = trigger.left + trigger.width;
  const bottom = trigger.top + trigger.height;
  const x = clampAxis(trigger.left, w, vw);
  const y = clampAxis(trigger.top, h, vh);

  if (bottom + gap + h <= vh - MARGIN) return { left: x, top: bottom + gap, side: "bottom" };
  if (trigger.top - gap - h >= MARGIN) return { left: x, top: trigger.top - gap - h, side: "top" };
  if (right + gap + w <= vw - MARGIN) return { left: right + gap, top: y, side: "right" };
  if (trigger.left - gap - w >= MARGIN)
    return { left: trigger.left - gap - w, top: y, side: "left" };
  return { left: x, top: clampAxis(bottom + gap, h, vh), side: "bottom" };
}
