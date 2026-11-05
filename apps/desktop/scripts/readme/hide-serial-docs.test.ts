// AC-AXHK2Q.7: a README line and the docs describe the "Hide serial numbers" option,
// and the README gate maps the FR to that line.
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import type { FeatureMap } from "./readme-features";
import { parseFeatureAreas } from "./readme-features";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, "../../../..");
const readme = readFileSync(join(repoRoot, "README.md"), "utf8");
const map = JSON.parse(readFileSync(join(here, "readme-features.json"), "utf8")) as FeatureMap;

describe("the Hide serial numbers option is documented (AC-AXHK2Q.7)", () => {
  it("has a README Features line", () => {
    const bullets = [...parseFeatureAreas(readme).values()].flat();
    expect(bullets.filter((b) => /hide serial numbers/i.test(b))).toHaveLength(1);
  });

  it("the README gate maps the FR to that line", () => {
    const value = map.AXHK2Q;
    expect(value).toEqual(expect.stringMatching(/^[a-z0-9-]+: hide serial numbers$/i));
    const area = (value ?? "").split(": ")[0] ?? "";
    const bullets = parseFeatureAreas(readme).get(area) ?? [];
    expect(bullets.some((b) => /hide serial numbers/i.test(b))).toBe(true);
  });

  it("the CLI reference gives --hide-serial for list and info", () => {
    const cli = readFileSync(join(repoRoot, "docs/reference/cli.md"), "utf8");
    expect(cli).toMatch(/powerz list[^\n]*--hide-serial|--hide-serial[^\n]*`?powerz list/);
    expect(cli).toMatch(/powerz info[^\n]*--hide-serial|--hide-serial[^\n]*`?powerz info/);
  });

  it("a how-to guide describes the desktop switch", () => {
    const dir = join(repoRoot, "docs/how-to");
    const guides = readdirSync(dir)
      .filter((f) => f.endsWith(".md"))
      .map((f) => readFileSync(join(dir, f), "utf8"));
    expect(guides.some((g) => g.includes("Hide serial numbers"))).toBe(true);
  });
});
