// FR 7V4B90: the README lists every shipped feature, linked to its guide.
// The coverage check maps each CHANGELOG "Added" entry (by its FR id) to a
// README Features area, using readme-features.json next to this file; the link
// check makes sure every relative README link points at a real file.
// `bun run test` runs both against the real repository, so they are the gate.
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, it } from "vitest";
import {
  checkFeatureCoverage,
  checkReadmeLinks,
  type FeatureMap,
  parseAddedEntries,
  parseFeatureAreas,
  readmeLinks,
  slugify,
} from "./readme-features";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, "../../../..");
const scriptPath = join(here, "readme-features.ts");
const mapPath = join(here, "readme-features.json");

const CHANGELOG = [
  "# Changelog",
  "",
  '## [0.2.0] — 2026-10-22 — "Live Wire"',
  "",
  "### Added",
  "- Live chart of VBUS and IBUS with per-series toggles (AAAA01)",
  "- Recording and CSV export, shared with `powerz stream` (AAAA02)",
  "",
  "### Fixed",
  "- The chart no longer leaks memory (FFFF01)",
  "",
  "Total test count at release: 10 tests, 0 failures, 0 errors.",
  "",
  "## [0.1.0] — 2026-10-20",
  "",
  "### Added",
  "- `powerz` command-line tool: `list`, `info`, `read` (BBBB01)",
  "",
  "### Changed",
  "- A new app icon (CCCC01)",
  "",
].join("\n");

const README = [
  "# demo",
  "",
  "## Install",
  "",
  "- not a feature line",
  "",
  "## Features",
  "",
  "### Recorder and charts",
  "",
  "- Chart VBUS live ([explore](docs/how-to/explore.md))",
  "- Record to CSV ([record](docs/how-to/record.md))",
  "",
  "### CLI",
  "",
  "- Script the meter with `powerz` ([CLI](docs/reference/cli.md))",
  "",
  "### Empty area",
  "",
  "## Coming next",
  "",
  "- A browser edition",
  "",
].join("\n");

describe("slugify", () => {
  it.each([
    ["Live meter", "live-meter"],
    ["Recorder and charts", "recorder-and-charts"],
    ["Demo device and simulator", "demo-device-and-simulator"],
    ["CLI", "cli"],
    ["Try it without a meter", "try-it-without-a-meter"],
    [
      "When the meter is unplugged, and plugged back in!",
      "when-the-meter-is-unplugged-and-plugged-back-in",
    ],
  ])("%s -> %s", (heading, slug) => {
    expect(slugify(heading)).toBe(slug);
  });
});

describe("parseAddedEntries", () => {
  it("returns every Added bullet of every version with its id, and nothing from other groups", () => {
    const entries = parseAddedEntries(CHANGELOG);
    expect(entries.map((e) => [e.version, e.id])).toEqual([
      ["0.2.0", "AAAA01"],
      ["0.2.0", "AAAA02"],
      ["0.1.0", "BBBB01"],
    ]);
    expect(entries[0]?.text).toContain("Live chart of VBUS and IBUS");
  });

  it("reads CRLF line endings", () => {
    const ids = parseAddedEntries(CHANGELOG.replace(/\n/g, "\r\n")).map((e) => e.id);
    expect(ids).toEqual(["AAAA01", "AAAA02", "BBBB01"]);
  });
});

describe("parseFeatureAreas", () => {
  it("maps each Features sub-heading slug to its bullet lines, ignoring other sections", () => {
    const areas = parseFeatureAreas(README);
    expect([...areas.keys()]).toEqual(["recorder-and-charts", "cli", "empty-area"]);
    expect(areas.get("recorder-and-charts")).toHaveLength(2);
    expect(areas.get("recorder-and-charts")?.[1]).toContain("Record to CSV");
    expect(areas.get("cli")).toHaveLength(1);
    expect(areas.get("empty-area")).toEqual([]);
  });
});

describe("checkFeatureCoverage (AC-7V4B90.6)", () => {
  const fullMap: FeatureMap = {
    AAAA01: "recorder-and-charts",
    AAAA02: "recorder-and-charts",
    BBBB01: "cli",
  };

  it("passes when every Added feature maps to a Features area with a line", () => {
    expect(checkFeatureCoverage(CHANGELOG, README, fullMap)).toEqual([]);
  });

  it("does not ask for Fixed or Changed entries", () => {
    const findings = checkFeatureCoverage(CHANGELOG, README, fullMap);
    expect(findings.map((f) => f.id)).not.toContain("FFFF01");
    expect(findings.map((f) => f.id)).not.toContain("CCCC01");
  });

  it("fails for an Added feature missing from the mapping file", () => {
    const partial = Object.fromEntries(Object.entries(fullMap).filter(([id]) => id !== "BBBB01"));
    const findings = checkFeatureCoverage(CHANGELOG, README, partial);
    expect(findings).toHaveLength(1);
    expect(findings[0]?.id).toBe("BBBB01");
    expect(findings[0]?.reason.length).toBeGreaterThan(0);
  });

  it("fails when the mapped README area does not exist", () => {
    const findings = checkFeatureCoverage(CHANGELOG, README, { ...fullMap, BBBB01: "pd-analyzer" });
    expect(findings.map((f) => f.id)).toEqual(["BBBB01"]);
    expect(findings[0]?.reason).toContain("pd-analyzer");
  });

  it("fails when the mapped README area has no feature line", () => {
    const findings = checkFeatureCoverage(CHANGELOG, README, { ...fullMap, BBBB01: "empty-area" });
    expect(findings.map((f) => f.id)).toEqual(["BBBB01"]);
  });

  it("fails when the README loses the Features line the mapping points at", () => {
    const noCli = README.replace(/### CLI\n\n- Script the meter[^\n]*\n/, "");
    const findings = checkFeatureCoverage(CHANGELOG, noCli, fullMap);
    expect(findings.map((f) => f.id)).toEqual(["BBBB01"]);
  });

  it("accepts null for an entry that is explicitly not a user-facing feature", () => {
    expect(checkFeatureCoverage(CHANGELOG, README, { ...fullMap, AAAA02: null })).toEqual([]);
  });
});

describe("checkFeatureCoverage with feature phrases (AC-7V4B90.6)", () => {
  // A mapping value "<area-slug>: <phrase>" is covered only by a bullet in that
  // area containing <phrase> (case-insensitive); a bare "<area-slug>" keeps the
  // area-level meaning.
  const phraseMap: FeatureMap = {
    AAAA01: "recorder-and-charts: chart vbus live",
    AAAA02: "recorder-and-charts: RECORD TO CSV",
    BBBB01: "cli: script the meter",
  };

  it("passes when each phrase is in a bullet of its mapped area, ignoring case", () => {
    expect(checkFeatureCoverage(CHANGELOG, README, phraseMap)).toEqual([]);
  });

  it("mixes phrase values with plain area values", () => {
    const mixed: FeatureMap = { ...phraseMap, AAAA01: "recorder-and-charts", BBBB01: null };
    expect(checkFeatureCoverage(CHANGELOG, README, mixed)).toEqual([]);
  });

  it("fails when no bullet in the area contains the phrase, even though the area has other bullets", () => {
    const map: FeatureMap = { ...phraseMap, AAAA02: "recorder-and-charts: export to parquet" };
    const findings = checkFeatureCoverage(CHANGELOG, README, map);
    expect(findings.map((f) => f.id)).toEqual(["AAAA02"]);
    expect(findings[0]?.reason).toContain("recorder-and-charts");
    expect(findings[0]?.reason).toContain("export to parquet");
  });

  it("fails when the feature's own README line is deleted but its area keeps other lines", () => {
    const noCsv = README.replace(/- Record to CSV[^\n]*\n/, "");
    expect(parseFeatureAreas(noCsv).get("recorder-and-charts")).toHaveLength(1);
    const findings = checkFeatureCoverage(CHANGELOG, noCsv, phraseMap);
    expect(findings.map((f) => f.id)).toEqual(["AAAA02"]);
    expect(findings[0]?.reason).toContain("recorder-and-charts");
    expect(findings[0]?.reason.toLowerCase()).toContain("record to csv");
  });

  it("does not count the phrase when it sits in a different area", () => {
    const map: FeatureMap = { ...phraseMap, BBBB01: "recorder-and-charts: script the meter" };
    const findings = checkFeatureCoverage(CHANGELOG, README, map);
    expect(findings.map((f) => f.id)).toEqual(["BBBB01"]);
    expect(findings[0]?.reason).toContain("script the meter");
  });
});

describe("checkFeatureCoverage with an Added bullet missing its (ID) (AC-7V4B90.6)", () => {
  const fullMap: FeatureMap = {
    AAAA01: "recorder-and-charts",
    AAAA02: "recorder-and-charts",
    BBBB01: "cli",
  };

  it("reports the bullet instead of silently skipping it, naming its version", () => {
    const changelog = CHANGELOG.replace(
      "- Recording and CSV export, shared with `powerz stream` (AAAA02)\n",
      "- Recording and CSV export, shared with `powerz stream` (AAAA02)\n- A feature someone forgot to tag\n",
    );
    const findings = checkFeatureCoverage(changelog, README, fullMap);
    expect(findings).toHaveLength(1);
    expect(findings[0]?.id.length).toBeGreaterThan(0);
    expect(findings[0]?.reason).toContain("0.2.0");
  });

  it("still ignores untagged bullets outside Added groups", () => {
    const changelog = CHANGELOG.replace(
      "- A new app icon (CCCC01)\n",
      "- A new app icon (CCCC01)\n- An untagged change\n",
    );
    expect(checkFeatureCoverage(changelog, README, fullMap)).toEqual([]);
  });
});

describe("readmeLinks / checkReadmeLinks (AC-7V4B90.7)", () => {
  const made: string[] = [];
  afterEach(() => {
    for (const d of made.splice(0)) rmSync(d, { recursive: true, force: true });
  });
  function fixtureRepo(files: Record<string, string>): string {
    const dir = mkdtempSync(join(tmpdir(), "readme-features-"));
    made.push(dir);
    for (const [name, body] of Object.entries(files)) {
      mkdirSync(dirname(join(dir, name)), { recursive: true });
      writeFileSync(join(dir, name), body);
    }
    return dir;
  }

  const LINKS = [
    "# x",
    "",
    "See [guide](docs/guide.md#a-section) and [license](LICENSE).",
    "![shot](docs/images/shot.png)",
    "[site](https://example.com/page.md) [mail](mailto:a@b.c) [top](#x)",
    "",
    "```bash",
    "echo [not](a-link.md)",
    "```",
  ].join("\n");

  it("lists relative links and images, not external links or code blocks", () => {
    const links = readmeLinks(LINKS);
    expect(links).toEqual(
      expect.arrayContaining(["docs/guide.md#a-section", "LICENSE", "docs/images/shot.png", "#x"]),
    );
    expect(links).not.toContain("https://example.com/page.md");
    expect(links).not.toContain("mailto:a@b.c");
    expect(links).not.toContain("a-link.md");
  });

  it("passes when every relative link resolves to a file", () => {
    const root = fixtureRepo({
      "docs/guide.md": "# g",
      LICENSE: "MIT",
      "docs/images/shot.png": "png",
    });
    expect(checkReadmeLinks(LINKS, root)).toEqual([]);
  });

  it("flags a link to a missing file, ignoring its #fragment", () => {
    const root = fixtureRepo({ LICENSE: "MIT", "docs/images/shot.png": "png" });
    const findings = checkReadmeLinks(LINKS, root);
    expect(findings).toHaveLength(1);
    expect(findings[0]?.link).toBe("docs/guide.md#a-section");
    expect(findings[0]?.reason.length).toBeGreaterThan(0);
  });

  it("flags an in-page anchor with no matching heading", () => {
    const root = fixtureRepo({});
    const findings = checkReadmeLinks("# Title\n\n[ok](#title) [bad](#no-such-heading)\n", root);
    expect(findings.map((f) => f.link)).toEqual(["#no-such-heading"]);
  });

  it("reports a malformed escape instead of throwing, in anchors and paths", () => {
    const root = fixtureRepo({});
    const findings = checkReadmeLinks("# Title\n\n[a](#100%) [b](docs/50%.md)\n", root);
    expect(findings.map((f) => f.link)).toEqual(["#100%", "docs/50%.md"]);
    expect(findings.every((f) => f.reason.includes("malformed"))).toBe(true);
  });

  it("flags a link that leaves the repository even when the file exists", () => {
    const outer = fixtureRepo({ "notes/x.md": "outside", "repo/LICENSE": "MIT" });
    const findings = checkReadmeLinks("[x](../notes/x.md) [ok](LICENSE)\n", join(outer, "repo"));
    expect(findings.map((f) => f.link)).toEqual(["../notes/x.md"]);
    expect(findings[0]?.reason).toContain("outside the repository");
  });

  it("keeps a longer code fence open past a shorter fence line inside it", () => {
    const md = ["````md", "```", "[in](inside-fence.md)", "```", "````", "[out](LICENSE)"].join(
      "\n",
    );
    expect(readmeLinks(md)).toEqual(["LICENSE"]);
  });

  it("does not close a code fence on a fence line that carries an info string", () => {
    const md = ["```", "```bash", "[in](inside-fence.md)", "```", "[out](LICENSE)"].join("\n");
    expect(readmeLinks(md)).toEqual(["LICENSE"]);
  });

  it("keeps inline code text in heading anchors, as GitHub does", () => {
    const root = fixtureRepo({});
    const md = "## The `powerz` CLI\n\n[ok](#the-powerz-cli) [bad](#the--cli)\n";
    expect(checkReadmeLinks(md, root).map((f) => f.link)).toEqual(["#the--cli"]);
  });

  it("CLI exits 1 for a repository whose README misses an Added feature", () => {
    const root = fixtureRepo({
      "README.md": README,
      "CHANGELOG.md": "## [9.9.9]\n\n### Added\n- Something nobody listed (ZZZZZZ)\n",
      "docs/how-to/explore.md": "x",
      "docs/how-to/record.md": "x",
      "docs/reference/cli.md": "x",
    });
    const r = spawnSync("bun", [scriptPath, root], { encoding: "utf8", timeout: 30_000 });
    expect(r.error).toBeUndefined();
    expect(r.status).toBe(1);
    expect(`${r.stdout}${r.stderr}`).toContain("ZZZZZZ");
  });
});

describe("repository gate", () => {
  const readme = readFileSync(join(repoRoot, "README.md"), "utf8");
  const changelog = readFileSync(join(repoRoot, "CHANGELOG.md"), "utf8");

  function section(name: string): string {
    const start = new RegExp(`^## ${name}\\r?$`, "m").exec(readme)?.index ?? -1;
    expect(start).toBeGreaterThanOrEqual(0);
    return readme.slice(start).split(/\r?\n## /)[0] ?? "";
  }

  it("AC-7V4B90.1: the status line states the real state", () => {
    const status = /\*\*Status:\*\*([\s\S]*?)\n\n/.exec(readme)?.[1] ?? "";
    expect(status).toMatch(/desktop app/i);
    expect(status).toMatch(/CLI/);
    expect(status).toMatch(/real KM003C/);
    expect(status).toMatch(/KM002C[^.]*untested/i);
    expect(status).toMatch(/not signed|unsigned/i);
    expect(readme).not.toMatch(/Nothing talks to a device yet/i);
    expect(readme).not.toMatch(/^## Planned features/m);
  });

  it("AC-7V4B90.2: Features has every area, one-line items, each with a link where a guide exists", () => {
    const areas = parseFeatureAreas(readme);
    for (const area of [
      "live-meter",
      "recorder-and-charts",
      "pd-analyzer",
      "protocol-triggers",
      "firmware-update",
      "offline-records",
      "meter-settings",
      "serial-stream",
      "several-meters",
      "cli",
      "demo-device-and-simulator",
      "releases",
    ]) {
      expect(areas.get(area)?.length ?? 0, area).toBeGreaterThan(0);
    }
    for (const lines of areas.values()) {
      for (const line of lines) {
        expect(line).not.toContain("\n");
        expect(line).toMatch(/\]\((?:docs\/|#)[^)]+\)/);
      }
    }
  });

  it("AC-7V4B90.3: Coming next names the planned work without dates", () => {
    const next = section("Coming next");
    // Record delete and settings writes shipped in 0.8.0 (JKWT3V, JKWT3W): no longer next.
    expect(next).not.toMatch(/delet\w* offline records|record delete/i);
    expect(next).not.toMatch(/settings/i);
    expect(next).toMatch(/polish/i);
    expect(next).toMatch(/browser edition/i);
    expect(next).not.toMatch(/\b20\d\d\b|\bQ[1-4]\b/);
  });

  it("AC-7V4B90.4: a small desktop screenshot from docs/ is shown", () => {
    const m = /!\[[^\]]*\]\((docs\/[^)\s]+\.(?:png|jpe?g|webp|gif))\)/.exec(readme);
    expect(m).not.toBeNull();
    const size = statSync(join(repoRoot, m?.[1] ?? "missing")).size;
    expect(size).toBeLessThan(500 * 1024);
  });

  it("AC-7V4B90.5: License says the AES keys ship, and nothing shipped is described as future", () => {
    const license = section("License");
    expect(license).toMatch(/ships the AES keys|AES keys[^.]*(?:are|is) (?:shipped|included)/i);
    expect(license).not.toMatch(/will (?:ship|be shipped|include)/i);
    expect(readme).not.toMatch(
      /\bwill (?:ship|support|add|be added)\b|\bnot (?:yet )?implemented\b/i,
    );
  });

  it("AC-7V4B90.6: every CHANGELOG Added feature has a matching README Features line", () => {
    const map = JSON.parse(readFileSync(mapPath, "utf8")) as FeatureMap;
    expect(checkFeatureCoverage(changelog, readme, map)).toEqual([]);
    expect(parseAddedEntries(changelog).length).toBeGreaterThan(30);
  });

  it("AC-7V4B90.6: every mapped feature names its README line as <area-slug>: <phrase>", () => {
    const map = JSON.parse(readFileSync(mapPath, "utf8")) as FeatureMap;
    const plain = Object.entries(map).filter(
      ([, value]) => value !== null && !/^[a-z0-9]+(?:-[a-z0-9]+)*: \S/.test(value),
    );
    expect(plain).toEqual([]);
  });

  it("AC-7V4B90.7: every relative README link resolves to a file in the repository", () => {
    expect(readmeLinks(readme).length).toBeGreaterThan(10);
    expect(checkReadmeLinks(readme, repoRoot)).toEqual([]);
  });

  it("CLI exits 0 on the repository", () => {
    const r = spawnSync("bun", [scriptPath, repoRoot], { encoding: "utf8", timeout: 30_000 });
    expect(r.error).toBeUndefined();
    expect(r.status).toBe(0);
  });
});
