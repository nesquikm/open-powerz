// AC-AXHK2V.8: a README line and the how-to describe the chart's appearance settings
// (axis presets, series colour / width / transparency, the title) and Save image, and
// say they are kept for the app, not per meter; the README gate maps the FR to that line.
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import type { FeatureMap } from "./readme-features";
import { parseFeatureAreas } from "./readme-features";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, "../../../..");
const readme = readFileSync(join(repoRoot, "README.md"), "utf8");
const map = JSON.parse(readFileSync(join(here, "readme-features.json"), "utf8")) as FeatureMap;
const howTo = readFileSync(join(repoRoot, "docs/how-to/explore-a-session.md"), "utf8");

const isStyleLine = (bullet: string): boolean =>
  /save (the chart as an )?image|\bPNG\b/i.test(bullet) &&
  /\bSVG\b/.test(bullet) &&
  /\bPDF\b/.test(bullet) &&
  /colou?r/i.test(bullet) &&
  /title/i.test(bullet);

describe("chart appearance and image export are documented (AC-AXHK2V.8)", () => {
  it("AC-AXHK2V.8: have one README Features line", () => {
    const bullets = [...parseFeatureAreas(readme).values()].flat();
    expect(bullets.filter(isStyleLine)).toHaveLength(1);
  });

  it("AC-AXHK2V.8: the README gate maps the FR to that line", () => {
    const value = map.AXHK2V ?? "";
    const [area = "", phrase = ""] = value.split(": ");
    expect(area).toBe("recorder-and-charts");
    expect(phrase).not.toBe("");
    const lines = (parseFeatureAreas(readme).get(area) ?? []).filter(isStyleLine);
    expect(lines).toHaveLength(1);
    expect(lines[0]?.toLowerCase()).toContain(phrase.toLowerCase());
  });

  it("AC-AXHK2V.8: the README line links to the how-to", () => {
    const line = [...parseFeatureAreas(readme).values()].flat().find(isStyleLine);
    expect(line).toContain("docs/how-to/explore-a-session.md#");
  });

  it.each([
    ["the axis presets", /Auto from zero/],
    ["a fixed VBUS range", /0[-–]24 V/],
    ["a fixed IBUS range", /0[-–]8 A/],
    ["each series' colour, width and transparency", /colou?r[^]*width[^]*transparen/i],
    ["the Reset", /\bReset\b/],
    ["the low-contrast warning", /contrast/i],
    ["the title", /\btitle\b/i],
    ["Save image", /Save image/],
    ["the three formats", /\bPNG\b[^]*\bSVG\b[^]*\bPDF\b/],
    ["the size limits", /320[^]*7680|7680[^]*320/],
    ["the white background", /white/i],
    ["no serial in saved images", /serial/i],
    ["kept for the app, not per meter", /not per meter|whichever meter|every meter/i],
  ])("AC-AXHK2V.8: the how-to covers %s", (_what, pattern) => {
    expect(howTo).toMatch(pattern);
  });
});
