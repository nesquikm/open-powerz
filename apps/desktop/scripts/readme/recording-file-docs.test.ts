// AC-AXHK2S.8: a README line and the how-to describe opening a recording and exporting
// a vendor .db, and the README gate maps the FR to that line.
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
const whole = readFileSync(join(repoRoot, "docs/how-to/record-a-csv.md"), "utf8");
/** The how-to's own section, up to the next heading of its level. */
const howTo = /^## Open a recording\b[^]*?(?=^## |(?![^]))/m.exec(whole)?.[0] ?? "";

const isFileLine = (bullet: string): boolean =>
  /\bopen\b/i.test(bullet) && /\.db\b/i.test(bullet) && /\bcsv\b/i.test(bullet);

describe("opening a recording is documented (AC-AXHK2S.8)", () => {
  it("has a README Features line", () => {
    const bullets = [...parseFeatureAreas(readme).values()].flat();
    expect(bullets.filter(isFileLine)).toHaveLength(1);
  });

  it("the README gate maps the FR to that line", () => {
    const value = map.AXHK2S ?? "";
    const [area = "", phrase = ""] = value.split(": ");
    expect(phrase).not.toBe("");
    const lines = (parseFeatureAreas(readme).get(area) ?? []).filter(isFileLine);
    expect(lines).toHaveLength(1);
    expect(lines[0]?.toLowerCase()).toContain(phrase.toLowerCase());
  });

  it("the README line links to the how-to", () => {
    const line = [...parseFeatureAreas(readme).values()].flat().find(isFileLine);
    expect(line).toContain("docs/how-to/record-a-csv.md");
  });

  it.each([
    ["opening a recording", /open recording/i],
    ["the file tab", /\bfile\b[^\n]*\btab\b|\btab\b[^\n]*\bfile\b/i],
    ["that no meter is needed", /no meter|without a meter/i],
    ["damaged rows", /damaged rows?/i],
    ["the row limit of a file", /10,000,000/],
    ["where charge and energy come from", /\bCHARGE\b[^]*\bENERGY\b[^]*\blast\b/],
    ["the vendor database", /vendor[^\n]*\.db|\.db[^\n]*vendor/i],
    ["the export", /export \.db/i],
    ["Cancel", /\bcancel\b/i],
  ])("the how-to describes %s", (_name, pattern) => {
    expect(howTo).toMatch(pattern);
  });

  it("the how-to says the .db layout is unproven against the vendor app", () => {
    expect(howTo).toMatch(/(not|un|never)[^\n]*(tested|proven|checked)[^\n]*vendor/i);
  });
});
