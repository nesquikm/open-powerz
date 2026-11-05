/**
 * FR AXHK2T: the chart ruler's cursor state and readout, a pure module (`ruler.ts`).
 *
 * API pinned here, beyond the FR's Technical Design:
 *
 * - `placeRuler(view: TimeRange): Ruler` puts T1 a third and T2 two thirds into the view;
 * - `moveRulerCursor(r, id, ms, bounds)` moves one cursor, clamped into `bounds`;
 * - `rulerStepMs(view, widthPx, coarse)`: one pixel of the view, or a tenth of it
 *   (Shift), the step the playhead takes too; `stepRulerCursor(r, id, direction, stepMs,
 *   bounds)` moves one cursor by it, clamped;
 * - `nearestRulerCursor(r, ms)`: the cursor a press at `ms` grabs;
 * - `rulerSpan(r)`: the two times in order, as the backend's `range_stats` takes them;
 * - `rulerValues(r, stats)`: delta time from the cursors, everything else from the
 *   backend's `RangeStats` (null before it answers);
 * - `rulerReadout(values)`: six `{ name, label, text }` items, the accessible name, the
 *   short visible label and the formatted value.
 */

import { describe, expect, it } from "vitest";
import type { RangeStats } from "./backend";
import { DASH } from "./format";
import {
  moveRulerCursor,
  nearestRulerCursor,
  placeRuler,
  rulerAnnouncement,
  rulerCursorSide,
  rulerKey,
  rulerReadout,
  rulerSpan,
  rulerStepMs,
  rulerValues,
  stepRulerCursor,
  type Ruler,
} from "./ruler";

const view = { t0Ms: 1_000, t1Ms: 4_000 };
const bounds = { t0Ms: 0, t1Ms: 10_000 };

const hour: RangeStats = {
  samples: 3601,
  charge_ah: 1,
  energy_wh: 5,
  vbus_avg_v: 5,
  ibus_avg_a: 1,
  power_avg_w: 5,
};

describe("placing the ruler (AC-AXHK2T.1)", () => {
  it("puts T1 a third and T2 two thirds into the view", () => {
    expect(placeRuler(view)).toEqual({ t1Ms: 2_000, t2Ms: 3_000 });
  });
});

describe("dragging a cursor (AC-AXHK2T.1)", () => {
  const r: Ruler = { t1Ms: 2_000, t2Ms: 3_000 };

  it("moves only the cursor dragged", () => {
    expect(moveRulerCursor(r, "t2", 3_500, bounds)).toEqual({ t1Ms: 2_000, t2Ms: 3_500 });
    expect(moveRulerCursor(r, "t1", 1_250, bounds)).toEqual({ t1Ms: 1_250, t2Ms: 3_000 });
  });

  it("keeps a cursor inside the bounds", () => {
    expect(moveRulerCursor(r, "t1", -500, bounds).t1Ms).toBe(0);
    expect(moveRulerCursor(r, "t2", 12_000, bounds).t2Ms).toBe(10_000);
  });

  it("lets T2 pass T1, and the span stays in order", () => {
    const crossed = moveRulerCursor(r, "t2", 1_500, bounds);
    expect(crossed).toEqual({ t1Ms: 2_000, t2Ms: 1_500 });
    expect(rulerSpan(crossed)).toEqual({ t0Ms: 1_500, t1Ms: 2_000 });
  });

  it("a press grabs the nearer cursor", () => {
    expect(nearestRulerCursor(r, 2_100)).toBe("t1");
    expect(nearestRulerCursor(r, 2_900)).toBe("t2");
    expect(nearestRulerCursor(r, 9_000)).toBe("t2");
  });
});

describe("moving a cursor by keyboard (AC-AXHK2T.3)", () => {
  const r: Ruler = { t1Ms: 2_000, t2Ms: 3_000 };

  it("steps one pixel of the view, or a tenth of it with Shift", () => {
    expect(rulerStepMs(view, 300, false)).toBe(10);
    expect(rulerStepMs(view, 300, true)).toBe(300);
  });

  it("an arrow moves the focused cursor that way, the other stays", () => {
    expect(stepRulerCursor(r, "t1", 1, 10, bounds)).toEqual({ t1Ms: 2_010, t2Ms: 3_000 });
    expect(stepRulerCursor(r, "t2", -1, 300, bounds)).toEqual({ t1Ms: 2_000, t2Ms: 2_700 });
  });

  it("stops at the bounds", () => {
    const atEnd = { t1Ms: 2_000, t2Ms: 9_995 };
    expect(stepRulerCursor(atEnd, "t2", 1, 10, bounds).t2Ms).toBe(10_000);
    const atStart = { t1Ms: 5, t2Ms: 3_000 };
    expect(stepRulerCursor(atStart, "t1", -1, 10, bounds).t1Ms).toBe(0);
  });

  it("a zero-width view still steps a whole millisecond", () => {
    expect(rulerStepMs({ t0Ms: 5, t1Ms: 5 }, 300, false)).toBeGreaterThanOrEqual(1);
    expect(rulerStepMs(view, 0, false)).toBeGreaterThan(0);
  });
});

describe("the ruler's values (AC-AXHK2T.1, .2)", () => {
  it("delta time from the cursors, charge, energy and averages from the backend", () => {
    const v = rulerValues({ t1Ms: 0, t2Ms: 3_600_000 }, hour);
    expect(v).toEqual({
      deltaMs: 3_600_000,
      chargeAh: 1,
      energyWh: 5,
      vbusV: 5,
      ibusA: 1,
      powerW: 5,
    });
  });

  it("delta time is never negative, whichever cursor leads", () => {
    expect(rulerValues({ t1Ms: 3_000, t2Ms: 1_500 }, hour).deltaMs).toBe(1_500);
  });

  it("before the backend answers, only delta time is known", () => {
    expect(rulerValues({ t1Ms: 1_000, t2Ms: 2_500 }, null)).toEqual({
      deltaMs: 1_500,
      chargeAh: null,
      energyWh: null,
      vbusV: null,
      ibusA: null,
      powerW: null,
    });
  });
});

describe("the ruler readout (AC-AXHK2T.1, .3)", () => {
  it("names, labels and formats the six values, in order", () => {
    const items = rulerReadout({
      deltaMs: 1_500,
      chargeAh: 0.0011112,
      energyWh: 0.0055561,
      vbusV: 5.0004,
      ibusA: 1.0002,
      powerW: 5.0012,
    });
    expect(items).toEqual([
      { name: "Delta time", label: "ΔT", text: "1.500 s" },
      { name: "Delta charge", label: "ΔAh", text: "0.001111 Ah" },
      { name: "Delta energy", label: "ΔWh", text: "0.005556 Wh" },
      { name: "Average voltage", label: "Avg V", text: "5.000 V" },
      { name: "Average current", label: "Avg A", text: "1.000 A" },
      { name: "Average power", label: "Avg W", text: "5.001 W" },
    ]);
  });

  it("writes negatives with a true minus sign", () => {
    const items = rulerReadout({
      deltaMs: 60_000,
      chargeAh: -0.0025,
      energyWh: -0.0125,
      vbusV: 5,
      ibusA: -0.15,
      powerW: -0.75,
    });
    expect(items.map((i) => i.text)).toEqual([
      "1:00.000",
      "−0.002500 Ah",
      "−0.012500 Wh",
      "5.000 V",
      "−0.150 A",
      "−0.750 W",
    ]);
  });

  it("an unknown value reads as a dash with its unit", () => {
    const items = rulerReadout(rulerValues({ t1Ms: 0, t2Ms: 2_000 }, null));
    expect(items[0]?.text).toBe("2.000 s");
    for (const item of items.slice(1)) expect(item.text.startsWith(DASH)).toBe(true);
    expect(items.slice(1).map((i) => i.text.split(" ").pop())).toEqual(["Ah", "Wh", "V", "A", "W"]);
  });
});

describe("a pending answer (AC-AXHK2T.1, .3)", () => {
  it("shows an ellipsis for every value not answered yet; delta time is always known", () => {
    const items = rulerReadout(rulerValues({ t1Ms: 0, t2Ms: 2_000 }, hour), true);
    expect(items[0]?.text).toBe("2.000 s");
    expect(items.slice(1).map((i) => i.text)).toEqual(["…", "…", "…", "…", "…"]);
  });
});

describe("a cursor out of view (AC-AXHK2T.3)", () => {
  it("says which side of the view a time lies on", () => {
    expect(rulerCursorSide(500, view)).toBe("before");
    expect(rulerCursorSide(2_000, view)).toBe("in");
    expect(rulerCursorSide(4_000, view)).toBe("in");
    expect(rulerCursorSide(4_001, view)).toBe("after");
  });
});

describe("the cursor keys (AC-AXHK2T.3)", () => {
  const r: Ruler = { t1Ms: 2_000, t2Ms: 3_000 };
  const key = (k: string, from: Ruler = r, shift = false) =>
    rulerKey(from, "t1", k, shift, view, bounds, 3_000);

  it("arrows step one pixel of the view, Shift a tenth, and stay in view", () => {
    expect(key("ArrowRight")?.t1Ms).toBe(2_001);
    expect(key("ArrowLeft")?.t1Ms).toBe(1_999);
    expect(key("ArrowRight", r, true)?.t1Ms).toBe(2_300);
    // Clamped to the view, so the focused cursor never scrolls away.
    expect(key("ArrowLeft", { t1Ms: 1_100, t2Ms: 3_000 }, true)?.t1Ms).toBe(1_000);
    expect(key("ArrowLeft", { t1Ms: 1_000, t2Ms: 3_000 })?.t1Ms).toBe(1_000);
  });

  it("PageUp and PageDown take a big step", () => {
    expect(key("PageUp")?.t1Ms).toBe(2_300);
    expect(key("PageDown")?.t1Ms).toBe(1_700);
  });

  it("Home and End go to the ends of the extent", () => {
    expect(key("Home")?.t1Ms).toBe(0);
    expect(key("End")?.t1Ms).toBe(10_000);
  });

  it("an arrow towards the view brings an off-view cursor to its edge", () => {
    expect(key("ArrowRight", { t1Ms: 100, t2Ms: 3_000 })?.t1Ms).toBe(1_000);
    expect(key("ArrowLeft", { t1Ms: 9_000, t2Ms: 3_000 })?.t1Ms).toBe(4_000);
  });

  it("an arrow away from the view steps within the extent", () => {
    expect(key("ArrowLeft", { t1Ms: 100, t2Ms: 3_000 })?.t1Ms).toBe(99);
    expect(key("ArrowLeft", { t1Ms: 0, t2Ms: 3_000 })?.t1Ms).toBe(0);
  });

  it("moves only the focused cursor and ignores other keys", () => {
    expect(key("ArrowRight")?.t2Ms).toBe(3_000);
    expect(key("a")).toBeNull();
    expect(key("ArrowUp")).toBeNull();
  });
});

describe("announcing the readout (AC-AXHK2T.3)", () => {
  it("reads every value with its name", () => {
    const text = rulerAnnouncement(rulerReadout(rulerValues({ t1Ms: 0, t2Ms: 2_000 }, hour)));
    expect(text).toBe(
      "Ruler: Delta time 2.000 s, Delta charge 1.000000 Ah, Delta energy 5.000000 Wh, " +
        "Average voltage 5.000 V, Average current 1.000 A, Average power 5.000 W",
    );
  });
});
