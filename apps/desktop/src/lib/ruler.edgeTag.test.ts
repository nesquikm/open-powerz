/**
 * FR AXHK2V (AC-AXHK2V.11): the tag a ruler cursor shows at the edge of the view.
 *
 * API pinned here, beyond the FR's Technical Design:
 *
 * - `rulerTag(name, side, time)` in `ruler.ts` is the tag's text: the name alone while
 *   the cursor is in view; at an edge an arrow towards it, the name, " · " and the time.
 */

import { describe, expect, it } from "vitest";
import { rulerTag } from "./ruler";

describe("a ruler cursor's edge tag (AC-AXHK2V.11)", () => {
  it("AC-AXHK2V.11: before the view: ◀, the name, a separator, the time", () => {
    expect(rulerTag("T1", "before", "0.000 s")).toBe("◀ T1 · 0.000 s");
  });

  it("AC-AXHK2V.11: after the view: the name, a separator, the time, ▶", () => {
    expect(rulerTag("T2", "after", "1:02.345")).toBe("T2 · 1:02.345 ▶");
  });

  it("AC-AXHK2V.11: in view: the name alone", () => {
    expect(rulerTag("T1", "in", "0.000 s")).toBe("T1");
  });
});
