// AC-CW66T1.11: a README line, the readme-features map and a how-to section describe the
// info popovers and the "Show (i) icons" switch.
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

const POPOVER = /\(i\)|info popover|info button/i;

describe("the info popovers are documented (AC-CW66T1.11)", () => {
  it("has one README Features line on the popovers and the switch", () => {
    const bullets = [...parseFeatureAreas(readme).values()].flat();
    const lines = bullets.filter((b) => POPOVER.test(b));
    expect(lines).toHaveLength(1);
    expect(lines[0]).toMatch(/abbreviation|term/i);
  });

  it("the README gate maps the FR to that line", () => {
    const value = map.CW66T1;
    expect(value).toEqual(expect.stringMatching(/^[a-z0-9-]+: .+$/i));
    const [area = "", phrase = ""] = (value ?? "").split(": ");
    const bullets = parseFeatureAreas(readme).get(area) ?? [];
    expect(bullets.some((b) => POPOVER.test(b) && b.toLowerCase().includes(phrase))).toBe(true);
  });

  it("a how-to section describes the popovers and the Show (i) icons switch", () => {
    const dir = join(repoRoot, "docs/how-to");
    const guides = readdirSync(dir)
      .filter((f) => f.endsWith(".md"))
      .map((f) => readFileSync(join(dir, f), "utf8"));
    const withSwitch = guides.filter((g) => g.includes("Show (i) icons"));
    expect(withSwitch.length).toBeGreaterThan(0);
    expect(
      withSwitch.some((g) => /^#{2,3} .*(\(i\)|info|popover|abbreviation)/im.test(g)),
      "a heading names the popovers",
    ).toBe(true);
    expect(withSwitch.some((g) => /dotted/i.test(g) && /Escape/.test(g))).toBe(true);
  });
});
