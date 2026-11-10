/**
 * FR 3B70S4: the zoom readout test (timeline.spec.ts, AC-FENCD9.9 "a visible readout
 * beside the zoom slider shows the span, with a tooltip") read the readout once in the
 * whole-session view, then waited for the slider to say that old value. The span keeps
 * growing while the demo streams, so a slow runner never saw them match.
 *
 * AC-3B70S4.1: after "Whole session" the test reads the readout and the slider's
 * aria-valuetext in one page evaluation, retried until the two agree, and still checks
 * the minutes-and-seconds shape.
 * AC-3B70S4.2: the earlier checks (10 s readout, shared tooltip, position) stay.
 * AC-3B70S4.3 (20 runs in a row on WebKit and Chromium) is checked by repeated
 * Playwright runs, not here.
 */

import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const SOURCE = readFileSync(new URL("timeline.spec.ts", import.meta.url), "utf8");
const LINES = SOURCE.split(/\r?\n/);

const TITLE =
  "AC-FENCD9.9: a visible readout beside the zoom slider shows the span, with a tooltip";

/** The body of that one test: from its `test(` line to the `});` at the same indent. */
function testBody(): string {
  const start = LINES.findIndex((l) => /^\s*test\(/.test(l) && l.includes(TITLE));
  if (start < 0) throw new Error(`no test named "${TITLE}" in timeline.spec.ts`);
  const indent = /^\s*/.exec(LINES[start] ?? "")?.[0] ?? "";
  const end = LINES.findIndex((l, i) => i > start && l === `${indent}});`);
  return LINES.slice(start, end < 0 ? undefined : end + 1).join("\n");
}

/** The part of the body after the "Whole session" click. */
function afterWholeSession(): string {
  const body = testBody();
  const at = body.search(
    /getByRole\(\s*"button",\s*\{\s*name:\s*"Whole session"\s*\}\s*\)\.click\(/,
  );
  if (at < 0) throw new Error("the test no longer clicks Whole session");
  return body.slice(at);
}

/** The text of each call starting at `re`'s match, up to its matching closing parenthesis. */
function callsAt(text: string, re: RegExp): string[] {
  const out: string[] = [];
  const g = new RegExp(re.source, "g");
  let m: RegExpExecArray | null;
  while ((m = g.exec(text)) !== null) {
    let i = m.index + m[0].length - 1; // at the "("
    let depth = 0;
    for (; i < text.length; i++) {
      if (text[i] === "(") depth++;
      else if (text[i] === ")" && --depth === 0) break;
    }
    out.push(text.slice(m.index, i + 1));
  }
  return out;
}

/** Does an evaluate call read both the readout's text and the slider's aria-valuetext? */
function readsBoth(call: string): boolean {
  const readout = /zoom-span/.test(call) || /^readout\.evaluate\(/.test(call);
  return readout && /aria-valuetext/.test(call);
}

/** The retrying waits after the click: `expect.poll(...)` calls and `expect(...).toPass(` blocks. */
function retryingWaits(text: string): string[] {
  const polls = callsAt(text, /\bexpect\s*\.poll\(/);
  const toPass = callsAt(text, /\bexpect\(\s*async\s*\(\)\s*=>\s*\{/).filter((c) => {
    const rest = text.slice(text.indexOf(c) + c.length);
    return /^\s*\.toPass\(/.test(rest);
  });
  return [...polls, ...toPass];
}

describe("FR 3B70S4: the zoom readout test compares readout and slider at one moment", () => {
  it("AC-3B70S4.1: no wait on aria-valuetext against a readout value read earlier", () => {
    const tail = afterWholeSession();
    expect(tail).not.toMatch(
      /toHaveAttribute\(\s*"aria-valuetext"[^;]*\.(textContent|innerText)\(/s,
    );
    // No readout text captured on its own and compared later.
    expect(tail).not.toMatch(/\breadout\.(textContent|innerText|innerHTML)\(/);
    expect(tail).not.toMatch(/\bslider\.getAttribute\(\s*"aria-valuetext"/);
  });

  it("AC-3B70S4.1: one evaluation reads both the readout text and the slider's aria-valuetext", () => {
    const evals = callsAt(afterWholeSession(), /\b(?:page|readout|slider)\.evaluate\(/);
    const both = evals.filter(readsBoth);
    expect(
      both.length,
      "no single evaluate(...) reads .zoom-span and aria-valuetext",
    ).toBeGreaterThan(0);
  });

  it("AC-3B70S4.1: that evaluation is retried until the two agree", () => {
    const waits = retryingWaits(afterWholeSession());
    const wait = waits.find((w) =>
      callsAt(w, /\b(?:page|readout|slider)\.evaluate\(/).some(readsBoth),
    );
    expect(
      wait,
      "the joint evaluate is not inside expect.poll(...) or expect(...).toPass()",
    ).toBeDefined();
    // The two strings are compared for equality, inside the evaluation or by the matcher.
    const tail = afterWholeSession();
    const compares =
      /===|==/.test(wait ?? "") ||
      /expect\.poll\([\s\S]*?\)\s*\.(toBe|toEqual|toStrictEqual)\(/.test(tail) ||
      /expect\([\s\S]*?\)\.(toBe|toEqual|toStrictEqual)\(/.test(wait ?? "");
    expect(compares, "the readout and aria-valuetext are not compared for equality").toBe(true);
  });

  it("AC-3B70S4.1: the minutes-and-seconds shape is still checked after Whole session", () => {
    expect(afterWholeSession()).toContain("/^\\d+:\\d{2}$/");
  });

  it("AC-3B70S4.2: the 10 s readout, the shared tooltip and the position checks stay", () => {
    const body = testBody();
    const before = body.slice(0, body.length - afterWholeSession().length);
    expect(before).toMatch(/await expect\(readout\)\.toBeVisible\(\)/);
    expect(before).toMatch(/await expect\(readout\)\.toHaveText\("10 s"\)/);
    expect(before).toMatch(
      /await expect\(slider\)\.toHaveAttribute\("title", "Zoom: visible span"\)/,
    );
    expect(before).toMatch(
      /await expect\(readout\)\.toHaveAttribute\("title", "Zoom: visible span"\)/,
    );
    expect(before).toMatch(/await readout\.boundingBox\(\)/);
    expect(before).toMatch(/await slider\.boundingBox\(\)/);
    expect(before).toMatch(
      /Math\.abs\(r\.y \+ r\.height \/ 2 - \(sl\.y \+ sl\.height \/ 2\)\)\)\.toBeLessThan\(sl\.height\)/,
    );
    expect(before).toMatch(
      /Math\.min\(Math\.abs\(r\.x - \(sl\.x \+ sl\.width\)\), Math\.abs\(sl\.x - \(r\.x \+ r\.width\)\)\),?\s*\)\.toBeLessThan\(24\)/,
    );
  });
});
