/**
 * FR PBDEQE (AC-PBDEQE.1): the browser-side range stats (the web edition and the mock
 * backend share `HistoryColumns`) answer the minimum and maximum of VBUS, IBUS and Power
 * over the samples in range, beside the averages, as the Rust `range_stats` does.
 */

import { describe, expect, it } from "vitest";
import type { RangeStats, Sample } from "./backend";
import { HistoryColumns } from "./history";

function sample(tMs: number, v: number, i: number): Sample {
  return { t_ms: tMs, vbus_v: v, ibus_a: i, cc1_v: 0, cc2_v: 0, dp_v: 0, dm_v: 0 };
}

/** Eleven seconds at 5 V / 1 A, one sample a second, with a VBUS dip to 4.5 V at 3 s, a
 * 2.5 A peak at 7 s and a reverse 0.5 A at 8 s. Float64 columns, as an opened file. */
function plain(): HistoryColumns {
  const h = new HistoryColumns(16, "f64");
  for (let s = 0; s <= 10; s++) {
    const v = s === 3 ? 4.5 : 5;
    const i = s === 7 ? 2.5 : s === 8 ? -0.5 : 1;
    h.push(sample(s * 1000, v, i));
  }
  return h;
}

type Extremes = Pick<
  RangeStats,
  "vbus_min_v" | "vbus_max_v" | "ibus_min_a" | "ibus_max_a" | "power_min_w" | "power_max_w"
>;

function expectExtremes(got: RangeStats | null, want: Extremes): void {
  expect(got).not.toBeNull();
  for (const key of Object.keys(want) as (keyof Extremes)[]) {
    expect(got?.[key], key).toBeCloseTo(want[key], 6);
  }
}

describe("HistoryColumns.rangeStats min and max (AC-PBDEQE.1)", () => {
  it("answers the extremes of the whole range beside the averages", () => {
    const s = plain().rangeStats(0, 10_000);
    expectExtremes(s, {
      vbus_min_v: 4.5,
      vbus_max_v: 5,
      ibus_min_a: -0.5,
      ibus_max_a: 2.5,
      power_min_w: -2.5,
      power_max_w: 12.5,
    });
    expect(s?.vbus_avg_v).toBeCloseTo((10 * 5 + 4.5) / 11, 9);
  });

  it("a range that leaves out the dip has the range's own minimum", () => {
    const s = plain().rangeStats(4_000, 6_000);
    expectExtremes(s, {
      vbus_min_v: 5,
      vbus_max_v: 5,
      ibus_min_a: 1,
      ibus_max_a: 1,
      power_min_w: 5,
      power_max_w: 5,
    });
  });

  it("either order answers the same", () => {
    const h = plain();
    expect(h.rangeStats(6_000, 4_000)).toEqual(h.rangeStats(4_000, 6_000));
  });

  it("one sample in range is its own min and max", () => {
    const s = plain().rangeStats(2_500, 3_500);
    expect(s?.samples).toBe(1);
    expectExtremes(s, {
      vbus_min_v: 4.5,
      vbus_max_v: 4.5,
      ibus_min_a: 1,
      ibus_max_a: 1,
      power_min_w: 4.5,
      power_max_w: 4.5,
    });
  });

  it("an empty range is still null", () => {
    expect(plain().rangeStats(20_000, 30_000)).toBeNull();
  });
});
