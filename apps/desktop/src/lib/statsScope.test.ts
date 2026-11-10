/**
 * FR PBDEQE: what the File tab's stats cover.
 *
 * AC-PBDEQE.2: a scope label, "Whole file" or "Shown range" with the range's start and
 * end times (in the File tab's range format).
 * AC-PBDEQE.3: Shown range is offered only while the chart is zoomed; the stats of the
 * range on screen come from its range stats, as a stats snapshot the stats view formats.
 * AC-PBDEQE.5: the live Stats panel's label.
 */

import { describe, expect, it } from "vitest";
import type { RangeStats } from "./backend";
import { formatSessionTime } from "./chartSeries";
import { rangeDecimals } from "./chartView";
import { LIVE_STATS_SCOPE, rangeStatsSnapshot, statsView } from "./stats";
import { effectiveScope, statsScopeLabel } from "./statsScope";

const zoom = { t0Ms: 4_000, t1Ms: 6_500 };

describe("statsScopeLabel (AC-PBDEQE.2)", () => {
  it("says Whole file while the stats cover the file", () => {
    expect(statsScopeLabel("file", null)).toBe("Whole file");
    expect(statsScopeLabel("file", zoom)).toBe("Whole file");
  });

  it("says Shown range with its start and end while they cover the range", () => {
    expect(statsScopeLabel("range", zoom)).toBe("Shown range, 4.0 s to 6.5 s");
  });

  it("writes the times as the File tab's range text does", () => {
    for (const r of [
      { t0Ms: 1_234, t1Ms: 1_290 },
      { t0Ms: 61_000, t1Ms: 125_000 },
    ]) {
      const d = rangeDecimals(r.t1Ms - r.t0Ms);
      expect(statsScopeLabel("range", r)).toBe(
        `Shown range, ${formatSessionTime(r.t0Ms, d)} to ${formatSessionTime(r.t1Ms, d)}`,
      );
    }
  });
});

describe("effectiveScope (AC-PBDEQE.3)", () => {
  it("is the range only while the chart is zoomed", () => {
    expect(effectiveScope("range", zoom)).toBe("range");
    expect(effectiveScope("range", null)).toBe("file");
  });

  it("Whole file stays the whole file, zoomed or not", () => {
    expect(effectiveScope("file", zoom)).toBe("file");
    expect(effectiveScope("file", null)).toBe("file");
  });
});

const range: RangeStats = {
  samples: 2_501,
  charge_ah: 0.000_7,
  energy_wh: 0.003_5,
  vbus_avg_v: 5.001,
  ibus_avg_a: 1.002,
  power_avg_w: 5.011,
  vbus_min_v: 4.99,
  vbus_max_v: 5.01,
  ibus_min_a: 0.98,
  ibus_max_a: 1.02,
  power_min_w: 4.89,
  power_max_w: 5.11,
};

describe("rangeStatsSnapshot (AC-PBDEQE.3)", () => {
  it("turns range stats and the range into a stats snapshot", () => {
    expect(rangeStatsSnapshot(range, zoom)).toEqual({
      elapsed_ms: 2_500,
      samples: 2_501,
      charge_ah: 0.000_7,
      energy_wh: 0.003_5,
      vbus: { min: 4.99, max: 5.01, avg: 5.001 },
      ibus: { min: 0.98, max: 1.02, avg: 1.002 },
      power: { min: 4.89, max: 5.11, avg: 5.011 },
    });
  });

  it("formats through the stats view like the whole file", () => {
    const view = statsView(rangeStatsSnapshot(range, { t0Ms: 0, t1Ms: 3_723_000 }));
    expect(view.elapsed).toBe("1:02:03");
    expect(view.vbus).toEqual({ min: "4.990 V", max: "5.010 V", avg: "5.001 V" });
    expect(view.power.max).toBe("5.110 W");
  });
});

describe("the live Stats panel's label (AC-PBDEQE.5)", () => {
  it("says what the live stats count", () => {
    expect(LIVE_STATS_SCOPE).toBe("Since connect or Reset");
  });
});
