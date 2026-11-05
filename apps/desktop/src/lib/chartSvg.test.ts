/**
 * FR AXHK2V: the SVG export, rendered from the chart's frame by a pure module
 * (`chartSvg.ts`).
 *
 * API pinned here, beyond the FR's Technical Design:
 *
 * - `chartSvg(input)` returns the whole SVG document as a string. `input` is
 *   `{ width, height, title, theme, white, style, visibility, view, originMs, data }`:
 *   the size in px, the chart title, the theme, whether to draw on white instead of the
 *   theme's background, the `ChartStyle`, the series shown, the x range shown
 *   (`{ t0Ms, t1Ms }`), the session start in x units, and the frame in uPlot's layout
 *   (`[x, …one array per CHART_SERIES entry]`, `null` a gap);
 * - the background is a full-size `<rect>` filled in that colour; each shown series is
 *   one `<path>` stroked in its look (its transparency as `stroke-opacity` or as the
 *   colour's alpha), a dashed series with a `stroke-dasharray`; the title, axis titles,
 *   tick labels and legend labels are `<text>`.
 *
 * Vitest runs in Node, with no DOMParser, so a small strict XML reader below checks the
 * output is well-formed.
 */

import { describe, expect, it } from "vitest";
import { chartSvg } from "./chartSvg";
import { CHART_SERIES, defaultVisibility, type ChartSeriesKey } from "./chartSeries";
import { DEFAULT_CHART_STYLE, setSeriesLook, type ChartStyle } from "./chartStyle";
import { THEME_COLORS } from "./theme";

// ---- A strict, small XML reader --------------------------------------------------------

interface XmlNode {
  name: string;
  attrs: Record<string, string>;
  children: XmlNode[];
  text: string;
}

const NAME = /^[A-Za-z_:][\w.:-]*/;

function decodeEntities(raw: string, at: number): string {
  return raw.replace(/&([^;\s&<]*);?/g, (whole, body: string) => {
    if (!whole.endsWith(";")) throw new Error(`bare & near ${at}`);
    if (body === "lt") return "<";
    if (body === "gt") return ">";
    if (body === "amp") return "&";
    if (body === "quot") return '"';
    if (body === "apos") return "'";
    const num = /^#(\d+)$/.exec(body) ?? /^#x([0-9a-f]+)$/i.exec(body);
    if (num) return String.fromCodePoint(Number.parseInt(num[1] ?? "", body[1] === "x" ? 16 : 10));
    throw new Error(`unknown entity &${body}; near ${at}`);
  });
}

/** Parses `xml`, throwing on anything not well-formed; returns the root element. */
function parseXml(xml: string): XmlNode {
  let i = 0;
  const stack: XmlNode[] = [];
  let root: XmlNode | null = null;
  const skip = (re: RegExp): void => {
    const m = re.exec(xml.slice(i));
    if (m) i += m[0].length;
  };
  while (i < xml.length) {
    if (xml.startsWith("<?", i)) {
      const end = xml.indexOf("?>", i);
      if (end < 0) throw new Error("open processing instruction");
      i = end + 2;
    } else if (xml.startsWith("<!--", i)) {
      const end = xml.indexOf("-->", i);
      if (end < 0) throw new Error("open comment");
      i = end + 3;
    } else if (xml.startsWith("<![CDATA[", i)) {
      const end = xml.indexOf("]]>", i);
      if (end < 0 || stack.length === 0) throw new Error("bad CDATA");
      const top = stack[stack.length - 1];
      if (top) top.text += xml.slice(i + 9, end);
      i = end + 3;
    } else if (xml.startsWith("<!DOCTYPE", i)) {
      const end = xml.indexOf(">", i);
      if (end < 0) throw new Error("open doctype");
      i = end + 1;
    } else if (xml.startsWith("</", i)) {
      i += 2;
      const m = NAME.exec(xml.slice(i));
      if (!m) throw new Error(`bad end tag at ${i}`);
      i += m[0].length;
      skip(/^\s*/);
      if (xml[i] !== ">") throw new Error(`bad end tag at ${i}`);
      i += 1;
      const open = stack.pop();
      if (!open || open.name !== m[0])
        throw new Error(`</${m[0]}> closes ${open?.name ?? "nothing"}`);
    } else if (xml[i] === "<") {
      i += 1;
      const m = NAME.exec(xml.slice(i));
      if (!m) throw new Error(`bad start tag at ${i}`);
      i += m[0].length;
      const node: XmlNode = { name: m[0], attrs: {}, children: [], text: "" };
      for (;;) {
        const space = /^\s*/.exec(xml.slice(i))?.[0].length ?? 0;
        i += space;
        if (xml.startsWith("/>", i) || xml[i] === ">") break;
        if (space === 0) throw new Error(`no space before an attribute at ${i}`);
        const a = NAME.exec(xml.slice(i));
        if (!a) throw new Error(`bad attribute at ${i}`);
        i += a[0].length;
        skip(/^\s*/);
        if (xml[i] !== "=") throw new Error(`attribute without value at ${i}`);
        i += 1;
        skip(/^\s*/);
        const quote = xml[i];
        if (quote !== '"' && quote !== "'") throw new Error(`unquoted attribute at ${i}`);
        const end = xml.indexOf(quote, i + 1);
        if (end < 0) throw new Error("open attribute value");
        const raw = xml.slice(i + 1, end);
        if (raw.includes("<")) throw new Error(`< in an attribute at ${i}`);
        if (a[0] in node.attrs) throw new Error(`attribute ${a[0]} twice`);
        node.attrs[a[0]] = decodeEntities(raw, i);
        i = end + 1;
      }
      const parent = stack[stack.length - 1];
      if (parent) parent.children.push(node);
      else if (root === null) root = node;
      else throw new Error("a second root element");
      if (xml.startsWith("/>", i)) {
        i += 2;
      } else {
        i += 1;
        stack.push(node);
      }
    } else {
      const end = xml.indexOf("<", i);
      const raw = xml.slice(i, end < 0 ? xml.length : end);
      if (raw.includes(">") && raw.includes("]]>")) throw new Error("]]> in text");
      const top = stack[stack.length - 1];
      if (top) top.text += decodeEntities(raw, i);
      else if (raw.trim() !== "") throw new Error(`text outside the root at ${i}`);
      i = end < 0 ? xml.length : end;
    }
  }
  if (stack.length > 0) throw new Error(`<${stack[stack.length - 1]?.name}> never closed`);
  if (root === null) throw new Error("no root element");
  return root;
}

function all(node: XmlNode, name: string): XmlNode[] {
  const out: XmlNode[] = [];
  const walk = (n: XmlNode): void => {
    if (n.name === name) out.push(n);
    for (const c of n.children) walk(c);
  };
  walk(node);
  return out;
}

/** Text of an element and everything under it. */
function textOf(n: XmlNode): string {
  return n.text + n.children.map(textOf).join("");
}

function texts(root: XmlNode): string[] {
  return all(root, "text").map((t) => textOf(t).trim());
}

/** `[r, g, b, a]` of a hex, `rgb()` or `rgba()` colour; `null` for anything else. */
function rgba(color: string | undefined): [number, number, number, number] | null {
  if (color === undefined) return null;
  const hex = /^#([0-9a-f]{6})([0-9a-f]{2})?$/i.exec(color.trim());
  if (hex) {
    const n = Number.parseInt(hex[1] ?? "000000", 16);
    const a = hex[2] === undefined ? 1 : Number.parseInt(hex[2], 16) / 255;
    return [(n >> 16) & 0xff, (n >> 8) & 0xff, n & 0xff, a];
  }
  const fn = /^rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*(?:,\s*([\d.]+)\s*)?\)$/i.exec(
    color.trim(),
  );
  if (fn) {
    return [Number(fn[1]), Number(fn[2]), Number(fn[3]), fn[4] === undefined ? 1 : Number(fn[4])];
  }
  return null;
}

function sameRgb(a: string | undefined, b: string): boolean {
  const x = rgba(a);
  const y = rgba(b);
  return x !== null && y !== null && x[0] === y[0] && x[1] === y[1] && x[2] === y[2];
}

/** Every x, y pair of a path's `d`, for absolute M / L commands. */
function points(d: string): [number, number][] {
  const out: [number, number][] = [];
  const re = /([MLml])\s*(-?[\d.]+(?:e-?\d+)?)[\s,]+(-?[\d.]+(?:e-?\d+)?)/g;
  for (const m of d.matchAll(re)) {
    if (m[1] === "m" || m[1] === "l") throw new Error("relative path commands");
    out.push([Number(m[2]), Number(m[3])]);
  }
  return out;
}

// ---- The frame -------------------------------------------------------------------------

/** One colour per series, so each path can be told apart. */
const COLORS: Record<ChartSeriesKey, string> = {
  vbus: "#d00000",
  ibus: "#00a000",
  power: "#0000d0",
  dp: "#a000a0",
  dm: "#a06000",
  cc1: "#006060",
  cc2: "#606000",
};

function coloured(): ChartStyle {
  let style = DEFAULT_CHART_STYLE;
  for (const s of CHART_SERIES) style = setSeriesLook(style, s.key, { color: COLORS[s.key] });
  return style;
}

/** 10 s of a 5 V, 1 A load at 10 Hz, D± at 0.6 V, CC1 at 1.7 V. */
function frame(): (number | null)[][] {
  const x: number[] = [];
  for (let t = 0; t <= 10_000; t += 100) x.push(t);
  const at = (f: (t: number) => number): number[] => x.map(f);
  return [
    x,
    at((t) => 5 + 0.01 * Math.sin(t / 300)),
    at(() => 1),
    at(() => 5),
    at(() => 0.6),
    at(() => 0.6),
    at(() => 1.7),
    at(() => 0),
  ];
}

function render(over: Partial<Parameters<typeof chartSvg>[0]> = {}): string {
  return chartSvg({
    width: 800,
    height: 400,
    title: "Bench run",
    theme: "light",
    white: false,
    style: coloured(),
    visibility: defaultVisibility(),
    view: { t0Ms: 2_000, t1Ms: 6_000 },
    originMs: 0,
    data: frame(),
    ...over,
  });
}

function seriesPath(root: XmlNode, key: ChartSeriesKey): XmlNode | undefined {
  return all(root, "path").find((p) => sameRgb(p.attrs.stroke, COLORS[key]));
}

// ---- The tests -------------------------------------------------------------------------

describe("the SVG is valid XML with series as paths (AC-AXHK2V.7)", () => {
  it("AC-AXHK2V.7: parses as one svg element of the asked size", () => {
    const root = parseXml(render());
    expect(root.name).toBe("svg");
    expect(root.attrs.xmlns).toBe("http://www.w3.org/2000/svg");
    expect(Number.parseFloat(root.attrs.width ?? "")).toBe(800);
    expect(Number.parseFloat(root.attrs.height ?? "")).toBe(400);
  });

  it("AC-AXHK2V.7: each shown series is a path in its colour, hidden ones are not drawn", () => {
    const root = parseXml(render());
    const shown = defaultVisibility();
    for (const s of CHART_SERIES) {
      const path = seriesPath(root, s.key);
      if (shown[s.key]) {
        expect(path, `${s.label} is a path`).toBeDefined();
        expect(points(path?.attrs.d ?? "").length).toBeGreaterThan(1);
      } else {
        expect(path, `${s.label} is hidden`).toBeUndefined();
      }
    }
  });

  it("AC-AXHK2V.7: dashes stay: VBUS solid, IBUS dashed", () => {
    const root = parseXml(render());
    const vbus = seriesPath(root, "vbus");
    const ibus = seriesPath(root, "ibus");
    expect(vbus?.attrs["stroke-dasharray"] ?? "none").toBe("none");
    expect(ibus?.attrs["stroke-dasharray"] ?? "none").not.toBe("none");
  });

  it("AC-AXHK2V.7: a series' width and transparency carry into its path", () => {
    const style = setSeriesLook(coloured(), "vbus", { width: 3, opacity: 0.5 });
    const vbus = seriesPath(parseXml(render({ style })), "vbus");
    expect(Number.parseFloat(vbus?.attrs["stroke-width"] ?? "")).toBe(3);
    const alpha = rgba(vbus?.attrs.stroke)?.[3] ?? 1;
    const opacity = Number.parseFloat(vbus?.attrs["stroke-opacity"] ?? "1") * alpha;
    expect(opacity).toBeCloseTo(0.5, 2);
  });
});

describe("the export shows the current view (AC-AXHK2V.6)", () => {
  it("AC-AXHK2V.6: draws only the view: every point lies inside the image", () => {
    const root = parseXml(render());
    for (const s of ["vbus", "ibus", "dp", "dm", "cc1", "cc2"] as const) {
      const pts = points(seriesPath(root, s)?.attrs.d ?? "");
      expect(pts.length).toBeGreaterThan(1);
      for (const [x, y] of pts) {
        expect(x).toBeGreaterThanOrEqual(0);
        expect(x).toBeLessThanOrEqual(800);
        expect(y).toBeGreaterThanOrEqual(0);
        expect(y).toBeLessThanOrEqual(400);
      }
    }
  });

  it("AC-AXHK2V.6: the view's time span sets the time axis, not the whole frame", () => {
    const labels = texts(parseXml(render()))
      .map((t) => /^(\d+(?:\.\d+)?) s$/.exec(t))
      .filter((m): m is RegExpExecArray => m !== null)
      .map((m) => Number(m[1]));
    expect(labels.length).toBeGreaterThanOrEqual(2);
    for (const s of labels) {
      expect(s).toBeGreaterThanOrEqual(2);
      expect(s).toBeLessThanOrEqual(6);
    }
    // The path spans the plot from the view's start to its end, not 0 to 10 s.
    const xs = points(seriesPath(parseXml(render()), "vbus")?.attrs.d ?? "").map(([x]) => x);
    const width = Math.max(...xs) - Math.min(...xs);
    expect(width).toBeGreaterThan(400);
  });

  it("AC-AXHK2V.6: holds the title, the axis titles and the legend", () => {
    const t = texts(parseXml(render()));
    expect(t).toContain("Bench run");
    expect(t).toContain("Volts (V)");
    expect(t).toContain("Amps (A)");
    const shown = defaultVisibility();
    for (const s of CHART_SERIES.filter((d) => shown[d.key])) {
      expect(
        t.some((x) => x.startsWith(s.label)),
        `legend ${s.label}`,
      ).toBe(true);
    }
  });

  it("AC-AXHK2V.6: draws on the theme's background, or on white by choice", () => {
    const fill = (svg: string): string[] =>
      all(parseXml(svg), "rect")
        .filter((r) => r.attrs.width === "100%" || Number.parseFloat(r.attrs.width ?? "") === 800)
        .map((r) => r.attrs.fill ?? "");
    expect(fill(render({ theme: "dark" })).some((f) => sameRgb(f, THEME_COLORS.dark.bg))).toBe(
      true,
    );
    expect(fill(render({ theme: "light" })).some((f) => sameRgb(f, THEME_COLORS.light.bg))).toBe(
      true,
    );
    const white = fill(render({ theme: "dark", white: true }));
    expect(white.some((f) => sameRgb(f, "#ffffff"))).toBe(true);
    expect(white.some((f) => sameRgb(f, THEME_COLORS.dark.bg))).toBe(false);
  });

  it("AC-AXHK2V.6: in the dark theme the text is the theme's, on white it is dark", () => {
    const fills = (svg: string): string[] =>
      all(parseXml(svg), "text").map((n) => n.attrs.fill ?? "");
    const dark = fills(render({ theme: "dark" }));
    expect(dark.some((f) => sameRgb(f, THEME_COLORS.dark.text))).toBe(true);
    const onWhite = fills(render({ theme: "dark", white: true }));
    expect(onWhite.some((f) => sameRgb(f, THEME_COLORS.dark.text))).toBe(false);
  });
});

describe("the title is text, never markup (AC-AXHK2V.4)", () => {
  it("AC-AXHK2V.4: a title holding markup and an ampersand stays text", () => {
    const title = '<script>alert("x")</script> & <b>bold</b>';
    const svg = render({ title });
    expect(svg).not.toContain("<script");
    expect(svg).not.toContain("<b>");
    const root = parseXml(svg);
    expect(all(root, "script")).toEqual([]);
    expect(all(root, "b")).toEqual([]);
    expect(texts(root)).toContain(title);
  });

  it("AC-AXHK2V.4: an empty title draws no title text", () => {
    const t = texts(parseXml(render({ title: "" })));
    expect(t).not.toContain("Bench run");
    expect(t.every((x) => x !== "")).toBe(true);
  });
});

describe("the SVG follows the live chart's axis settings (AC-AXHK2V.6)", () => {
  /** Only VBUS and IBUS, IBUS rising from 0.5 A to 1.5 A over the 10 s. */
  function rising(): (number | null)[][] {
    const f = frame();
    const x = f[0] ?? [];
    f[2] = x.map((t) => 0.5 + (t ?? 0) / 10_000);
    return f;
  }
  const twoSeries = { ...defaultVisibility(), dp: false, dm: false, cc1: false, cc2: false };

  /** The value tick labels on one side, as `[value, y]`, top to bottom. */
  function ticks(root: XmlNode, side: "left" | "right"): [number, number][] {
    return all(root, "text")
      .filter((t) => t.attrs["text-anchor"] === (side === "left" ? "end" : "start"))
      .filter((t) => t.attrs.transform === undefined && t.attrs.dy === "4")
      .filter((t) => (side === "left" ? Number(t.attrs.x) < 100 : Number(t.attrs.x) > 700))
      .map((t): [number, number] => [Number(textOf(t).trim().replace("−", "-")), Number(t.attrs.y)])
      .filter(([v]) => Number.isFinite(v))
      .sort((a, b) => a[1] - b[1]);
  }

  it("AC-AXHK2V.6: with zero-based axes on, the volts axis starts at 0", () => {
    const off = ticks(parseXml(render({ visibility: twoSeries })), "left");
    expect(off.length).toBeGreaterThan(1);
    expect(off.some(([v]) => v === 0)).toBe(false);

    const on = ticks(parseXml(render({ visibility: twoSeries, zeroBased: true })), "left");
    expect(on.length).toBeGreaterThan(1);
    // The lowest label on the axis reads 0.
    const bottom = on[on.length - 1];
    expect(bottom?.[0]).toBe(0);
    expect(Math.min(...on.map(([v]) => v))).toBe(0);
  });

  it("AC-AXHK2V.6: with reverse current on, the amps axis is flipped", () => {
    const ibusYs = (svg: string): number[] =>
      points(seriesPath(parseXml(svg), "ibus")?.attrs.d ?? "").map(([, y]) => y);

    const normal = ibusYs(render({ visibility: twoSeries, data: rising() }));
    expect(normal.length).toBeGreaterThan(1);
    // Rising current climbs the plot (y falls) the usual way…
    expect(normal[normal.length - 1] ?? 0).toBeLessThan(normal[0] ?? 0);
    const up = ticks(parseXml(render({ visibility: twoSeries, data: rising() })), "right");
    expect((up[0]?.[0] ?? 0) > (up[up.length - 1]?.[0] ?? 0)).toBe(true);

    // …and falls down it on a reversed axis, its labels read top to bottom upwards.
    const svg = render({ visibility: twoSeries, data: rising(), reverseCurrent: true });
    const reversed = ibusYs(svg);
    expect(reversed[reversed.length - 1] ?? 0).toBeGreaterThan(reversed[0] ?? 0);
    const down = ticks(parseXml(svg), "right");
    expect(down.length).toBeGreaterThan(1);
    expect((down[0]?.[0] ?? 0) < (down[down.length - 1]?.[0] ?? 0)).toBe(true);
    // VBUS, on the volts axis, is not flipped.
    const vbus = ticks(parseXml(svg), "left");
    expect((vbus[0]?.[0] ?? 0) > (vbus[vbus.length - 1]?.[0] ?? 0)).toBe(true);
  });
});
