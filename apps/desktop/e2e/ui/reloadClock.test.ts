/**
 * FR 3B70S3: the live-window reload test (reload.spec.ts, AC-VEDDFD.6) waited ten
 * seconds of real time for six seconds of demo data. The mock demo meter makes its
 * samples per timer tick, so a slow runner got less data in the same real time.
 *
 * AC-3B70S3.1: the test holds the page clock and steps it until the statistics clock
 * reads at least 6 s; no wait in it depends on how fast the runner's wall clock moves.
 * AC-3B70S3.2: after the reload, each window (10 s, 1 min, 10 min, then 10 s again)
 * must show the backfilled history within 1.5 s of page time, stepped on the held
 * clock, with the same window checks as before.
 * AC-3B70S3.3 (20 runs in a row on WebKit and Chromium) is checked by repeated
 * Playwright runs, not here.
 */

import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const SOURCE = readFileSync(new URL("reload.spec.ts", import.meta.url), "utf8");
const LINES = SOURCE.split(/\r?\n/);

const TITLE = "straight after a reload the live windows show the samples from before it";

/** The text from `startLine` to the first later line that closes it at column 0. */
function blockFrom(startLine: number): string {
  const end = LINES.findIndex((l, i) => i > startLine && /^}\)?;?\s*$/.test(l));
  return LINES.slice(startLine, end < 0 ? undefined : end + 1).join("\n");
}

/** The body of the AC-VEDDFD.6 test. */
function testBody(): string {
  const start = LINES.findIndex((l) => /^test\(/.test(l) && l.includes(TITLE));
  if (start < 0) throw new Error(`no test named "${TITLE}" in reload.spec.ts`);
  return blockFrom(start);
}

/** A top-level function's whole text, or null when the spec has none of that name. */
function functionBody(name: string): string | null {
  const start = LINES.findIndex((l) =>
    new RegExp(`^(export\\s+)?(async\\s+)?function\\s+${name}\\b`).test(l),
  );
  return start < 0 ? null : blockFrom(start);
}

/** Each `stepHeldClock(` call in `text`, up to its matching closing parenthesis. */
function stepCalls(text: string): string[] {
  const out: string[] = [];
  const re = /\bstepHeldClock\(/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text)) !== null) {
    let depth = 0;
    let i = m.index + "stepHeldClock".length;
    for (; i < text.length; i++) {
      if (text[i] === "(") depth++;
      else if (text[i] === ")" && --depth === 0) break;
    }
    out.push(text.slice(m.index, i + 1));
  }
  return out;
}

/** A top-level `const NAME = <value>;` in the spec, or null. */
function constValue(name: string): string | null {
  const m = new RegExp(`^const\\s+${name}\\b[^=]*=\\s*([^;]+);`, "m").exec(SOURCE);
  return m?.[1]?.trim() ?? null;
}

/** Does `arg` stand for exactly 1.5 s of page time (a literal or a plain constant)? */
function isPageTime1500(arg: string): boolean {
  const a = arg.trim();
  if (/^1_?500$/.test(a)) return true;
  if (/^[A-Z_][A-Z0-9_]*$/.test(a)) {
    const v = constValue(a);
    return v !== null && /^1_?500$/.test(v);
  }
  return false;
}

/** The last top-level argument of a call's text. */
function lastArg(call: string): string {
  const inner = call.slice(call.indexOf("(") + 1, -1);
  let depth = 0;
  let cut = 0;
  for (let i = 0; i < inner.length; i++) {
    const c = inner[i];
    if (c === "(" || c === "[" || c === "{") depth++;
    else if (c === ")" || c === "]" || c === "}") depth--;
    else if (c === "," && depth === 0 && inner.slice(i + 1).trim() !== "") cut = i + 1;
  }
  return inner.slice(cut).replace(/,\s*$/, "").trim();
}

/** The live-window checks in the test body: lines naming a `LIVE_WINDOW_MS[...]` window. */
function windowCheckLines(body: string): string[] {
  return body.split("\n").filter((l) => /LIVE_WINDOW_MS\[/.test(l));
}

/** The function a window-check line calls (`await <name>(page, ...)`). */
function calledName(line: string): string | null {
  return /await\s+([A-Za-z_$][\w$]*)\(/.exec(line)?.[1] ?? null;
}

describe("FR 3B70S3: the live-window reload test runs on the held page clock", () => {
  it("AC-3B70S3.1: holds the clock before the 6 s wait", () => {
    const body = testBody();
    const hold = body.search(/\bholdClock\(\s*page\s*\)/);
    expect(hold, "the test never calls holdClock(page)").toBeGreaterThanOrEqual(0);
    const sixWait = stepCalls(body).find((c) => /statsElapsedSeconds/.test(c));
    expect(sixWait, "no stepHeldClock(...) waits on statsElapsedSeconds").toBeDefined();
    expect(hold).toBeLessThan(body.indexOf(sixWait ?? "\u0000"));
  });

  it("AC-3B70S3.1: steps the held clock until the stats clock reads at least 6 s", () => {
    const sixWait = stepCalls(testBody()).find((c) => /statsElapsedSeconds/.test(c)) ?? "";
    expect(sixWait).toMatch(/statsElapsedSeconds\(\s*page\s*\)\)?\s*>=\s*6\b/);
  });

  it("AC-3B70S3.1: no wall-clock poll on the stats clock and no wall-clock budget", () => {
    const body = testBody();
    expect(body).not.toMatch(/expect\s*\.poll\(\s*\(\)\s*=>\s*statsElapsedSeconds/);
    expect(body).not.toMatch(/\bexpect\s*\.poll\(/);
    expect(body).not.toMatch(/\bciLatencyBudgetMs\b/);
    expect(body).not.toMatch(/\bBACKFILL_BUDGET_MS\b/);
    expect(body).not.toMatch(/waitForTimeout\(/);
  });

  it("AC-3B70S3.2: four window checks, 10 s, 1 min, 10 min, then 10 s again", () => {
    const body = testBody();
    const checks = windowCheckLines(body);
    // The 1 min / 10 min checks run in a loop over their names, as before, or one each.
    const named = body.match(/LIVE_WINDOW_MS\["(10 s|1 min|10 min)"\]/g) ?? [];
    const loop = /for\s*\(\s*const\s+\w+\s+of\s+\["1 min",\s*"10 min"\]\s*\)/.test(body);
    if (loop) {
      expect(named).toEqual(['LIVE_WINDOW_MS["10 s"]', 'LIVE_WINDOW_MS["10 s"]']);
      expect(checks.length).toBe(3);
    } else {
      expect(named).toEqual([
        'LIVE_WINDOW_MS["10 s"]',
        'LIVE_WINDOW_MS["1 min"]',
        'LIVE_WINDOW_MS["10 min"]',
        'LIVE_WINDOW_MS["10 s"]',
      ]);
    }
  });

  it("AC-3B70S3.2: each window check steps the held clock with a 1.5 s page-time limit", () => {
    const checks = windowCheckLines(testBody());
    expect(checks.length).toBeGreaterThan(0);
    for (const line of checks) {
      expect(line, `window check uses a wall-clock budget: ${line.trim()}`).not.toMatch(
        /BACKFILL_BUDGET_MS|ciLatencyBudgetMs/,
      );
      const name = calledName(line);
      if (name === "stepHeldClock") {
        expect(isPageTime1500(lastArg(line.slice(line.indexOf("stepHeldClock("))))).toBe(true);
        continue;
      }
      expect(name, `no awaited call on: ${line.trim()}`).not.toBeNull();
      const fn = functionBody(name ?? "");
      expect(fn, `window check calls ${name}, which reload.spec.ts does not define`).not.toBeNull();
      const helper = fn ?? "";
      // The helper waits on the held clock, never on a wall-clock poll.
      expect(helper, `${name} does not step the held clock`).toMatch(/\bstepHeldClock\(\s*page\b/);
      expect(helper, `${name} still polls on wall-clock time`).not.toMatch(/\bexpect\s*\.poll\(/);
      // Its page-time limit is 1.5 s: given at the call site or fixed in the helper.
      const limits = stepCalls(helper).map(lastArg);
      const fixed = limits.some(isPageTime1500);
      const callLimit = lastArg(
        line
          .trim()
          .replace(/;$/, "")
          .replace(/^await\s+/, ""),
      );
      expect(
        fixed || isPageTime1500(callLimit),
        `${name}'s stepHeldClock limit is not 1.5 s of page time (call: ${line.trim()})`,
      ).toBe(true);
    }
  });

  it("AC-3B70S3.2: the window checks keep the same state checks as before", () => {
    const checks = windowCheckLines(testBody());
    const names = new Set(checks.map(calledName).filter((n): n is string => n !== null));
    names.delete("stepHeldClock");
    // Whatever runs the check, the spec still asks liveWindowProblem and the zoom readout.
    const texts = [...names].map((n) => functionBody(n) ?? "").join("\n") + testBody();
    expect(texts).toMatch(/\bliveWindowProblem\(/);
    expect(texts).toMatch(/formatSpan\(\s*draw\.t1Ms\s*-\s*draw\.t0Ms\s*\)/);
    expect(texts).toMatch(/reachBackMs/);
  });

  it("the 1.5 s budget is no longer scaled by the CI factor", () => {
    expect(SOURCE).not.toMatch(/ciLatencyBudgetMs\(\s*1_?500\s*\)/);
  });
});
