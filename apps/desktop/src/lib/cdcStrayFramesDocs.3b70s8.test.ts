/** FR 3B70S8 AC-3B70S8.5: the protocol doc says the CDC reader takes a request's
 * reply by its id and skips the stream frames, echoed starts, text lines and other
 * replies it did not ask for, also with no ADCNEW stream started; the CLI reference
 * says `watch --source cdc` falls back to ADC snapshots, with a note on stderr, when
 * the meter keeps echoing the start, while `stream --source cdc` still gives up. */

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

describe("the docs describe the skipped CDC frames and the watch fallback (AC-3B70S8.5)", () => {
  const protocol = read("docs/protocol.md");
  const cli = read("docs/reference/cli.md");

  it("3b70s8: protocol.md says a reply is matched by its request id and stray frames are skipped", () => {
    const hits = sentences(protocol).filter(
      (s) =>
        /\b(?:skip|skips|skipped|drop|drops|dropped)\b/i.test(s) &&
        /\becho/i.test(s) &&
        /\bids?\b/.test(s),
    );
    expect(hits.length, "a sentence on skipping echoes and other ids").toBeGreaterThan(0);
    const text = hits.join(" ");
    expect(text).toMatch(/ADCNEW|stream frame/i);
    expect(text).toMatch(/text line/i);
  });

  it("3b70s8: protocol.md says the skipping holds with no ADCNEW stream started", () => {
    const hits = sentences(protocol).filter(
      (s) =>
        /\becho/i.test(s) &&
        /\b(?:no|without an?|before any)\s+(?:ADCNEW\s+)?stream\b|\bnot started\b|\bnever started\b/i.test(
          s,
        ),
    );
    expect(
      hits.length,
      "a sentence saying it also applies without a started stream",
    ).toBeGreaterThan(0);
  });

  it("3b70s8: cli.md says watch --source cdc falls back to ADC snapshots when the meter keeps echoing", () => {
    const row = commandRow(cli, "watch");
    const hits = sentences(row).filter(
      (s) => /\bfalls? back\b/i.test(s) && /\bsnapshots?\b/i.test(s) && /\becho/i.test(s),
    );
    expect(hits.length, "a watch sentence on the snapshot fallback").toBeGreaterThan(0);
    expect(row).toMatch(/\b(?:stderr|standard error)\b/i);
  });

  it("3b70s8: cli.md keeps stream --source cdc giving up after the echo limit", () => {
    const row = commandRow(cli, "stream");
    expect(row).toMatch(/giving up after 3 echoes|gives up after 3 echoes/i);
    expect(row).not.toMatch(/\bfalls? back\b/i);
  });
});
