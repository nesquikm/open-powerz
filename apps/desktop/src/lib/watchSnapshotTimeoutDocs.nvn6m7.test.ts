/** FR NVN6M7 AC-NVN6M7.5: the CLI reference's watch section says a missed
 * (timed-out) snapshot is skipped and retried instead of ending the watch. */

import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const read = (path: string) =>
  readFileSync(new URL(`../../../../${path}`, import.meta.url), "utf8");

/** Joins wrapped Markdown lines so a phrase split across lines still matches. */
const flat = (doc: string) => doc.replace(/\s*\n\s*/g, " ");

const sentences = (doc: string): string[] => flat(doc).split(/(?<=[.;])\s+/);

/** The `| \`name\` | ... |` row of the CLI command table. */
const commandRow = (doc: string, name: string): string => {
  const row = doc.split("\n").find((l) => l.startsWith(`| \`${name}\` |`));
  expect(row, `a ${name} row in cli.md`).toBeDefined();
  return row ?? "";
};

describe("the CLI reference describes a missed watch snapshot (AC-NVN6M7.5)", () => {
  const cli = read("docs/reference/cli.md");

  it("nvn6m7: cli.md's watch row says a missed snapshot is skipped and retried", () => {
    const row = commandRow(cli, "watch");
    const hits = sentences(row).filter(
      (s) =>
        /\bsnapshots?\b/i.test(s) &&
        /\b(?:miss(?:es|ed)?|unanswered|times? out|timed[- ]out|time-?outs?)\b/i.test(s) &&
        /\b(?:skip|skips|skipped)\b/i.test(s) &&
        /\b(?:retr(?:y|ies|ied)|tries again|tried again|next refresh)\b/i.test(s),
    );
    expect(
      hits.length,
      "a watch sentence saying a missed snapshot is skipped and retried",
    ).toBeGreaterThan(0);
  });
});
