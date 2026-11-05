/**
 * FR AXHK2V: what "Save image" asks for, one pure module (`chartExport.ts`).
 *
 * API pinned here, beyond the FR's Technical Design:
 *
 * - `EXPORT_FORMATS`: `["png", "svg", "pdf"]`, in the order the dialog offers them;
 * - `EXPORT_MIN_PX`, `EXPORT_MAX_PX`: 320 and 7680; `exportSizeError(width, height)` is a
 *   sentence naming both limits for a size outside them (or not a whole number), else
 *   `null`;
 * - `chartImageName(format, now)`: the default file name, `powerz-chart-YYYYMMDD-HHMMSS`
 *   in local time plus the format's extension, with no serial or UID in it.
 */

import { describe, expect, it } from "vitest";
import {
  EXPORT_FORMATS,
  EXPORT_MAX_PX,
  EXPORT_MIN_PX,
  EXPORT_LEGEND_PX,
  EXPORT_TITLE_PX,
  chartImageName,
  exportAxisRanges,
  exportLayout,
  exportSizeError,
  exportTexts,
} from "./chartExport";
import { CHART_SERIES, defaultVisibility } from "./chartSeries";
import { DEFAULT_CHART_STYLE, seriesLooks, setSeriesLook } from "./chartStyle";

describe("Save image's size and format (AC-AXHK2V.5)", () => {
  it("AC-AXHK2V.5: offers PNG, SVG and PDF", () => {
    expect([...EXPORT_FORMATS]).toEqual(["png", "svg", "pdf"]);
  });

  it("AC-AXHK2V.5: takes any whole size from 320 to 7680 px", () => {
    expect(EXPORT_MIN_PX).toBe(320);
    expect(EXPORT_MAX_PX).toBe(7680);
    for (const [w, h] of [
      [320, 320],
      [7680, 7680],
      [1920, 1080],
      [320, 7680],
    ] as const) {
      expect(exportSizeError(w, h)).toBeNull();
    }
  });

  it.each([
    [319, 600],
    [800, 319],
    [7681, 600],
    [800, 7681],
    [0, 0],
    [-800, 600],
    [800.5, 600],
    [Number.NaN, 600],
    [800, Number.POSITIVE_INFINITY],
  ])("AC-AXHK2V.5: refuses %s × %s px, naming the limits", (w, h) => {
    const error = exportSizeError(w, h);
    expect(error).not.toBeNull();
    expect(error).toContain("320");
    expect(error).toContain("7680");
  });
});

describe("default file names (AC-AXHK2V.10)", () => {
  const now = new Date(2026, 10, 2, 9, 5, 7);

  it.each(EXPORT_FORMATS.map((f) => [f]))(
    "AC-AXHK2V.10: a %s is named by date and time, never by meter",
    (format) => {
      expect(chartImageName(format, now)).toBe(`powerz-chart-20261102-090507.${format}`);
    },
  );
});

describe("the saved image's title band and legend row (AC-AXHK2V.6)", () => {
  const layout = (title: string, visibility = defaultVisibility()) =>
    exportLayout({
      width: 800,
      height: 600,
      title,
      theme: "light",
      style: setSeriesLook(DEFAULT_CHART_STYLE, "vbus", { color: "#ff0000", width: 3 }),
      visibility,
    });

  it("AC-AXHK2V.6: one legend entry per shown series, labelled with its unit", () => {
    const shown = CHART_SERIES.filter((s) => defaultVisibility()[s.key]);
    const l = layout("Bench run");
    expect(l.legend.map((i) => i.label)).toEqual(shown.map((s) => `${s.label} (${s.unit})`));
    expect(l.legend[0]?.label).toBe("VBUS (V)");
    const power = layout("", { ...defaultVisibility(), power: true });
    expect(power.legend.map((i) => i.label)).toContain("Power (W)");
  });

  it("AC-AXHK2V.6: each swatch takes its series' colour, width, opacity and dash", () => {
    const l = layout("Bench run");
    const looks = seriesLooks(
      "light",
      setSeriesLook(DEFAULT_CHART_STYLE, "vbus", { color: "#ff0000", width: 3 }),
    );
    for (const item of l.legend) {
      expect(item.look).toEqual(looks.find((x) => x.key === item.look.key));
    }
    expect(l.legend[0]?.look.color).toBe("#ff0000");
    expect(l.legend[0]?.look.width).toBe(3);
    expect(l.legend.find((i) => i.look.key === "ibus")?.look.dash.length).toBeGreaterThan(0);
  });

  it("AC-AXHK2V.6: the legend sits in its row at the foot, entries apart and inside the image", () => {
    const l = layout("Bench run");
    expect(l.bottom).toBe(EXPORT_LEGEND_PX);
    let right = 0;
    for (const item of l.legend) {
      expect(item.y).toBeGreaterThan(600 - EXPORT_LEGEND_PX);
      expect(item.y).toBeLessThan(600);
      expect(item.x0).toBeGreaterThanOrEqual(right);
      expect(item.x1).toBeGreaterThan(item.x0);
      expect(item.textX).toBeGreaterThan(item.x1);
      expect(item.textX).toBeLessThan(800);
      right = item.textX;
    }
  });

  it("AC-AXHK2V.6: a title takes the band at the top; no title, no band", () => {
    const titled = layout("Bench run");
    expect(titled.top).toBe(EXPORT_TITLE_PX);
    expect(titled.title?.text).toBe("Bench run");
    expect(titled.title?.y).toBeLessThan(EXPORT_TITLE_PX);
    expect(exportTexts(titled)[0]).toBe("Bench run");
    const bare = layout("");
    expect(bare.top).toBe(0);
    expect(bare.title).toBeNull();
    expect(exportTexts(bare)).toEqual(bare.legend.map((i) => i.label));
  });

  it("AC-AXHK2V.6: zero-based axes and fixed presets set the ranges both images share", () => {
    const x = [0, 1000, 2000];
    const data = [x, [5, 5.01, 5], [1, 1, 1], [5, 5, 5], ...[0, 0, 0, 0].map(() => [4.9, 5, 5])];
    const visibility = defaultVisibility();
    const view = { t0Ms: 0, t1Ms: 2000 };
    const auto = exportAxisRanges({ style: DEFAULT_CHART_STYLE, visibility, view, data });
    expect(auto.volts[0]).toBeGreaterThan(0);
    const zero = exportAxisRanges({
      style: DEFAULT_CHART_STYLE,
      visibility,
      view,
      data,
      zeroBased: true,
    });
    expect(zero.volts[0]).toBe(0);
    expect(zero.amps[0]).toBe(0);
    const fixed = exportAxisRanges({
      style: { ...DEFAULT_CHART_STYLE, vbusRange: "0-24" },
      visibility,
      view,
      data,
    });
    expect(fixed.volts).toEqual([0, 24]);
  });
});

describe("the PNG's drawn texts carry no serial (AC-AXHK2V.10)", () => {
  it("AC-AXHK2V.10: the title and legend labels never hold the meter serial, hidden or not", () => {
    // The demo meter's serial. The layout takes no meter, serial or "Hide serial numbers"
    // state at all, so with that switch on or off the PNG draws these same texts.
    const serial = "123456";
    for (const theme of ["light", "dark"] as const) {
      for (const title of [DEFAULT_CHART_STYLE.title, "Bench run"]) {
        const texts = exportTexts(
          exportLayout({
            width: 800,
            height: 600,
            title,
            theme,
            style: { ...DEFAULT_CHART_STYLE, title },
            visibility: { ...defaultVisibility(), power: true },
          }),
        );
        expect(texts.length).toBeGreaterThan(0);
        for (const t of texts) expect(t, t).not.toContain(serial);
      }
    }
  });
});
