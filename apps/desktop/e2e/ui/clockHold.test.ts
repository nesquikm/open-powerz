/**
 * FR FFQVBQ: the logo write tests hold the page clock mid-write, then must step it in
 * bounded slices until the write's end shows. Resuming it in real time first makes the
 * fake clock replay two demo meters' stream batches and chart frames on every tick; on a
 * slow runner each catch-up outgrows the last and the write never ends.
 *
 * AC-FFQVBQ.1: every held-clock write test steps the clock before waiting for the end.
 * AC-FFQVBQ.2: `helpers.ts` exports `stepHeldClock(page, until, limitMs?)`, built on
 * `page.clock.runFor`, that stops once `until` holds and fails clearly when it never does.
 * AC-FFQVBQ.3: this check fails when a spec waits for a write's end right after
 * `page.clock.resume()`.
 */

import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const read = (name: string): string => readFileSync(new URL(name, import.meta.url), "utf8");

const SPECS = ["logo.spec.ts", "logo-hold.spec.ts"] as const;

/** At least this many held-clock end waits per spec (the sites FR FFQVBQ names). */
const MIN_SITES: Record<(typeof SPECS)[number], number> = {
  "logo.spec.ts": 2,
  "logo-hold.spec.ts": 4,
};

/** A wait for a logo write's end. */
const END_WAIT = (line: string): boolean =>
  /timeout:\s*30_000/.test(line) &&
  (/toContainText\(\s*\/verified\/i/.test(line) ||
    /toContainText\(\s*ending\b/.test(line) ||
    /"logo-menu-save"\)\)\.toBeVisible\(/.test(line) ||
    /"Add meter"\)\)\.toBeEnabled\(/.test(line));

const HOLD = /\b(holdClock|holdWrite)\(page\)/;
const RESUME = /page\.clock\.resume\(\)/;
const STEP = /\bstepHeldClock\(\s*page\b/;
const BLOCK_START = /^\s*(test\(|test\.describe\(|(export\s+)?async\s+function\s|function\s)/;

interface Block {
  /** 1-based line of the block's first line. */
  start: number;
  lines: string[];
}

/** The spec split at each `test(` and top-level function. */
function blocks(lines: string[]): Block[] {
  const out: Block[] = [];
  lines.forEach((line, i) => {
    if (BLOCK_START.test(line) || out.length === 0) out.push({ start: i + 1, lines: [] });
    out[out.length - 1]?.lines.push(line);
  });
  return out;
}

interface Site {
  file: string;
  line: number;
  problem: string | null;
}

/** Every held-clock end wait in a spec, with what is wrong before it (if anything). */
function sites(file: string): Site[] {
  const out: Site[] = [];
  for (const b of blocks(read(file).split(/\r?\n/))) {
    const isFinish = /async\s+function\s+finishWrite\b/.test(b.lines[0] ?? "");
    let hold = isFinish ? 0 : b.lines.findIndex((l) => HOLD.test(l));
    if (hold < 0) continue;
    // holdWrite's own body holds the clock but waits for no end.
    if (/async\s+function\s+holdWrite\b/.test(b.lines[0] ?? "")) continue;
    for (let e = hold + 1; e < b.lines.length; e++) {
      if (!END_WAIT(b.lines[e] ?? "")) continue;
      const between = b.lines.slice(hold + 1, e);
      const step = between.findIndex((l) => STEP.test(l));
      const resume = between.findIndex((l) => RESUME.test(l));
      let problem: string | null = null;
      if (resume >= 0 && (step < 0 || resume < step))
        problem = `waits for the write's end after page.clock.resume() (line ${b.start + hold + 1 + resume}) without stepping the held clock first`;
      else if (step < 0) problem = "waits for the write's end without stepHeldClock";
      out.push({ file, line: b.start + e, problem });
      hold = e;
    }
  }
  return out;
}

describe("logo write tests", () => {
  for (const file of SPECS) {
    it(`${file}: finds every held-clock end wait`, () => {
      expect(sites(file).length).toBeGreaterThanOrEqual(MIN_SITES[file]);
    });

    it(`${file}: steps the held clock before each write-end wait, never resumes first (AC-FFQVBQ.1, AC-FFQVBQ.3)`, () => {
      const bad = sites(file)
        .filter((s) => s.problem !== null)
        .map((s) => `${s.file}:${s.line}: ${s.problem}`);
      expect(bad).toEqual([]);
    });
  }

  it("helpers.ts exports stepHeldClock(page, until, limitMs?) built on page.clock.runFor (AC-FFQVBQ.2)", () => {
    const lines = read("helpers.ts").split(/\r?\n/);
    const start = lines.findIndex((l) =>
      /^export\s+async\s+function\s+stepHeldClock\(\s*page\s*:\s*Page\s*,\s*until\b/.test(l),
    );
    expect(
      start,
      "export async function stepHeldClock(page: Page, until, ...)",
    ).toBeGreaterThanOrEqual(0);
    const rest = lines.slice(start);
    const end = rest.findIndex((l, i) => i > 0 && /^\}/.test(l));
    const body = rest.slice(0, end < 0 ? undefined : end + 1).join("\n");
    expect(body, "the signature takes an optional page-time limit").toMatch(
      /limitMs\s*(\?\s*:|:[^,)]*=|=)/,
    );
    expect(body, "it advances the held clock with page.clock.runFor").toMatch(
      /page\.clock\.runFor\(/,
    );
    expect(body, "it checks the condition between steps").toMatch(/\buntil\(/);
    expect(body, "it fails with a clear message when the condition never holds").toMatch(
      /throw\s+new\s+Error\(|expect[\s\S]*toBe\(\s*true|\.fail\(/,
    );
  });
});
