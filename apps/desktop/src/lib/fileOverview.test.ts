// FR 09H204: the File tab's overview strip state, from the file's extent and the range
// on screen (AC-09H204.2, AC-09H204.4).
import { describe, expect, it } from "vitest";
import { fileOverviewState } from "./fileOverview";

const extent = { t0Ms: 1_000, t1Ms: 95_000 };

describe("fileOverviewState", () => {
  it("is null with no file open, so the strip clears (AC-09H204.4)", () => {
    expect(fileOverviewState(null, null)).toBeNull();
    expect(fileOverviewState(null, { t0Ms: 2_000, t1Ms: 3_000 })).toBeNull();
  });

  it("spans the whole file when not zoomed (AC-09H204.2)", () => {
    expect(fileOverviewState(extent, null)).toEqual({
      range: { t0Ms: 1_000, t1Ms: 95_000 },
      session: { firstMs: 1_000, lastMs: 95_000 },
      originMs: 0,
    });
  });

  it("covers only the zoomed part after a zoom, within the whole file (AC-09H204.2)", () => {
    const shown = { t0Ms: 21_000, t1Ms: 40_000 };
    const state = fileOverviewState(extent, shown);
    expect(state?.range).toEqual({ t0Ms: 21_000, t1Ms: 40_000 });
    expect(state?.session).toEqual({ firstMs: 1_000, lastMs: 95_000 });
    expect(state?.originMs).toBe(0);
  });

  it("follows a new file's extent, keeping nothing of the old one (AC-09H204.4)", () => {
    fileOverviewState(extent, { t0Ms: 21_000, t1Ms: 40_000 });
    const next = fileOverviewState({ t0Ms: 0, t1Ms: 10_000 }, null);
    expect(next?.session).toEqual({ firstMs: 0, lastMs: 10_000 });
    expect(next?.range).toEqual({ t0Ms: 0, t1Ms: 10_000 });
  });
});
