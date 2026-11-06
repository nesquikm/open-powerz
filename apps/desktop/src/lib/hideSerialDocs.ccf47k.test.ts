/** The user docs and the CLI reference describe the hide-serial wording (FR CCF47K, AC .7):
 * labels name the model alone, same-model meters get a number, and a shown value reads
 * "***". Neither says a serial reads "Hidden" or prints `hidden` any more. */

import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const read = (path: string) =>
  readFileSync(new URL(`../../../../${path}`, import.meta.url), "utf8");

/** The section of `doc` under the heading that contains `title`. */
function section(doc: string, title: string): string {
  const parts = doc.split(/^#+ /m);
  return parts.find((p) => (p.split("\n", 1)[0] ?? "").includes(title)) ?? "";
}

describe("hide-serial wording in the docs (AC-CCF47K.7)", () => {
  const guide = section(read("docs/how-to/explore-a-session.md"), "Hide serial numbers");
  const cli = section(read("docs/reference/cli.md"), "Hide serial numbers");

  it("ccf47k: the how-to has its Hide serial numbers section", () => {
    expect(guide).not.toBe("");
  });

  it("ccf47k: the how-to says a shown value reads ***", () => {
    expect(guide).toContain("***");
    expect(guide).not.toMatch(/"Hidden"/);
  });

  it("ccf47k: the how-to says labels name the model alone", () => {
    expect(guide).toMatch(/model alone|only the model|just the model/i);
    expect(guide).toContain('"KM003C"');
  });

  it("ccf47k: the how-to says same-model meters get a number", () => {
    expect(guide).toContain("KM003C (1)");
    expect(guide).toContain("KM003C (2)");
  });

  it("ccf47k: the CLI reference says both commands print ***", () => {
    expect(cli).not.toBe("");
    expect(cli).toMatch(/powerz list --hide-serial`[^\n]*`\*\*\*`/);
    expect(cli).toMatch(/powerz info --hide-serial`[^\n]*`\*\*\*`/);
    expect(cli).not.toMatch(/prints `hidden`/);
  });

  it("ccf47k: the CLI reference table no longer lists hidden for the flag", () => {
    expect(read("docs/reference/cli.md")).not.toMatch(/prints? `hidden`/);
  });
});
