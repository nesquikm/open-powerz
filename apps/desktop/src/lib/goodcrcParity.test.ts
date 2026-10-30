/**
 * AC-4PJRVH.7: GoodCRC detection lives in the library and in the UI, so one fixture
 * pins both. This side runs `crates/powerz-protocol/tests/vectors/goodcrc_parity.json`
 * through the UI's refusal; `crates/powerz-protocol/tests/goodcrc_parity.rs` runs the
 * same file through the library's.
 */

import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import type { MessageSop } from "./backend";
import { GOODCRC_REASON, lineRefusal, messageRefusal } from "./console.svelte";

interface Fixture {
  lines: { line: string; verdict: "goodcrc" | "unsure" | "none" }[];
  messages: { sop: MessageSop; msg_type: number; data_objects: number[]; good_crc: boolean }[];
}

const fixture = JSON.parse(
  readFileSync(
    new URL(
      "../../../../crates/powerz-protocol/tests/vectors/goodcrc_parity.json",
      import.meta.url,
    ),
    "utf8",
  ),
) as Fixture;

/** What the UI says about a console line, in the fixture's words. */
function verdict(line: string): Fixture["lines"][number]["verdict"] {
  const reason = lineRefusal(line);
  if (reason === null) return "none";
  return reason === GOODCRC_REASON ? "goodcrc" : "unsure";
}

describe("AC-4PJRVH.7: the UI and the library share one GoodCRC fixture", () => {
  it("the fixture has its lines", () => {
    expect(fixture.lines.length).toBeGreaterThan(40);
    expect(fixture.messages.length).toBeGreaterThan(0);
  });

  it.each(fixture.lines.map((c) => [JSON.stringify(c.line), c.line, c.verdict] as const))(
    "line %s",
    (_name, line, want) => {
      expect(verdict(line)).toBe(want);
    },
  );

  it.each(fixture.messages.map((m) => [JSON.stringify(m), m] as const))(
    "message %s",
    (_name, m) => {
      const { good_crc: want, ...message } = m;
      expect(messageRefusal(message) === GOODCRC_REASON).toBe(want);
    },
  );
});
