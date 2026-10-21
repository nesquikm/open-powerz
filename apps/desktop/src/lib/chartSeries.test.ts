import uPlot from "uplot";
import { describe, expect, it } from "vitest";
import { THEME_COLORS, type ResolvedTheme, type ThemeStorage } from "./theme";
import {
  CHART_AXES,
  CHART_SERIES,
  LIVE_WINDOW_KEY,
  MIN_AXIS_SPAN,
  SERIES_VISIBILITY_KEY,
  ZERO_BASED_KEY,
  chartOptions,
  defaultVisibility,
  envelopeRange,
  loadLiveWindow,
  loadVisibility,
  loadZeroBased,
  minSpanRange,
  saveLiveWindow,
  saveVisibility,
  saveZeroBased,
  seriesStroke,
  seriesStyles,
  toggleSeries,
  visibleAxes,
  zeroBasedRange,
  type ChartSeriesKey,
} from "./chartSeries";

const THEMES: readonly ResolvedTheme[] = ["light", "dark"];

function def(key: ChartSeriesKey) {
  const found = CHART_SERIES.find((s) => s.key === key);
  if (!found) throw new Error(`no series ${key}`);
  return found;
}

class FakeStorage implements ThemeStorage {
  map = new Map<string, string>();
  getItem(key: string): string | null {
    return this.map.get(key) ?? null;
  }
  setItem(key: string, value: string): void {
    this.map.set(key, value);
  }
}

describe("CHART_SERIES / CHART_AXES (AC-AH2AEM.1)", () => {
  it("puts VBUS, D+, D−, CC1 and CC2 on the volts axis", () => {
    const volts = CHART_SERIES.filter((s) => s.axis === "volts");
    expect(volts.map((s) => s.key).sort()).toEqual(["cc1", "cc2", "dm", "dp", "vbus"]);
    expect(volts.map((s) => s.label).sort()).toEqual(["CC1", "CC2", "D+", "D−", "VBUS"].sort());
    for (const s of volts) expect(s.unit).toBe("V");
  });

  it("puts IBUS alone on the amps axis and power alone on the watts axis", () => {
    expect(CHART_SERIES.filter((s) => s.axis === "amps").map((s) => s.key)).toEqual(["ibus"]);
    expect(def("ibus").unit).toBe("A");
    expect(def("ibus").label).toBe("IBUS");
    expect(CHART_SERIES.filter((s) => s.axis === "watts").map((s) => s.key)).toEqual(["power"]);
    expect(def("power").unit).toBe("W");
  });

  it("has the volts axis on the left and the amps axis on the right", () => {
    const side = (id: string) => CHART_AXES.find((a) => a.id === id)?.side;
    expect(side("volts")).toBe("left");
    expect(side("amps")).toBe("right");
    expect(CHART_AXES.map((a) => a.id).sort()).toEqual(["amps", "volts", "watts"]);
  });

  it("hides power (and its watts axis) by default and shows every other series", () => {
    const v = defaultVisibility();
    expect(v.power).toBe(false);
    for (const s of CHART_SERIES) {
      if (s.key !== "power") expect(v[s.key]).toBe(true);
    }
    expect(visibleAxes(v)).toEqual(expect.arrayContaining(["volts", "amps"]));
    expect(visibleAxes(v)).not.toContain("watts");
    expect(visibleAxes(toggleSeries(v, "power"))).toContain("watts");
  });

  it("builds uPlot options with volts on side 3 (left), amps on side 1 (right), watts hidden", () => {
    const opts = chartOptions({
      theme: "light",
      visibility: defaultVisibility(),
      width: 800,
      height: 300,
    });
    const axis = (scale: string) => opts.axes?.find((a) => a.scale === scale);
    expect(axis("volts")?.side).toBe(3);
    expect(axis("amps")?.side).toBe(1);
    expect(axis("watts")?.show).toBe(false);
    const shown = chartOptions({
      theme: "light",
      visibility: toggleSeries(defaultVisibility(), "power"),
      width: 800,
      height: 300,
    });
    expect(shown.axes?.find((a) => a.scale === "watts")?.show).not.toBe(false);
    expect(opts.width).toBe(800);
    expect(opts.height).toBe(300);
  });

  it("maps each uPlot series (after x) to its axis scale, in CHART_SERIES order", () => {
    const opts = chartOptions({
      theme: "dark",
      visibility: defaultVisibility(),
      width: 640,
      height: 240,
    });
    expect(opts.series).toHaveLength(CHART_SERIES.length + 1);
    CHART_SERIES.forEach((s, i) => {
      const u = opts.series[i + 1];
      expect(u?.scale).toBe(s.axis);
      expect(u?.label).toBe(s.label);
    });
  });
});

describe("series visibility (AC-AH2AEM.5)", () => {
  it("toggles exactly one series and leaves the others alone", () => {
    const v = defaultVisibility();
    const off = toggleSeries(v, "cc2");
    expect(off.cc2).toBe(false);
    for (const s of CHART_SERIES) {
      if (s.key !== "cc2") expect(off[s.key]).toBe(v[s.key]);
    }
    expect(toggleSeries(off, "cc2").cc2).toBe(true);
    expect(v.cc2).toBe(true); // the input is not mutated
  });

  it("applies the choice to the uPlot series' show flag", () => {
    const v = toggleSeries(toggleSeries(defaultVisibility(), "dp"), "power");
    const opts = chartOptions({ theme: "light", visibility: v, width: 500, height: 200 });
    CHART_SERIES.forEach((s, i) => {
      expect(opts.series[i + 1]?.show).toBe(v[s.key]);
    });
  });

  it("persists the choice for the session and reads it back", () => {
    const storage = new FakeStorage();
    const v = toggleSeries(toggleSeries(defaultVisibility(), "ibus"), "power");
    saveVisibility(v, storage);
    expect(storage.map.has(SERIES_VISIBILITY_KEY)).toBe(true);
    expect(loadVisibility(storage)).toEqual(v);
  });

  it("falls back to the defaults on missing, corrupt or unreadable storage", () => {
    expect(loadVisibility(undefined)).toEqual(defaultVisibility());
    expect(loadVisibility(new FakeStorage())).toEqual(defaultVisibility());
    const corrupt = new FakeStorage();
    corrupt.setItem(SERIES_VISIBILITY_KEY, "{not json");
    expect(loadVisibility(corrupt)).toEqual(defaultVisibility());
    const throwing: ThemeStorage = {
      getItem: () => {
        throw new Error("denied");
      },
      setItem: () => {
        throw new Error("denied");
      },
    };
    expect(loadVisibility(throwing)).toEqual(defaultVisibility());
    expect(() => saveVisibility(defaultVisibility(), throwing)).not.toThrow();
    expect(loadVisibility(throwing).vbus).toBe(true);
  });

  it("keeps defaults for keys a stored choice does not mention", () => {
    const storage = new FakeStorage();
    storage.setItem(SERIES_VISIBILITY_KEY, JSON.stringify({ vbus: false, bogus: true }));
    const v = loadVisibility(storage);
    expect(v.vbus).toBe(false);
    expect(v.ibus).toBe(true);
    expect(v.power).toBe(false);
    expect(Object.keys(v).sort()).toEqual(CHART_SERIES.map((s) => s.key).sort());
  });
});

describe("gaps are never bridged (AC-AH2AEM.7)", () => {
  it("never sets spanGaps on a series", () => {
    const opts = chartOptions({
      theme: "light",
      visibility: defaultVisibility(),
      width: 800,
      height: 300,
    });
    for (const s of opts.series.slice(1)) expect(s.spanGaps ?? false).toBe(false);
  });
});

describe("series styles (AC-AH2AEM.8)", () => {
  for (const theme of THEMES) {
    it(`takes every series colour from the ${theme} theme's chart tokens`, () => {
      const palette = THEME_COLORS[theme].chart;
      const styles = seriesStyles(theme);
      expect(styles.map((s) => s.key)).toEqual(CHART_SERIES.map((s) => s.key));
      for (const s of CHART_SERIES) {
        expect(Object.keys(palette)).toContain(s.token);
        expect(styles.find((x) => x.key === s.key)?.stroke).toBe(palette[s.token]);
      }
    });

    it(`gives every series a distinct line dash in the ${theme} theme`, () => {
      const dashes = seriesStyles(theme).map((s) => JSON.stringify(s.dash));
      expect(new Set(dashes).size).toBe(CHART_SERIES.length);
    });

    it(`passes the ${theme} colours and dashes into the uPlot series and axes`, () => {
      const opts = chartOptions({
        theme,
        visibility: defaultVisibility(),
        width: 800,
        height: 300,
      });
      const styles = seriesStyles(theme);
      CHART_SERIES.forEach((s, i) => {
        expect(opts.series[i + 1]?.stroke).toBe(styles[i]?.stroke);
      });
      const tokens = Object.values(THEME_COLORS[theme]).filter((v) => typeof v === "string");
      for (const a of opts.axes ?? []) expect(tokens).toContain(a.stroke);
    });
  }

  it("uses the same dash for a series in both themes but re-colours it", () => {
    const light = seriesStyles("light");
    const dark = seriesStyles("dark");
    light.forEach((s, i) => {
      expect(dark[i]?.dash).toEqual(s.dash);
      expect(dark[i]?.stroke).not.toBe(s.stroke);
    });
  });
});

describe("axis titles (AC-AH2AEM.1)", () => {
  it("names each value axis in words with its unit, so it reads when rotated", () => {
    const opts = chartOptions({
      theme: "light",
      visibility: defaultVisibility(),
      width: 800,
      height: 400,
    });
    const label = (scale: string) => opts.axes?.find((a) => a.scale === scale)?.label;
    expect(label("volts")).toBe("Volts (V)");
    expect(label("amps")).toBe("Amps (A)");
    expect(label("watts")).toBe("Watts (W)");
  });
});

describe("time axis ticks (visual check)", () => {
  it("shows as many decimals as the tick spacing needs, so labels never repeat", async () => {
    const { tickDecimals } = await import("./chartSeries");
    expect(tickDecimals(5_000)).toBe(0);
    expect(tickDecimals(1_000)).toBe(0);
    expect(tickDecimals(500)).toBe(1);
    expect(tickDecimals(50)).toBe(2);
    expect(tickDecimals(25)).toBe(3);
    expect(tickDecimals(1)).toBe(3);
    expect(tickDecimals(0)).toBe(0);
  });
});

describe("zero-based axes (AC-AH2AEM.9)", () => {
  it("stretches a positive-only range down to zero", () => {
    const [min, max] = zeroBasedRange(1.94, 2.06);
    expect(min).toBe(0);
    expect(max).toBeGreaterThanOrEqual(2.06);
  });

  it("stretches a negative-only range up to zero, so negative currents stay visible", () => {
    const [min, max] = zeroBasedRange(-1.2, -0.8);
    expect(min).toBeLessThanOrEqual(-1.2);
    expect(max).toBe(0);
  });

  it("keeps both signs of a range that crosses zero, with padding", () => {
    const [min, max] = zeroBasedRange(-0.5, 2);
    expect(min).toBeLessThan(-0.5);
    expect(max).toBeGreaterThan(2);
  });

  it("gives a finite range that includes zero when there is no data", () => {
    const [min, max] = zeroBasedRange(null, null);
    expect(Number.isFinite(min) && Number.isFinite(max)).toBe(true);
    expect(min).toBeLessThanOrEqual(0);
    expect(max).toBeGreaterThan(min);
  });

  it("gives every value axis the zero-based range when on, and leaves auto-scale when off", () => {
    const base = {
      theme: "light",
      visibility: defaultVisibility(),
      width: 500,
      height: 200,
    } as const;
    const off = chartOptions({ ...base, zeroBased: false });
    const on = chartOptions({ ...base, zeroBased: true });
    for (const a of CHART_AXES) {
      // Off: auto-scale with the axis's minimum span (AC-CWWKJ6.2).
      const offRange = off.scales?.[a.id]?.range;
      expect(typeof offRange).toBe("function");
      const offFn = offRange as (
        u: unknown,
        min: number,
        max: number,
        key: string,
      ) => [number, number];
      expect(offFn(null, 0, 20, a.id)).toEqual(uPlot.rangeNum(0, 20, 0.1, true));
      const range = on.scales?.[a.id]?.range;
      expect(typeof range).toBe("function");
      const fn = range as (u: unknown, min: number, max: number, key: string) => [number, number];
      expect(fn(null, 1.94, 2.06, a.id)).toEqual(zeroBasedRange(1.94, 2.06));
    }
    expect(on.scales?.x?.range).toBeUndefined();
  });

  it("is off by default and persists the choice for the session", () => {
    expect(loadZeroBased(undefined)).toBe(false);
    const storage = new FakeStorage();
    expect(loadZeroBased(storage)).toBe(false);
    saveZeroBased(true, storage);
    expect(storage.map.get(ZERO_BASED_KEY)).toBeDefined();
    expect(loadZeroBased(storage)).toBe(true);
    saveZeroBased(false, storage);
    expect(loadZeroBased(storage)).toBe(false);
    storage.setItem(ZERO_BASED_KEY, "{not json");
    expect(loadZeroBased(storage)).toBe(false);
    const throwing: ThemeStorage = {
      getItem: () => {
        throw new Error("denied");
      },
      setItem: () => {
        throw new Error("denied");
      },
    };
    expect(loadZeroBased(throwing)).toBe(false);
    expect(() => saveZeroBased(true, throwing)).not.toThrow();
  });
});

describe("value axis ticks (true minus sign)", () => {
  it("writes every tick with the decimals its spacing needs and U+2212 for negatives", async () => {
    const { axisTicks } = await import("./chartSeries");
    expect(axisTicks([-0.2, -0.1, 0, 0.1], 0.1)).toEqual(["−0.2", "−0.1", "0.0", "0.1"]);
    expect(axisTicks([0, 0.5, 1], 0.5)).toEqual(["0.0", "0.5", "1.0"]);
    expect(axisTicks([1.94, 1.96], 0.02)).toEqual(["1.94", "1.96"]);
    expect(axisTicks([-5, 0, 5], 5)).toEqual(["−5", "0", "5"]);
  });
});

describe("time since the session start (AC-272WSN.1, AC-272WSN.2)", () => {
  it("labels ticks in seconds, then m:ss, then h:mm:ss as the range grows (AC-272WSN.1)", async () => {
    const { timeTicks } = await import("./chartSeries");
    expect(timeTicks([0, 5_000, 10_000], 5_000)).toEqual(["0 s", "5 s", "10 s"]);
    expect(timeTicks([0, 500, 1_000], 500)).toEqual(["0.0 s", "0.5 s", "1.0 s"]);
    expect(timeTicks([0, 30_000, 60_000, 90_000], 30_000)).toEqual([
      "0:00",
      "0:30",
      "1:00",
      "1:30",
    ]);
    expect(timeTicks([60_000, 60_500], 500)).toEqual(["1:00.0", "1:00.5"]);
    expect(timeTicks([0, 1_800_000, 3_600_000], 1_800_000)).toEqual([
      "0:00:00",
      "0:30:00",
      "1:00:00",
    ]);
    expect(timeTicks([-5_000, 0], 5_000)).toEqual(["−5 s", "0 s"]);
  });

  it("writes one time in the unit its size needs, as the hover readout shows it (AC-272WSN.2)", async () => {
    const { formatSessionTime } = await import("./chartSeries");
    expect(formatSessionTime(12_345, 3)).toBe("12.345 s");
    expect(formatSessionTime(62_345, 3)).toBe("1:02.345");
    expect(formatSessionTime(3_723_456, 3)).toBe("1:02:03.456");
    expect(formatSessionTime(-1_500, 1)).toBe("−1.5 s");
    expect(formatSessionTime(59_999.6, 0)).toBe("1:00");
  });

  it('writes a gap\'s length in whole seconds, for its "No data" label (AC-272WSN.5)', async () => {
    const { formatDuration } = await import("./chartSeries");
    expect(formatDuration(45_200)).toBe("45 s");
    expect(formatDuration(2_400_000)).toBe("40:00");
    expect(formatDuration(3_723_000)).toBe("1:02:03");
  });

  it("the x axis counts from the origin the chart gives it, not the meter's clock (AC-272WSN.1)", () => {
    const opts = chartOptions({
      theme: "dark",
      visibility: defaultVisibility(),
      width: 800,
      height: 400,
      originMs: () => 30_400_000,
    });
    const values = opts.axes?.[0]?.values as (
      u: unknown,
      splits: number[],
      axis: number,
      space: number,
      incr: number,
    ) => string[];
    expect(values(null, [30_400_000, 30_460_000, 30_520_000], 0, 0, 60_000)).toEqual([
      "0:00",
      "1:00",
      "2:00",
    ]);
  });
});

describe("axis width (AC-272WSN.6)", () => {
  /** A uPlot stand-in whose font is 7 px per character at `dpr`. */
  function fakePlot(dpr: number) {
    return {
      axes: [{ ticks: { size: 10 }, gap: 5, font: ["12px sans-serif"], _size: 50 }],
      ctx: { font: "", measureText: (s: string) => ({ width: s.length * 7 * dpr }) },
    };
  }

  it("is as wide as the widest tick label plus the ticks and their gap", async () => {
    const { axisSize } = await import("./chartSeries");
    const u = fakePlot(1);
    // "−0.0015" is 7 characters: 49 px, plus 10 px of ticks and a 5 px gap.
    expect(axisSize(u as never, ["0.0010", "−0.0015"], 0, 0, 1)).toBe(64);
    // In CSS pixels, whatever the display's pixel ratio.
    expect(axisSize(fakePlot(2) as never, ["0.0010", "−0.0015"], 0, 0, 2)).toBe(64);
    // Later sizing passes keep the size of the first.
    expect(axisSize(u as never, ["1"], 0, 2, 1)).toBe(50);
    expect(axisSize(u as never, null, 0, 0, 1)).toBe(15);
  });

  it("sizes every value axis to its labels, so a title never covers them", async () => {
    const { axisSize } = await import("./chartSeries");
    const opts = chartOptions({
      theme: "light",
      visibility: defaultVisibility(),
      width: 800,
      height: 400,
    });
    for (const axis of (opts.axes ?? []).slice(1)) {
      expect(typeof axis.size).toBe("function");
      // The title keeps clear of the labels it sits beside.
      expect(axis.labelGap).toBeGreaterThanOrEqual(4);
    }
    expect(axisSize).toBeTypeOf("function");
  });
});

describe("time ticks sit on round session times (AC-272WSN.1)", () => {
  it("places ticks every `incr` counted from the origin, inside the scale", async () => {
    const { timeSplits } = await import("./chartSeries");
    // Origin 30 400 123 ms; a 0:00–2:30 view; one-minute steps.
    const o = 30_400_123;
    expect(timeSplits(o, o + 150_000, 60_000, o)).toEqual([o, o + 60_000, o + 120_000]);
    expect(timeSplits(o + 1, o + 150_000, 60_000, o)).toEqual([o + 60_000, o + 120_000]);
  });

  it("steps in seconds and minutes, not in plain powers of ten", () => {
    const opts = chartOptions({
      theme: "dark",
      visibility: defaultVisibility(),
      width: 800,
      height: 400,
    });
    const incrs = opts.axes?.[0]?.incrs as number[];
    for (const step of [1_000, 15_000, 30_000, 60_000, 300_000, 600_000, 1_800_000, 3_600_000]) {
      expect(incrs).toContain(step);
    }
    expect(incrs).not.toContain(50_000);
    expect(typeof opts.axes?.[0]?.splits).toBe("function");
  });
});

describe("time formatting edge cases (AC-272WSN.1, AC-272WSN.2)", () => {
  it("writes a missing or non-finite time as a dash", async () => {
    const { formatSessionTime, timeTicks } = await import("./chartSeries");
    expect(formatSessionTime(Number.NaN, 3)).toBe("—");
    expect(formatSessionTime(Number.POSITIVE_INFINITY, 0)).toBe("—");
    expect(timeTicks([Number.NaN, 0], 1_000)).toEqual(["—", "0 s"]);
  });

  it("never loops past a sane number of ticks, whatever the range", async () => {
    const { timeSplits } = await import("./chartSeries");
    expect(timeSplits(0, Number.POSITIVE_INFINITY, 1_000, 0)).toEqual([]);
    expect(timeSplits(0, 1e12, 1, 0).length).toBeLessThanOrEqual(1_000);
  });

  it("measures labels without leaving its font on the shared canvas", async () => {
    const { axisSize } = await import("./chartSeries");
    const ctx = { font: "10px original", measureText: (s: string) => ({ width: s.length * 7 }) };
    const u = { axes: [{ ticks: { size: 10 }, gap: 5, font: ["12px axis"] }], ctx };
    axisSize(u as never, ["1.0"], 0, 0, 1);
    expect(ctx.font).toBe("10px original");
  });
});

describe("the zoom readout's span (AC-FENCD9.9)", () => {
  it("writes the visible span like the time axis: seconds, then m:ss, then h:mm:ss", async () => {
    const { formatSpan } = await import("./chartSeries");
    expect(formatSpan(10_000)).toBe("10 s");
    expect(formatSpan(150_000)).toBe("2:30");
    expect(formatSpan(2_423_000)).toBe("40:23");
    expect(formatSpan(3_723_000)).toBe("1:02:03");
    // Short spans keep the decimals they need.
    expect(formatSpan(2_500)).toBe("2.5 s");
    expect(formatSpan(250)).toBe("0.25 s");
    expect(formatSpan(10)).toBe("0.01 s");
    expect(formatSpan(Number.NaN)).toBe("—");
  });
});

describe("series stroke per pixel ratio (AC-AKNVND.1)", () => {
  const DASHED = CHART_SERIES.filter((s) => s.key !== "vbus" && s.key !== "power");

  it("draws the solid VBUS line at least 1.5 CSS px wide at DPR 1", () => {
    const s = seriesStroke("vbus", 1);
    expect(s.dash).toEqual([]);
    expect(s.width).toBeGreaterThanOrEqual(1.5);
  });

  it("gives every dashed series a whole-pixel dash pattern at DPR 1 and 2", () => {
    for (const dpr of [1, 2]) {
      for (const def of DASHED) {
        const s = seriesStroke(def.key, dpr);
        expect(s.dash.length, `${def.key} is dashed`).toBeGreaterThan(0);
        for (const d of s.dash) {
          expect(d, `${def.key} dash entry`).toBeGreaterThan(0);
          expect(Number.isInteger(d * dpr), `${def.key} dash ${d} at DPR ${dpr}`).toBe(true);
        }
        expect(s.width, `${def.key} width`).toBeGreaterThanOrEqual(1);
      }
    }
  });

  it("keeps each dashed series' pattern distinct at DPR 1", () => {
    const patterns = DASHED.map((d) => seriesStroke(d.key, 1).dash.join(","));
    expect(new Set(patterns).size).toBe(patterns.length);
  });

  it("chartOptions passes the DPR-1 widths and dashes to uPlot", () => {
    const opts = chartOptions({
      theme: "light",
      visibility: defaultVisibility(),
      width: 800,
      height: 400,
      pxRatio: 1,
    } as Parameters<typeof chartOptions>[0] & { pxRatio: number });
    CHART_SERIES.forEach((def, i) => {
      const series = opts.series[i + 1];
      const want = seriesStroke(def.key, 1);
      expect(series?.width, `${def.key} width`).toBe(want.width);
    });
    expect(opts.series[1]?.width).toBeGreaterThanOrEqual(1.5);
  });
});

describe("dashes without a canvas dash (AC-4NSVC9.3)", () => {
  const DASHED = CHART_SERIES.filter((s) => seriesStroke(s.key, 1).dash.length > 0);

  it("has dashed series to draw", () => {
    expect(DASHED.map((s) => s.key)).toEqual(["ibus", "power", "dp", "dm", "cc1", "cc2"]);
  });

  for (const pxRatio of [1, 2]) {
    it(`never hands uPlot a dash setting at DPR ${pxRatio}`, () => {
      const opts = chartOptions({
        theme: "light",
        visibility: defaultVisibility(),
        width: 800,
        height: 400,
        pxRatio,
      });
      CHART_SERIES.forEach((def, i) => {
        const dash = opts.series[i + 1]?.dash;
        expect(dash === undefined || dash.length === 0, `${def.key} canvas dash ${dash}`).toBe(
          true,
        );
      });
    });

    it(`gives every dashed series its own paths builder at DPR ${pxRatio}`, () => {
      const opts = chartOptions({
        theme: "dark",
        visibility: defaultVisibility(),
        width: 800,
        height: 400,
        pxRatio,
      });
      CHART_SERIES.forEach((def, i) => {
        if (!DASHED.includes(def)) return;
        expect(typeof opts.series[i + 1]?.paths, `${def.key} paths builder`).toBe("function");
      });
    });
  }

  it("keeps the legend's dash patterns, distinct per dashed series", () => {
    for (const theme of THEMES) {
      const styles = seriesStyles(theme).filter((s) => DASHED.some((d) => d.key === s.key));
      expect(styles).toHaveLength(DASHED.length);
      for (const s of styles) expect(s.dash.length, `${s.key} legend dash`).toBeGreaterThan(0);
      expect(new Set(styles.map((s) => s.dash.join(","))).size).toBe(DASHED.length);
    }
  });
});

describe("live window preference (AC-AKNVND.5)", () => {
  it("defaults to 10 s with nothing stored", () => {
    expect(loadLiveWindow(new FakeStorage())).toBe("10s");
    expect(loadLiveWindow(undefined)).toBe("10s");
  });

  it("round-trips every window choice", () => {
    for (const id of ["10s", "1m", "10m", "all"] as const) {
      const storage = new FakeStorage();
      saveLiveWindow(id, storage);
      expect(storage.map.get(LIVE_WINDOW_KEY)).toBeDefined();
      expect(loadLiveWindow(storage)).toBe(id);
    }
  });

  it("falls back to 10 s on a corrupt or unknown value", () => {
    for (const raw of ["not json", '"5m"', "42", "null", "{}"]) {
      const storage = new FakeStorage();
      storage.map.set(LIVE_WINDOW_KEY, raw);
      expect(loadLiveWindow(storage), raw).toBe("10s");
    }
  });

  it("ignores storage that throws", () => {
    const broken: ThemeStorage = {
      getItem: () => {
        throw new Error("denied");
      },
      setItem: () => {
        throw new Error("denied");
      },
    };
    expect(() => saveLiveWindow("1m", broken)).not.toThrow();
    expect(loadLiveWindow(broken)).toBe("10s");
  });

  it("is stored apart from the legend choice", () => {
    expect(LIVE_WINDOW_KEY).not.toBe(SERIES_VISIBILITY_KEY);
    expect(LIVE_WINDOW_KEY).toMatch(/^open-powerz\.chart\./);
  });
});

type RangeFn = (
  u: unknown,
  min: number | null,
  max: number | null,
  key: string,
) => [number, number];

function span([lo, hi]: [number, number]): number {
  return hi - lo;
}
function mid([lo, hi]: [number, number]): number {
  return (lo + hi) / 2;
}

describe("minimum axis span (AC-CWWKJ6.2)", () => {
  it("is 0.1 V, 0.02 A and 0.1 W", () => {
    expect(MIN_AXIS_SPAN).toEqual({ volts: 0.1, amps: 0.02, watts: 0.1 });
  });

  it("widens a flat or noisy signal to the minimum span, centred on the data", () => {
    const amps = minSpanRange(0.0005, 0.0015, 0.02);
    expect(span(amps)).toBeGreaterThanOrEqual(0.02 - 1e-12);
    expect(mid(amps)).toBeCloseTo(0.001, 9);
    const volts = minSpanRange(5, 5, 0.1);
    expect(span(volts)).toBeGreaterThanOrEqual(0.1 - 1e-12);
    expect(mid(volts)).toBeCloseTo(5, 9);
  });

  it("keeps the noise small on screen: a 1 mA ripple fills at most a tenth of the amps axis", () => {
    const r = minSpanRange(0.0005, 0.0015, MIN_AXIS_SPAN.amps);
    expect(0.001 / span(r)).toBeLessThanOrEqual(0.1);
  });

  it("autoscales larger swings as before", () => {
    expect(minSpanRange(0, 3, 0.02)).toEqual(uPlot.rangeNum(0, 3, 0.1, true));
    expect(minSpanRange(4.8, 20.2, 0.1)).toEqual(uPlot.rangeNum(4.8, 20.2, 0.1, true));
  });

  it("is each value axis's range when zero-based axes are off", () => {
    const opts = chartOptions({
      theme: "light",
      visibility: defaultVisibility(),
      width: 500,
      height: 200,
      zeroBased: false,
    });
    const fn = (id: string): RangeFn => opts.scales?.[id]?.range as unknown as RangeFn;
    // An idle output: ~1 mA of noise on the amps axis, a flat 5 V, ~5 mW.
    const amps = fn("amps")(null, 0.0005, 0.0015, "amps");
    expect(span(amps)).toBeGreaterThanOrEqual(0.02 - 1e-12);
    expect(mid(amps)).toBeCloseTo(0.001, 9);
    const volts = fn("volts")(null, 5.001, 5.003, "volts");
    expect(span(volts)).toBeGreaterThanOrEqual(0.1 - 1e-12);
    expect(mid(volts)).toBeCloseTo(5.002, 9);
    const watts = fn("watts")(null, 0.004, 0.006, "watts");
    expect(span(watts)).toBeGreaterThanOrEqual(0.1 - 1e-12);
    expect(mid(watts)).toBeCloseTo(0.005, 9);
    // A real load swings wider and autoscales as before.
    expect(fn("amps")(null, 0, 3, "amps")).toEqual(uPlot.rangeNum(0, 3, 0.1, true));
  });

  it("leaves the zero-based range alone when zero-based axes are on", () => {
    const opts = chartOptions({
      theme: "light",
      visibility: defaultVisibility(),
      width: 500,
      height: 200,
      zeroBased: true,
    });
    const amps = (opts.scales?.amps?.range as unknown as RangeFn)(null, 0.0005, 0.0015, "amps");
    expect(amps).toEqual(zeroBasedRange(0.0005, 0.0015));
  });
});

describe("overview envelope range (AC-CWWKJ6.3)", () => {
  it("gives IBUS at least the amps axis's minimum span, centred, so idle noise reads flat", () => {
    const r = envelopeRange("ibus", 0.0005, 0.0015);
    expect(span(r)).toBeGreaterThanOrEqual(MIN_AXIS_SPAN.amps - 1e-12);
    expect(mid(r)).toBeCloseTo(0.001, 9);
  });

  it("gives VBUS at least the volts axis's minimum span, centred", () => {
    const r = envelopeRange("vbus", 4.999, 5.001);
    expect(span(r)).toBeGreaterThanOrEqual(MIN_AXIS_SPAN.volts - 1e-12);
    expect(mid(r)).toBeCloseTo(5, 9);
  });

  it("keeps the data's own extent when it already spans more", () => {
    expect(envelopeRange("vbus", 0, 20)).toEqual([0, 20]);
    expect(envelopeRange("ibus", -1, 3)).toEqual([-1, 3]);
  });
});

describe("minSpanRange with no data (AC-CWWKJ6.2)", () => {
  it("gives an empty axis a centred minimum span instead of null bounds", () => {
    expect(minSpanRange(null, null, 0.02)).toEqual([-0.01, 0.01]);
    expect(minSpanRange(Number.NaN, 1, 0.1)).toEqual([-0.05, 0.05]);
  });
});
