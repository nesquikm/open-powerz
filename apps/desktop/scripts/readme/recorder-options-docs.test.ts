// AC-AXHK2R.8: a README line and the how-to describe the recorder's options, and the
// README gate maps the FR to that line.
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
const howTo = readFileSync(join(repoRoot, "docs/how-to/record-a-csv.md"), "utf8");
const explore = readFileSync(join(repoRoot, "docs/how-to/explore-a-session.md"), "utf8");

const isRecorderLine = (bullet: string): boolean =>
  /\bpause\b/i.test(bullet) && /auto-stop/i.test(bullet);

describe("the recorder's options are documented (AC-AXHK2R.8)", () => {
  it("has a README Features line", () => {
    const bullets = [...parseFeatureAreas(readme).values()].flat();
    expect(bullets.filter(isRecorderLine)).toHaveLength(1);
  });

  it("the README gate maps the FR to that line", () => {
    const value = map.AXHK2R ?? "";
    const [area = "", phrase = ""] = value.split(": ");
    expect(phrase).not.toBe("");
    const lines = (parseFeatureAreas(readme).get(area) ?? []).filter(isRecorderLine);
    expect(lines).toHaveLength(1);
    expect(lines[0]?.toLowerCase()).toContain(phrase.toLowerCase());
  });

  it("the README line links to the how-to", () => {
    const line = [...parseFeatureAreas(readme).values()].flat().find(isRecorderLine);
    expect(line).toContain("docs/how-to/record-a-csv.md");
  });

  it.each([
    ["pause and resume", /\bpause\b[^]*\bresume\b/i],
    ["auto-stop", /auto-stop/i],
    ["the row limit", /row limit/i],
    ["the row limit's default", /10,000,000/],
    ["the prefix", /prefix/i],
    ["the folder", /folder/i],
    ["the memory depth", /live chart buffer/i],
  ])("the how-to describes %s", (_name, pattern) => {
    expect(howTo).toMatch(pattern);
  });

  it("the how-to says powerz stream has none of this", () => {
    expect(howTo).toMatch(/`powerz stream`[^\n]*(does not|doesn't|has no|no )/i);
  });
});

describe("the how-to pages call it the live chart buffer (AC-2VPF6P.3)", () => {
  it.each([
    ["record-a-csv", howTo],
    ["explore-a-session", explore],
  ])("%s says live chart buffer, not chart memory", (_name, page) => {
    expect(page).toMatch(/live chart buffer/i);
    expect(page).not.toMatch(/chart'?s? memory/i);
  });

  it("record-a-csv uses the app's words for the span and the history", () => {
    expect(howTo).toMatch(/live chart buffer[^]{0,200}10 minutes[^]{0,80}1000 SPS/i);
    expect(howTo).toMatch(/session history keeps far more/i);
  });
});
