/** The docs say when the meter's screen shows a settings write (FR JV54SP, AC .3):
 * the protocol doc records the 2026-11-06 device run (the screen changed only after
 * the value was changed in the meter's menu, one step and back), and neither the
 * user docs nor the backend reference say that opening the setting is enough. */

import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const read = (path: string) =>
  readFileSync(new URL(`../../../../${path}`, import.meta.url), "utf8");

/** Wording that says opening the setting in the menu is enough. */
const OPENING_IS_ENOUGH =
  /open(?:s|ing)? (?:that|the) setting|setting is opened|(?:item|setting)s? (?:was|were|is|are) opened|when (?:its|their) menu items? (?:was|were) opened/i;

/** Joins wrapped Markdown lines so a phrase split across lines still matches. */
const flat = (doc: string) => doc.replace(/\s*\n\s*/g, " ");

/** The `[device]` bullets of protocol.md, one string each, lines joined. */
function deviceBullets(doc: string): string[] {
  return doc
    .split(/\n(?=- )/)
    .filter((b) => b.startsWith("- [device]"))
    .map((b) => flat(b.split(/\n\n/)[0] ?? ""));
}

describe("settings-write hint in the docs (AC-JV54SP.3)", () => {
  it("jv54sp: protocol.md has a [device] note dated 2026-11-06 about changing the value in the menu", () => {
    const notes = deviceBullets(read("docs/protocol.md")).filter((b) => b.includes("2026-11-06"));
    const hit = notes.find(
      (b) => /chang\w* (?:that|the) value/i.test(b) && /one step/i.test(b) && /menu/i.test(b),
    );
    expect(hit, notes.join("\n---\n")).toBeDefined();
    expect(hit).toMatch(/brightness/i);
  });

  it("jv54sp: protocol.md no longer says the apps point to opening the setting", () => {
    const doc = flat(read("docs/protocol.md"));
    // The 2026-10-29 history may stay; only the claim about what the apps say goes.
    expect(doc).not.toMatch(/screen shows the change after that setting\s+is opened/i);
  });

  it("jv54sp: the how-to no longer says opening the setting is enough", () => {
    const doc = flat(read("docs/how-to/read-the-meters-memory.md"));
    expect(doc).not.toMatch(OPENING_IS_ENOUGH);
    expect(doc).toMatch(/chang\w* (?:that|the) value in the meter's (?:own )?menu/i);
  });

  it("jv54sp: the CLI reference settings set row no longer says opening the setting is enough", () => {
    const row =
      read("docs/reference/cli.md")
        .split("\n")
        .find((l) => l.startsWith("| `settings set` |")) ?? "";
    expect(row).not.toBe("");
    expect(row).not.toMatch(OPENING_IS_ENOUGH);
    expect(row).toMatch(/chang\w* (?:that|the) value in (?:its|the meter's) menu/i);
  });

  it("jv54sp: the backend reference no longer says opening the setting is enough", () => {
    const doc = flat(read("docs/reference/desktop-backend.md"));
    expect(doc).not.toMatch(OPENING_IS_ENOUGH);
    expect(doc).toMatch(
      /SAVED_NOTICE[^.]*(?:value is changed|chang\w* (?:that|the) value) in (?:its|the meter's) menu/i,
    );
  });
});
