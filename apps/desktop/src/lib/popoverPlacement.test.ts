// FR CW66T1, AC .7: the popover stays inside the window and off its own trigger.
import { describe, expect, it } from "vitest";
import { placePopover } from "./popoverPlacement";

interface Box {
  left: number;
  top: number;
  width: number;
  height: number;
}

function overlaps(a: Box, b: Box): boolean {
  return (
    a.left < b.left + b.width &&
    b.left < a.left + a.width &&
    a.top < b.top + b.height &&
    b.top < a.top + a.height
  );
}

const POPOVER = { width: 320, height: 180 };
const VIEWPORTS = [
  { width: 375, height: 667 },
  { width: 1280, height: 800 },
];

/** Triggers in the corners, on the edges and in the middle of a viewport. */
function triggers(vw: number, vh: number): Box[] {
  const s = 20;
  const xs = [0, 4, vw / 2 - s / 2, vw - s - 4, vw - s];
  const ys = [0, 4, vh / 2 - s / 2, vh - s - 4, vh - s];
  return xs.flatMap((left) => ys.map((top) => ({ left, top, width: s, height: s })));
}

describe("AC-CW66T1.7: placement inside the window, beside the trigger", () => {
  for (const vp of VIEWPORTS) {
    it(`${vp.width} px: always fully inside the window`, () => {
      for (const t of triggers(vp.width, vp.height)) {
        const p = placePopover(t, POPOVER, vp);
        const where = `trigger at ${t.left},${t.top}`;
        expect(p.left, where).toBeGreaterThanOrEqual(0);
        expect(p.top, where).toBeGreaterThanOrEqual(0);
        expect(p.left + POPOVER.width, where).toBeLessThanOrEqual(vp.width);
        expect(p.top + POPOVER.height, where).toBeLessThanOrEqual(vp.height);
      }
    });

    it(`${vp.width} px: never covers its trigger when there is room`, () => {
      for (const t of triggers(vp.width, vp.height)) {
        const p = placePopover(t, POPOVER, vp);
        const box = { left: p.left, top: p.top, ...POPOVER };
        expect(overlaps(box, t), `trigger at ${t.left},${t.top}`).toBe(false);
        expect(["top", "bottom", "left", "right"]).toContain(p.side);
      }
    });
  }

  it("prefers below the trigger, a gap away, when that fits", () => {
    const t = { left: 100, top: 100, width: 20, height: 20 };
    const p = placePopover(t, POPOVER, { width: 1280, height: 800 }, 8);
    expect(p.side).toBe("bottom");
    expect(p.top).toBe(128);
  });

  it("flips above a trigger at the bottom of the window", () => {
    const t = { left: 100, top: 760, width: 20, height: 20 };
    const p = placePopover(t, POPOVER, { width: 1280, height: 800 }, 8);
    expect(p.side).toBe("top");
    expect(p.top + POPOVER.height).toBeLessThanOrEqual(752);
  });

  it("a popover wider than the window is pinned to its left edge, still on screen", () => {
    const t = { left: 300, top: 300, width: 20, height: 20 };
    const p = placePopover(t, { width: 500, height: 100 }, { width: 375, height: 667 });
    expect(p.left).toBe(0);
    expect(p.top).toBeGreaterThanOrEqual(0);
  });
});
