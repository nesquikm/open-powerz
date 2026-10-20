import { describe, expect, it } from "vitest";
import { THEME_COLORS, type ResolvedTheme, type ThemeStorage } from "./theme";
import {
  CHART_AXES,
  CHART_SERIES,
  SERIES_VISIBILITY_KEY,
  chartOptions,
  defaultVisibility,
  loadVisibility,
  saveVisibility,
  seriesStyles,
  toggleSeries,
  visibleAxes,
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
        expect(opts.series[i + 1]?.dash).toEqual(styles[i]?.dash);
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
