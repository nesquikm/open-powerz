/**
 * FR AXHK2T: segment marks, the E.D. series and the chart options' persistence, one pure
 * module (`chartExtras.ts`). Reverse current and "all off draws as before" in
 * `chartOptions` are checked here too.
 *
 * API pinned here, beyond the FR's Technical Design:
 *
 * - the marks and E.D. themselves come from the backend's raw samples (`run_progress`,
 *   see `runProgressFeed.ts`); `extrasNotes(options, totals)` says in plain words why
 *   nothing is drawn when a total is not above zero;
 * - `ED_DEFINITION`: the one sentence that defines E.D., shown by the UI and quoted by the
 *   docs (its text is pinned here and only here);
 * - `MARK_STEPS`, `DEFAULT_CHART_EXTRAS`, `CHART_EXTRAS_KEY`, `loadChartExtras`,
 *   `saveChartExtras`: the chart options and where they persist;
 * - `chartOptions({ …, reverseCurrent })`: the amps scale runs top to bottom
 *   (`dir: -1`) and nothing else changes.
 */

import { describe, expect, it } from "vitest";
import {
  CHART_EXTRAS_KEY,
  DEFAULT_CHART_EXTRAS,
  ED_DEFINITION,
  MARK_STEPS,
  extrasNotes,
  loadChartExtras,
  saveChartExtras,
  type ChartExtras,
} from "./chartExtras";
import { chartOptions, defaultVisibility } from "./chartSeries";

function memoryStorage(initial: Record<string, string> = {}) {
  const data = new Map(Object.entries(initial));
  return {
    data,
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => void data.set(key, value),
  };
}

const throwing = {
  getItem: (): string | null => {
    throw new Error("denied");
  },
  setItem: (): void => {
    throw new Error("denied");
  },
};

describe("segment marks (AC-AXHK2T.4)", () => {
  it("defaults to a 10% step, among the steps offered", () => {
    expect(DEFAULT_CHART_EXTRAS.markStep).toBe(10);
    expect(MARK_STEPS).toContain(10);
    for (const s of MARK_STEPS) expect(100 % s).toBe(0);
  });
});

describe("E.D., the energy share (AC-AXHK2T.5)", () => {
  it("is defined in our own words, as ours and not the vendor's number", () => {
    // The one place the definition's text is pinned: the UI shows ED_DEFINITION and the
    // docs test quotes it, so the two always agree.
    expect(ED_DEFINITION).toBe(
      "E.D. is our own definition, not the vendor's number: the energy measured from the " +
        "start of the session up to each moment, as a percentage of the whole session's energy.",
    );
  });
});

describe("why nothing is drawn (AC-AXHK2T.4, .5)", () => {
  const on = { marks: "charge" as const, ed: true };

  it("says so in plain words when the run's total is not above zero", () => {
    expect(extrasNotes({ ...on, ed: false }, { marks: false, energy: null })).toEqual([
      "No charge in this run yet, so no marks.",
    ]);
    expect(extrasNotes({ marks: "energy", ed: false }, { marks: false, energy: null })).toEqual([
      "No energy in this run yet, so no marks.",
    ]);
  });

  it("says so when the session's energy is not above zero", () => {
    expect(extrasNotes({ marks: "off", ed: true }, { marks: null, energy: false })).toEqual([
      "No energy in this session yet, so no E.D. line.",
    ]);
  });

  it("says nothing while the answer is unknown, positive, or the option is off", () => {
    expect(extrasNotes(on, { marks: null, energy: null })).toEqual([]);
    expect(extrasNotes(on, { marks: true, energy: true })).toEqual([]);
    expect(extrasNotes({ marks: "off", ed: false }, { marks: false, energy: false })).toEqual([]);
  });

  it("gives both notes when both are empty", () => {
    expect(extrasNotes(on, { marks: false, energy: false })).toHaveLength(2);
  });
});

describe("the chart options persist (AC-AXHK2T.8)", () => {
  it("are all off by default", () => {
    expect(DEFAULT_CHART_EXTRAS).toEqual({
      ruler: false,
      marks: "off",
      markStep: 10,
      ed: false,
      subChart: false,
      reverseCurrent: false,
    });
  });

  it("use their own open-powerz storage key", () => {
    expect(CHART_EXTRAS_KEY).toBe("open-powerz.chart.extras");
  });

  it("round-trip every option", () => {
    const s = memoryStorage();
    const on: ChartExtras = {
      ruler: true,
      marks: "energy",
      markStep: 25,
      ed: true,
      subChart: true,
      reverseCurrent: true,
    };
    saveChartExtras(on, s);
    expect(s.data.has(CHART_EXTRAS_KEY)).toBe(true);
    expect(loadChartExtras(s)).toEqual(on);
    saveChartExtras(DEFAULT_CHART_EXTRAS, s);
    expect(loadChartExtras(s)).toEqual(DEFAULT_CHART_EXTRAS);
  });

  it("keep each valid field and drop a bad one", () => {
    const s = memoryStorage({
      [CHART_EXTRAS_KEY]: JSON.stringify({
        ruler: true,
        marks: "volts",
        markStep: 7,
        ed: "yes",
        subChart: true,
      }),
    });
    expect(loadChartExtras(s)).toEqual({
      ...DEFAULT_CHART_EXTRAS,
      ruler: true,
      subChart: true,
    });
  });

  it("are the defaults for nothing saved, corrupt JSON, or unreadable storage", () => {
    expect(loadChartExtras(memoryStorage())).toEqual(DEFAULT_CHART_EXTRAS);
    expect(loadChartExtras(undefined)).toEqual(DEFAULT_CHART_EXTRAS);
    expect(loadChartExtras(memoryStorage({ [CHART_EXTRAS_KEY]: "{" }))).toEqual(
      DEFAULT_CHART_EXTRAS,
    );
    expect(loadChartExtras(memoryStorage({ [CHART_EXTRAS_KEY]: "[1]" }))).toEqual(
      DEFAULT_CHART_EXTRAS,
    );
    expect(loadChartExtras(throwing)).toEqual(DEFAULT_CHART_EXTRAS);
  });

  it("ignore a storage that refuses the write", () => {
    expect(() => saveChartExtras(DEFAULT_CHART_EXTRAS, throwing)).not.toThrow();
    expect(loadChartExtras(throwing)).toEqual(DEFAULT_CHART_EXTRAS);
  });
});

describe("reverse current (AC-AXHK2T.7) and all off (AC-AXHK2T.8) in the plot options", () => {
  const base = {
    theme: "light" as const,
    visibility: defaultVisibility(),
    width: 800,
    height: 300,
  };

  it("flips the amps axis only", () => {
    const opts = chartOptions({ ...base, reverseCurrent: true });
    expect(opts.scales?.amps?.dir).toBe(-1);
    expect(opts.scales?.volts?.dir ?? 1).toBe(1);
    expect(opts.scales?.watts?.dir ?? 1).toBe(1);
    expect(opts.scales?.x?.dir ?? 1).toBe(1);
  });

  it("keeps the axis range: values are drawn reversed, not changed", () => {
    const off = chartOptions(base);
    const on = chartOptions({ ...base, reverseCurrent: true });
    const range = (o: typeof off) =>
      o.scales?.amps?.range as (u: unknown, a: number, b: number) => [number, number];
    expect(range(on)(null, -0.2, 1.3)).toEqual(range(off)(null, -0.2, 1.3));
  });

  it("with every option off, the plot options are what they were", () => {
    const before = chartOptions(base);
    const off = chartOptions({ ...base, reverseCurrent: false });
    expect(off.series.map((s) => s.label)).toEqual(before.series.map((s) => s.label));
    expect(off.series.map((s) => s.scale)).toEqual(before.series.map((s) => s.scale));
    expect(off.axes?.length).toBe(before.axes?.length);
    expect(Object.keys(off.scales ?? {})).toEqual(Object.keys(before.scales ?? {}));
    expect(before.scales?.amps?.dir ?? 1).toBe(1);
    expect(off.scales?.amps?.dir ?? 1).toBe(1);
  });
});
