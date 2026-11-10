/** FR NVN6M8 AC-NVN6M8.5: the use-several-meters and record-a-csv pages and the CLI
 * reference's watch row say that a CLI polling the meter over CDC (the serial port)
 * pauses the app's queue stream. */

import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const read = (path: string) =>
  readFileSync(new URL(`../../../../${path}`, import.meta.url), "utf8");

/** Joins wrapped Markdown lines so a phrase split across lines still matches. */
const flat = (doc: string) => doc.replace(/\s*\n\s*/g, " ");

const sentences = (doc: string): string[] => flat(doc).split(/(?<=[.;:])\s+/);

/** The `| \`name\` | ... |` row of the CLI command table. */
const commandRow = (doc: string, name: string): string => {
  const row = doc.split("\n").find((l) => l.startsWith(`| \`${name}\` |`));
  expect(row, `a ${name} row in cli.md`).toBeDefined();
  return row ?? "";
};

/** Sentences that tie a CLI polling over CDC / the serial port to the app's queue
 * stream pausing (stopping, freezing). */
const pauseSentences = (text: string): string[] =>
  sentences(text).filter(
    (s) =>
      /\b(?:CLI|powerz watch|`powerz watch`|`watch`|watch)\b/i.test(s) &&
      /\b(?:CDC|serial port)\b/i.test(s) &&
      /\bqueue\b/i.test(s) &&
      /\b(?:paus(?:e|es|ed|ing)|stops?|stopped|freez(?:e|es)|halts?)\b/i.test(s) &&
      /\bapp\b|\bdesktop\b|\bweb edition\b/i.test(s),
  );

describe("the docs say a CLI polling over CDC pauses the app's queue stream (AC-NVN6M8.5)", () => {
  for (const page of ["docs/how-to/use-several-meters.md", "docs/how-to/record-a-csv.md"]) {
    it(`nvn6m8: ${page} says it`, () => {
      const hits = pauseSentences(read(page));
      expect(hits.length, `a sentence in ${page}`).toBeGreaterThan(0);
    });
  }

  it("nvn6m8: the CLI reference's watch row says it", () => {
    const row = commandRow(read("docs/reference/cli.md"), "watch");
    expect(pauseSentences(row).length, "a sentence in the watch row").toBeGreaterThan(0);
  });
});
