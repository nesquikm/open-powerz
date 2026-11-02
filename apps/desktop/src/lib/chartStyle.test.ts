/**
 * FR AXHK2V: the chart's appearance settings, one pure module (`chartStyle.ts`) that
 * feeds the uPlot options.
 *
 * API pinned here, beyond the FR's Technical Design:
 *
 * - `VBUS_PRESETS`, `IBUS_PRESETS`: the axis range presets, `{ id, label }[]` in the
 *   order the UI offers them (a label may write the range with an en dash);
 * - `ChartStyle`: `{ vbusRange, ibusRange, title, … }` (a preset id per axis, the
 *   chart title) plus each series' look; `DEFAULT_CHART_STYLE` is Auto, Auto and no title;
 * - `SeriesLook`: `{ color, width, opacity }` (`#rrggbb`, CSS px, 0–1);
 *   `setSeriesLook(style, key, patch)` and `resetSeriesLook(style, key)` return a new
 *   style; `seriesLooks(theme, style)` gives `{ key, color, width, opacity, dash }` for
 *   every series in `CHART_SERIES` order (the theme's colour while not set);
 * - `lowContrastWarning(color, theme)`: a sentence that names the contrast, or `null`;
 * - `CHART_STYLE_KEY`, `loadChartStyle(storage)`, `saveChartStyle(style, storage)`: one
 *   app-wide key (not per meter), read back field by field;
 * - `chartOptions({ …, style })`: the volts scale follows the VBUS preset, the amps scale
 *   the IBUS preset, and each series is stroked in its look.
 */

import type uPlot from "uplot";
import { describe, expect, it } from "vitest";
import {
  CHART_STYLE_KEY,
  DEFAULT_CHART_STYLE,
  IBUS_PRESETS,
  VBUS_PRESETS,
  loadChartStyle,
  lowContrastWarning,
  resetSeriesLook,
  saveChartStyle,
  seriesLooks,
  setSeriesLook,
  type ChartStyle,
} from "./chartStyle";
import { CHART_SERIES, chartOptions, defaultVisibility, seriesStyles } from "./chartSeries";
import { THEME_COLORS, contrastRatio } from "./theme";

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

/** A preset label with its range dash written as a hyphen, as the AC writes it. */
const plain = (label: string): string => label.replace(/–/g, "-");

function presetId(presets: readonly { id: string; label: string }[], label: string): string {
  const found = presets.find((p) => plain(p.label) === label);
  if (found === undefined) throw new Error(`no preset ${label}`);
  return found.id;
}

type RangeFn = (u: uPlot, min: number, max: number) => [number, number];

function scaleRange(opts: uPlot.Options, scale: string): RangeFn {
  const range = opts.scales?.[scale]?.range;
  if (typeof range !== "function") throw new Error(`scale ${scale} has no range function`);
  return range as unknown as RangeFn;
}

function optionsWith(style: ChartStyle): uPlot.Options {
  return chartOptions({
    theme: "light",
    visibility: defaultVisibility(),
    width: 800,
    height: 400,
    pxRatio: 1,
    style,
  });
}

/** `[r, g, b, a]` of `#rrggbb`, `#rrggbbaa`, `rgb()` or `rgba()`. */
function rgba(color: string): [number, number, number, number] {
  const hex = /^#([0-9a-f]{6})([0-9a-f]{2})?$/i.exec(color);
  if (hex) {
    const n = Number.parseInt(hex[1] ?? "000000", 16);
    const a = hex[2] === undefined ? 1 : Number.parseInt(hex[2], 16) / 255;
    return [(n >> 16) & 0xff, (n >> 8) & 0xff, n & 0xff, a];
  }
  const fn = /^rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*(?:,\s*([\d.]+)\s*)?\)$/i.exec(color);
  if (fn) {
    return [Number(fn[1]), Number(fn[2]), Number(fn[3]), fn[4] === undefined ? 1 : Number(fn[4])];
  }
  throw new Error(`not a colour: ${color}`);
}

describe("axis range presets (AC-AXHK2V.1)", () => {
  it("AC-AXHK2V.1: VBUS offers Auto, Auto from zero, 0-6 V, 0-12 V and 0-24 V", () => {
    expect(VBUS_PRESETS.map((p) => plain(p.label))).toEqual([
      "Auto",
      "Auto from zero",
      "0-6 V",
      "0-12 V",
      "0-24 V",
    ]);
  });

  it("AC-AXHK2V.1: IBUS offers Auto, Auto from zero, 0-2 A, 0-5 A and 0-8 A", () => {
    expect(IBUS_PRESETS.map((p) => plain(p.label))).toEqual([
      "Auto",
      "Auto from zero",
      "0-2 A",
      "0-5 A",
      "0-8 A",
    ]);
  });

  it("AC-AXHK2V.1: both start on Auto", () => {
    expect(DEFAULT_CHART_STYLE.vbusRange).toBe(presetId(VBUS_PRESETS, "Auto"));
    expect(DEFAULT_CHART_STYLE.ibusRange).toBe(presetId(IBUS_PRESETS, "Auto"));
  });

  it.each([
    ["0-6 V", [0, 6]],
    ["0-12 V", [0, 12]],
    ["0-24 V", [0, 24]],
  ] as const)("AC-AXHK2V.1: VBUS %s fixes the volts axis whatever the data", (label, want) => {
    const opts = optionsWith({ ...DEFAULT_CHART_STYLE, vbusRange: presetId(VBUS_PRESETS, label) });
    expect(scaleRange(opts, "volts")(null as never, 4.9, 5.1)).toEqual(want);
    expect(scaleRange(opts, "volts")(null as never, 19.5, 20.5)).toEqual(want);
  });

  it.each([
    ["0-2 A", [0, 2]],
    ["0-5 A", [0, 5]],
    ["0-8 A", [0, 8]],
  ] as const)("AC-AXHK2V.1: IBUS %s fixes the amps axis whatever the data", (label, want) => {
    const opts = optionsWith({ ...DEFAULT_CHART_STYLE, ibusRange: presetId(IBUS_PRESETS, label) });
    expect(scaleRange(opts, "amps")(null as never, 0.9, 1.1)).toEqual(want);
  });

  it("AC-AXHK2V.1: Auto from zero takes the axis down to zero, Auto does not", () => {
    const zero = optionsWith({
      ...DEFAULT_CHART_STYLE,
      vbusRange: presetId(VBUS_PRESETS, "Auto from zero"),
      ibusRange: presetId(IBUS_PRESETS, "Auto from zero"),
    });
    const [vMin, vMax] = scaleRange(zero, "volts")(null as never, 4.9, 5.1);
    expect(vMin).toBe(0);
    expect(vMax).toBeGreaterThanOrEqual(5.1);
    const [aMin, aMax] = scaleRange(zero, "amps")(null as never, 0.9, 1.1);
    expect(aMin).toBe(0);
    expect(aMax).toBeGreaterThanOrEqual(1.1);

    const auto = optionsWith(DEFAULT_CHART_STYLE);
    const [autoMin] = scaleRange(auto, "volts")(null as never, 4.9, 5.1);
    expect(autoMin).toBeGreaterThan(4);
  });
});

describe("each series' colour, width and transparency (AC-AXHK2V.2)", () => {
  it("AC-AXHK2V.2: starts on the theme's colours, fully opaque", () => {
    for (const theme of ["light", "dark"] as const) {
      const looks = seriesLooks(theme, DEFAULT_CHART_STYLE);
      expect(looks.map((l) => l.key)).toEqual(CHART_SERIES.map((s) => s.key));
      expect(looks.map((l) => l.color)).toEqual(seriesStyles(theme).map((s) => s.stroke));
      for (const l of looks) {
        expect(l.opacity).toBe(1);
        expect(l.width).toBeGreaterThan(0);
      }
    }
  });

  it("AC-AXHK2V.2: a set colour, width and transparency apply to that series alone", () => {
    const style = setSeriesLook(DEFAULT_CHART_STYLE, "vbus", {
      color: "#ff0000",
      width: 3,
      opacity: 0.5,
    });
    const looks = seriesLooks("light", style);
    const vbus = looks.find((l) => l.key === "vbus");
    expect(vbus).toMatchObject({ color: "#ff0000", width: 3, opacity: 0.5 });
    const before = seriesLooks("light", DEFAULT_CHART_STYLE);
    expect(looks.filter((l) => l.key !== "vbus")).toEqual(before.filter((l) => l.key !== "vbus"));
    // The style it came from is left as it was.
    expect(seriesLooks("light", DEFAULT_CHART_STYLE)).toEqual(before);
  });

  it("AC-AXHK2V.2: the plot strokes each series in its look", () => {
    const style = setSeriesLook(DEFAULT_CHART_STYLE, "vbus", {
      color: "#ff0000",
      width: 3,
      opacity: 0.5,
    });
    const opts = optionsWith(style);
    const vbus = opts.series[1];
    expect(vbus?.label).toBe("VBUS");
    expect(vbus?.width).toBe(3);
    expect(typeof vbus?.stroke).toBe("string");
    const [r, g, b, a] = rgba(String(vbus?.stroke));
    expect([r, g, b]).toEqual([255, 0, 0]);
    expect(a).toBeCloseTo(0.5, 2);
    // Another series keeps its theme colour.
    expect(rgba(String(opts.series[2]?.stroke))).toEqual(
      rgba(THEME_COLORS.light.chart.current ?? ""),
    );
  });

  it("AC-AXHK2V.2: Reset gives a series back its theme look", () => {
    const changed = setSeriesLook(
      setSeriesLook(DEFAULT_CHART_STYLE, "vbus", { color: "#ff0000", width: 4, opacity: 0.3 }),
      "ibus",
      { color: "#00ff00" },
    );
    const reset = resetSeriesLook(changed, "vbus");
    const looks = seriesLooks("dark", reset);
    expect(looks.find((l) => l.key === "vbus")).toEqual(
      seriesLooks("dark", DEFAULT_CHART_STYLE).find((l) => l.key === "vbus"),
    );
    expect(looks.find((l) => l.key === "ibus")?.color).toBe("#00ff00");
  });

  it("AC-AXHK2V.2: dash patterns stay distinct whatever the colours and widths", () => {
    let style = DEFAULT_CHART_STYLE;
    for (const s of CHART_SERIES) {
      style = setSeriesLook(style, s.key, { color: "#333333", width: 2, opacity: 1 });
    }
    for (const theme of ["light", "dark"] as const) {
      const dashes = seriesLooks(theme, style).map((l) => JSON.stringify(l.dash));
      expect(new Set(dashes).size).toBe(CHART_SERIES.length);
    }
  });
});

describe("a low-contrast colour (AC-AXHK2V.3)", () => {
  it("AC-AXHK2V.3: the theme's own series colours raise no warning", () => {
    for (const theme of ["light", "dark"] as const) {
      for (const s of seriesStyles(theme)) expect(lowContrastWarning(s.stroke, theme)).toBeNull();
    }
  });

  it("AC-AXHK2V.3: a colour close to the background warns, naming the contrast", () => {
    expect(lowContrastWarning("#ffffff", "light")).toMatch(/contrast/i);
    expect(lowContrastWarning("#fafafa", "light")).toMatch(/contrast/i);
    expect(lowContrastWarning(THEME_COLORS.dark.bg, "dark")).toMatch(/contrast/i);
    expect(lowContrastWarning("#000000", "dark")).toMatch(/contrast/i);
  });

  it("AC-AXHK2V.3: the warning follows the 3:1 line of a chart line", () => {
    // #949494 is just above 3:1 on white, #a0a0a0 just below.
    expect(contrastRatio("#949494", THEME_COLORS.light.bg)).toBeGreaterThan(3);
    expect(contrastRatio("#a0a0a0", THEME_COLORS.light.bg)).toBeLessThan(3);
    expect(lowContrastWarning("#949494", "light")).toBeNull();
    expect(lowContrastWarning("#a0a0a0", "light")).toMatch(/contrast/i);
  });

  it("AC-AXHK2V.3: the colour is still allowed", () => {
    const style = setSeriesLook(DEFAULT_CHART_STYLE, "ibus", { color: "#ffffff" });
    expect(seriesLooks("light", style).find((l) => l.key === "ibus")?.color).toBe("#ffffff");
    const opts = optionsWith(style);
    expect(rgba(String(opts.series[2]?.stroke)).slice(0, 3)).toEqual([255, 255, 255]);
  });
});

describe("the chart title (AC-AXHK2V.4)", () => {
  it("AC-AXHK2V.4: starts empty and is kept as typed, markup and all", () => {
    expect(DEFAULT_CHART_STYLE.title).toBe("");
    const storage = memoryStorage();
    const title = '<b>Bold</b> & <script>alert("x")</script>';
    saveChartStyle({ ...DEFAULT_CHART_STYLE, title }, storage);
    expect(loadChartStyle(storage).title).toBe(title);
  });
});

describe("the settings persist per app (AC-AXHK2V.8)", () => {
  const changed: ChartStyle = setSeriesLook(
    {
      ...DEFAULT_CHART_STYLE,
      vbusRange: presetId(VBUS_PRESETS, "0-24 V"),
      ibusRange: presetId(IBUS_PRESETS, "0-5 A"),
      title: "Bench run",
    },
    "cc1",
    { color: "#123456", width: 2.5, opacity: 0.75 },
  );

  it("AC-AXHK2V.8: one app-wide key, read back as saved", () => {
    expect(CHART_STYLE_KEY).toBe("open-powerz.chart.style");
    const storage = memoryStorage();
    saveChartStyle(changed, storage);
    expect([...storage.data.keys()]).toEqual([CHART_STYLE_KEY]);
    const back = loadChartStyle(storage);
    expect(back.vbusRange).toBe(changed.vbusRange);
    expect(back.ibusRange).toBe(changed.ibusRange);
    expect(back.title).toBe("Bench run");
    expect(seriesLooks("light", back)).toEqual(seriesLooks("light", changed));
  });

  it("AC-AXHK2V.8: nothing saved, corrupt or unreadable storage gives the defaults", () => {
    const defaults = seriesLooks("light", DEFAULT_CHART_STYLE);
    for (const storage of [
      memoryStorage(),
      memoryStorage({ [CHART_STYLE_KEY]: "{not json" }),
      memoryStorage({ [CHART_STYLE_KEY]: "[1,2]" }),
      throwing,
      undefined,
    ]) {
      const s = loadChartStyle(storage);
      expect(s.vbusRange).toBe(DEFAULT_CHART_STYLE.vbusRange);
      expect(s.ibusRange).toBe(DEFAULT_CHART_STYLE.ibusRange);
      expect(s.title).toBe("");
      expect(seriesLooks("light", s)).toEqual(defaults);
    }
  });

  it("AC-AXHK2V.8: a bad field falls back to its default alone", () => {
    const storage = memoryStorage();
    saveChartStyle(changed, storage);
    const saved = JSON.parse(storage.data.get(CHART_STYLE_KEY) ?? "{}") as Record<string, unknown>;
    storage.data.set(
      CHART_STYLE_KEY,
      JSON.stringify({ ...saved, vbusRange: "0-1000 V", title: 42 }),
    );
    const back = loadChartStyle(storage);
    expect(back.vbusRange).toBe(DEFAULT_CHART_STYLE.vbusRange);
    expect(back.title).toBe("");
    expect(back.ibusRange).toBe(changed.ibusRange);
    expect(seriesLooks("light", back).find((l) => l.key === "cc1")?.color).toBe("#123456");
  });

  it("AC-AXHK2V.8: a storage failure on save is ignored", () => {
    expect(() => saveChartStyle(changed, throwing)).not.toThrow();
    expect(loadChartStyle(throwing).title).toBe("");
  });
});
