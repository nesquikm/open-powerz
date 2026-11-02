// AC-AXHK2T.5, AC-AXHK2T.8: a README line and the how-to describe the ruler, segment
// marks, E.D., the sub-chart and Reverse current; the how-to states the E.D. definition
// in the very words the UI shows (`ED_DEFINITION`); the README gate maps the FR to that line.
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { ED_DEFINITION } from "../../src/lib/chartExtras";
import type { FeatureMap } from "./readme-features";
import { parseFeatureAreas } from "./readme-features";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, "../../../..");
const readme = readFileSync(join(repoRoot, "README.md"), "utf8");
const map = JSON.parse(readFileSync(join(here, "readme-features.json"), "utf8")) as FeatureMap;
const whole = readFileSync(join(repoRoot, "docs/how-to/explore-a-session.md"), "utf8");
/** The how-to's own section, up to the next heading of its level. */
const howTo = /^## Chart tools\b[^]*?(?=^## |(?![^]))/m.exec(whole)?.[0] ?? "";

const isToolsLine = (bullet: string): boolean =>
  /\bruler\b/i.test(bullet) &&
  /segment marks?/i.test(bullet) &&
  /\bE\.D\./.test(bullet) &&
  /sub-chart/i.test(bullet) &&
  /revers/i.test(bullet);

describe("the chart tools are documented (AC-AXHK2T.8)", () => {
  it("have a README Features line", () => {
    const bullets = [...parseFeatureAreas(readme).values()].flat();
    expect(bullets.filter(isToolsLine)).toHaveLength(1);
  });

  it("the README gate maps the FR to that line", () => {
    const value = map.AXHK2T ?? "";
    const [area = "", phrase = ""] = value.split(": ");
    expect(area).toBe("recorder-and-charts");
    expect(phrase).not.toBe("");
    const lines = (parseFeatureAreas(readme).get(area) ?? []).filter(isToolsLine);
    expect(lines).toHaveLength(1);
    expect(lines[0]?.toLowerCase()).toContain(phrase.toLowerCase());
  });

  it("the README line links to the how-to section", () => {
    const line = [...parseFeatureAreas(readme).values()].flat().find(isToolsLine);
    expect(line).toContain("docs/how-to/explore-a-session.md#chart-tools");
  });

  it.each([
    ["the two ruler cursors", /\bT1\b[^]*\bT2\b/],
    ["delta time, charge and energy", /delta time[^]*\bAh\b[^]*\bWh\b|ΔT[^]*ΔAh[^]*ΔWh/i],
    ["the averages", /average[^\n]*\bV\b[^\n]*\bA\b[^\n]*\bW\b|Avg V[^]*Avg A[^]*Avg W/i],
    ["that the ruler reads the raw samples", /raw samples/i],
    ["moving a cursor by keyboard", /arrow keys?/i],
    ["screen readers", /screen reader/i],
    ["segment marks", /segment marks/i],
    ["the default 10% step", /10\s?%/],
    ["charge or energy for the marks", /charge[^\n]*energy|energy[^\n]*charge/i],
    ["the current run", /current run/i],
    ["the E.D. series", /\bE\.D\./],
    ["the sub-chart", /sub-chart/i],
    ["its own series", /own series/i],
    ["the shared time range, cursor and zoom", /time range[^\n]*cursor[^\n]*zoom/i],
    ["Reverse current", /Reverse current/],
    ["that an opened recording has the tools too", /open(ed)? recording/i],
    ["that the options are kept", /\b(kept|remember(ed|s)?|persist(s|ed)?)\b/i],
  ])("the how-to describes %s", (_name, pattern) => {
    expect(howTo).toMatch(pattern);
  });

  it("the how-to says Reverse current changes no data, export or statistics", () => {
    const reverse = howTo
      .split("\n")
      .filter((l) => /Reverse current/.test(l))
      .join("\n");
    expect(reverse).toMatch(/data/i);
    expect(reverse).toMatch(/export/i);
    expect(reverse).toMatch(/statistic/i);
  });
});

describe("the docs state the E.D. definition (AC-AXHK2T.5)", () => {
  it("in the words the UI shows", () => {
    expect(ED_DEFINITION.length).toBeGreaterThan(0);
    expect(howTo.replace(/\s+/g, " ")).toContain(ED_DEFINITION);
  });
});
