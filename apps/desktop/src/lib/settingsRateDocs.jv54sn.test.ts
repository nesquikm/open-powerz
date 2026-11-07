/** The docs record the cause behind the chart that stopped drawing after a
 * settings write (FR JV54SN, AC .6): the protocol doc has an `[inferred]` note
 * that a settings write re-applies the meter's own rate to a running queue,
 * not seen on the device, and says v0.9.0 has the same write path; the FR's
 * root cause is tagged `[inferred]` and says so too. */

import { existsSync, readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const read = (path: string) =>
  readFileSync(new URL(`../../../../${path}`, import.meta.url), "utf8");

/** The FR moves to specs/frs/archive/ when its milestone is archived. */
const readFr = (id: string) => {
  const live = `specs/frs/${id}.md`;
  return existsSync(new URL(`../../../../${live}`, import.meta.url))
    ? read(live)
    : read(`specs/frs/archive/${id}.md`);
};

/** Joins wrapped Markdown lines so a phrase split across lines still matches. */
const flat = (doc: string) => doc.replace(/\s*\n\s*/g, " ");

/** The body of the `## <name>` or `### <name>` section, up to the next heading of that level or higher. */
function section(doc: string, heading: RegExp): string {
  const lines = doc.split("\n");
  const start = lines.findIndex((l) => /^#{2,4} /.test(l) && heading.test(l));
  if (start < 0) return "";
  const level = ((lines[start] ?? "").match(/^#+/) ?? [""])[0].length;
  const end = lines.findIndex(
    (l, i) => i > start && /^#+ /.test(l) && (l.match(/^#+/) ?? [""])[0].length <= level,
  );
  return lines.slice(start, end < 0 ? undefined : end).join("\n");
}

/** Paragraphs and bullets of `text`, lines joined, that carry `[inferred]`. */
function inferredNotes(text: string): string[] {
  return text
    .split(/\n\s*\n|\n(?=- )/)
    .map(flat)
    .filter((p) => p.includes("[inferred]"));
}

const RATE_REAPPLIED =
  /(?:settings write|0x0A)[^.]*(?:re-?appl|resets?|puts? back|goes back|returns?)[^.]*(?:own|stored) (?:sample )?rate[^.]*(?:queue|stream)|(?:queue|stream)[^.]*(?:own|stored) (?:sample )?rate[^.]*(?:settings write|0x0A)/i;

describe("settings write and the queue rate in the docs (AC-JV54SN.6)", () => {
  it("jv54sn: protocol.md has an [inferred] note that a settings write re-applies the meter's own rate to a running queue", () => {
    const doc = read("docs/protocol.md");
    const scope = [section(doc, /Settings write/i), section(doc, /ADC queue streaming/i)].join(
      "\n\n",
    );
    const notes = inferredNotes(scope);
    const hit = notes.find((n) => RATE_REAPPLIED.test(n));
    expect(hit, notes.join("\n---\n")).toBeDefined();
    expect(hit).toMatch(/not (?:yet )?(?:seen|confirmed|observed) on the (?:device|meter)/i);
    expect(hit).toMatch(/v0\.9\.0/);
    expect(hit).toMatch(/same (?:write )?path|same settings write/i);
  });

  it("jv54sn: the FR's root cause is tagged [inferred] and names v0.9.0's write path", () => {
    const fr = readFr("JV54SN");
    const root = flat(section(fr, /Root cause/i));
    expect(root).toContain("[inferred]");
    expect(root).toMatch(/not (?:yet )?(?:seen|confirmed|observed) on the (?:device|meter)/i);
    expect(root).toMatch(/v0\.9\.0/);
  });
});
