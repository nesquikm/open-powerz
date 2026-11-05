/**
 * FR AXHK2V (AC-AXHK2V.14): ruler tags never overlap.
 *
 * API pinned here, beyond the FR's Technical Design:
 *
 * - `rulerTagOffsets(tags)` in `ruler.ts` takes each cursor tag's box as laid out
 *   (`{ left, top, right, bottom }`, px, one frame, in cursor order) and returns how far
 *   to move each one down, px (≥ 0, one number per tag, in order). Tags that do not touch
 *   stay where they are (all 0); tags that would touch are moved apart so that, once
 *   moved, no two tags touch or overlap. At least one tag stays put.
 */

import { describe, expect, it } from "vitest";
import { rulerTagOffsets } from "./ruler";

interface Box {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

/** A tag `width` px wide at `left`, on the row at the top of the plot. */
function tag(left: number, width: number): Box {
  return { left, top: 0, right: left + width, bottom: 14 };
}

/** True when `a` and `b` overlap or touch (share an edge). */
function touch(a: Box, b: Box): boolean {
  return a.left <= b.right && b.left <= a.right && a.top <= b.bottom && b.top <= a.bottom;
}

function moved(boxes: readonly Box[], dy: readonly number[]): Box[] {
  return boxes.map((b, k) => ({
    ...b,
    top: b.top + (dy[k] ?? 0),
    bottom: b.bottom + (dy[k] ?? 0),
  }));
}

describe("ruler tags never overlap (AC-AXHK2V.14)", () => {
  it("AC-AXHK2V.14: tags apart from each other stay where they are", () => {
    expect(rulerTagOffsets([tag(10, 90), tag(300, 20)])).toEqual([0, 0]);
  });

  it("AC-AXHK2V.14: T2's in-view tag over T1's edge tag is moved apart", () => {
    // The screenshot: T1 behind the view (edge tag "◀ T1 · 2.345 s" at the left edge),
    // T2 just inside it (tag "T2" a few px to the right, on top of T1's).
    const tags = [tag(14, 92), tag(20, 20)];
    const dy = rulerTagOffsets(tags);
    expect(dy).toHaveLength(2);
    for (const d of dy) expect(d).toBeGreaterThanOrEqual(0);
    expect(Math.min(...dy)).toBe(0);
    const [a, b] = moved(tags, dy);
    expect(touch(a!, b!), "the moved tags still touch").toBe(false);
  });

  it("AC-AXHK2V.14: tags that would only touch edge to edge are moved apart too", () => {
    const tags = [tag(14, 60), tag(74, 20)];
    const [a, b] = moved(tags, rulerTagOffsets(tags));
    expect(touch(a!, b!)).toBe(false);
  });

  it("AC-AXHK2V.14: both cursors' edge tags at the same edge are moved apart", () => {
    // Both cursors behind the view: both tags at the left edge.
    const tags = [tag(14, 92), tag(14, 92)];
    const [a, b] = moved(tags, rulerTagOffsets(tags));
    expect(touch(a!, b!)).toBe(false);
  });

  it("AC-AXHK2V.14: whatever the positions, the moved tags never touch", () => {
    for (let x = -40; x <= 140; x += 7) {
      for (const w of [20, 60, 110]) {
        const tags = [tag(14, 92), tag(x, w)];
        const dy = rulerTagOffsets(tags);
        const [a, b] = moved(tags, dy);
        expect(touch(a!, b!), `T2 at ${x}, ${w} px wide`).toBe(false);
        if (!touch(tags[0]!, tags[1]!)) expect(dy).toEqual([0, 0]);
      }
    }
  });
});
