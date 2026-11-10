/** FR 3B70S7 AC-3B70S7.4: the user docs on exports say that rows before the first
 * valid temperature reading have no TEMP, in both editions: the desktop's recording
 * and export how-to, and the web edition's page. */

import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const read = (path: string) =>
  readFileSync(new URL(`../../../../${path}`, import.meta.url), "utf8");

/** Joins wrapped Markdown lines so a phrase split across lines still matches. */
const flat = (doc: string) => doc.replace(/\s*\n\s*/g, " ");

/** A sentence that says rows before the first valid temperature reading have no TEMP. */
const RULE = /before the (?:meter's )?first valid temperature reading/i;

/** The sentences of `doc` that state the rule and name TEMP. */
const ruleSentences = (doc: string): string[] =>
  flat(doc)
    .split(/(?<=[.;:])\s+/)
    .filter((s) => RULE.test(s) && /\bTEMP\b/.test(s));

describe("the docs state the first-reading TEMP rule (AC-3B70S7.4)", () => {
  it("3b70s7: record-a-csv.md says rows before the first valid temperature reading have no TEMP", () => {
    const hits = ruleSentences(read("docs/how-to/record-a-csv.md"));
    expect(hits.length, "a sentence naming TEMP and the first valid reading").toBeGreaterThan(0);
    expect(hits.join(" ")).toMatch(/\b(?:no|empty)\b/i);
  });

  it("3b70s7: web-edition.md says rows before the first valid temperature reading have no TEMP", () => {
    const hits = ruleSentences(read("docs/how-to/web-edition.md"));
    expect(hits.length, "a sentence naming TEMP and the first valid reading").toBeGreaterThan(0);
    expect(hits.join(" ")).toMatch(/\b(?:no|empty)\b/i);
  });
});
