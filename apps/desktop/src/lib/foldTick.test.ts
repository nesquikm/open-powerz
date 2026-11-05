/**
 * FR AXHK2W (AC-AXHK2W.11): the chart marks where the session history's full resolution
 * starts with a faint tick that says the data before it is coarser.
 *
 * API pinned here, beyond the FR's Technical Design:
 *
 * - `foldTick(boundaryMs, view, widthPx)` in `foldTick.ts`: where the tick goes, as
 *   `{ x, label }` with `x` in px from the plot's left edge (linear over
 *   `view.t0Ms..view.t1Ms` → `0..widthPx`), or `null` with no boundary, a boundary
 *   outside the view, an empty view or no width;
 * - `FOLD_TICK_LABEL = "coarser before here"`, the tick's label.
 */

import { describe, expect, it } from "vitest";
import { FOLD_TICK_LABEL, foldTick } from "./foldTick";

describe("foldTick (AC-AXHK2W.11)", () => {
  it("says the data before it is coarser", () => {
    expect(FOLD_TICK_LABEL).toBe("coarser before here");
  });

  it("places the tick linearly over the view", () => {
    expect(foldTick(2_500, { t0Ms: 0, t1Ms: 10_000 }, 400)).toEqual({
      x: 100,
      label: FOLD_TICK_LABEL,
    });
    expect(foldTick(15_000, { t0Ms: 10_000, t1Ms: 20_000 }, 800)?.x).toBeCloseTo(400, 9);
  });

  it("keeps a tick on either edge of the view", () => {
    expect(foldTick(10_000, { t0Ms: 10_000, t1Ms: 20_000 }, 800)?.x).toBe(0);
    expect(foldTick(20_000, { t0Ms: 10_000, t1Ms: 20_000 }, 800)?.x).toBe(800);
  });

  it("draws nothing with no boundary or one outside the view", () => {
    expect(foldTick(null, { t0Ms: 0, t1Ms: 10_000 }, 400)).toBeNull();
    expect(foldTick(9_999, { t0Ms: 10_000, t1Ms: 20_000 }, 400)).toBeNull();
    expect(foldTick(20_001, { t0Ms: 10_000, t1Ms: 20_000 }, 400)).toBeNull();
  });

  it("draws nothing on an empty view or a plot with no width", () => {
    expect(foldTick(5_000, { t0Ms: 5_000, t1Ms: 5_000 }, 400)).toBeNull();
    expect(foldTick(5_000, { t0Ms: 0, t1Ms: 10_000 }, 0)).toBeNull();
  });
});
