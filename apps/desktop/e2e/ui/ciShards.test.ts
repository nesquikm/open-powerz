/**
 * FR 9QTPNR: the UI suite runs in shards that fit CI's clock, the frame-rate and memory
 * tests run once per OS with one worker, and a cancelled job still keeps its report.
 *
 * AC-9QTPNR.1: the e2e-ui job is four Playwright shards per OS.
 * AC-9QTPNR.2: the @perf tests and the WebKit memory test run once per OS, outside the
 * shards, each with one worker; the shards leave @perf out.
 * AC-9QTPNR.3: every UI job uploads its report on failure or cancellation, named apart.
 * AC-9QTPNR.6: Windows runs only the chromium project (the app's WebView2 is Chromium);
 * macOS and Linux run both.
 */

import { readFileSync, readdirSync } from "node:fs";
import { describe, expect, it } from "vitest";

const WORKFLOW = readFileSync(
  new URL("../../../../.github/workflows/ci.yml", import.meta.url),
  "utf8",
);
const LINES = WORKFLOW.split(/\r?\n/);
const SHARDS = 4;
/** The frame-rate tests' tag, as helpers.ts declares it (read as text: helpers.ts loads
 * Playwright, which does not run under Vitest). */
const PERF_TAG = /export const PERF_TAG = "(@[\w-]+)";/.exec(
  readFileSync(new URL("./helpers.ts", import.meta.url), "utf8"),
)?.[1];
const OSES = ["ubuntu-24.04", "macos-latest", "windows-latest"];
/** AC-9QTPNR.6: Windows runs the engine the app ships there, WebView2 (Chromium). */
const WINDOWS_CHROMIUM_ONLY = "${{ runner.os == 'Windows' && '--project=chromium' || '' }}";

/** The lines of the top-level job `name` (two-space indent), up to the next job. */
function job(name: string): string[] {
  const start = LINES.findIndex((l) => l === `  ${name}:`);
  if (start < 0) throw new Error(`no job ${name}`);
  const rest = LINES.slice(start + 1);
  const end = rest.findIndex((l) => /^ {2}[\w-]+:\s*$/.test(l));
  return end < 0 ? rest : rest.slice(0, end);
}

/** The `run:` line of the step named `name` in `lines`, trimmed. */
function runOf(lines: string[], name: string): string {
  const at = lines.findIndex((l) => l.trim() === `- name: ${name}`);
  if (at < 0) throw new Error(`no step ${name}`);
  const run = lines.slice(at + 1).find((l) => /^\s+run:/.test(l));
  return (run ?? "").trim();
}

/** The `if:` line of the step named `name` in `lines`, or "" when it has none. */
function ifOf(lines: string[], name: string): string {
  const at = lines.findIndex((l) => l.trim() === `- name: ${name}`);
  const body: string[] = [];
  for (const l of lines.slice(at + 1)) {
    if (/^\s+- /.test(l)) break;
    body.push(l);
  }
  return (body.find((l) => /^\s+if:/.test(l)) ?? "").trim();
}

function matrixList(lines: string[], key: string): string[] {
  const line = lines.find((l) => new RegExp(`^\\s+${key}:\\s*\\[`).test(l)) ?? "";
  const m = /\[(.*)\]/.exec(line);
  return m ? m[1]!.split(",").map((s) => s.trim()) : [];
}

it("finds the frame-rate tag in helpers.ts", () => {
  expect(PERF_TAG).toBe("@perf");
});

describe("CI: the UI suite in shards (AC-9QTPNR.1)", () => {
  const shards = job("e2e-ui");

  it("runs four shards on each of the three OSes, each within 30 minutes", () => {
    expect(matrixList(shards, "os")).toEqual(OSES);
    expect(matrixList(shards, "shard")).toEqual(["1", "2", "3", "4"]);
    expect(shards).toContain("    timeout-minutes: 30");
  });

  it("hands each job its own shard and leaves the frame-rate tests out", () => {
    const run = runOf(shards, "UI e2e");
    expect(run).toContain(`--shard=\${{ matrix.shard }}/${SHARDS}`);
    expect(run).toContain(`--grep-invert ${PERF_TAG}`);
  });

  it("runs only the chromium project on Windows, both projects elsewhere (AC-9QTPNR.6)", () => {
    expect(runOf(shards, "UI e2e")).toContain(WINDOWS_CHROMIUM_ONLY);
  });

  it("names each shard's job by OS and shard", () => {
    expect(shards.find((l) => /^\s+name:/.test(l))?.trim()).toBe(
      `name: e2e-ui (\${{ matrix.os }}, \${{ matrix.shard }}/${SHARDS})`,
    );
  });
});

describe("CI: the frame-rate and memory tests once per OS (AC-9QTPNR.2)", () => {
  const perf = job("e2e-ui-perf");

  it("is one job per OS, with no shard axis", () => {
    expect(matrixList(perf, "os")).toEqual(OSES);
    expect(matrixList(perf, "shard")).toEqual([]);
  });

  it("runs the @perf tests alone with one worker, Chromium only on Windows", () => {
    expect(runOf(perf, "UI frame rate")).toBe(
      `run: bun run e2e:ui --grep ${PERF_TAG} --workers=1 ${WINDOWS_CHROMIUM_ONLY}`,
    );
  });

  it("runs the WebKit memory test with one worker, not on Windows, and only there", () => {
    expect(runOf(perf, "UI memory (WebKit)")).toBe(
      "run: bun run e2e:ui memory.spec.ts --project=webkit --workers=1",
    );
    expect(ifOf(perf, "UI memory (WebKit)")).toContain("runner.os != 'Windows'");
    expect(job("e2e-ui").join("\n")).not.toContain("memory.spec.ts");
  });

  it("tags every frame-rate test, and only those, with @perf", () => {
    const dir = new URL("./", import.meta.url);
    const tagged = readdirSync(dir)
      .filter((f) => f.endsWith(".spec.ts"))
      .filter((f) => readFileSync(new URL(f, dir), "utf8").includes("tag: PERF_TAG"))
      .sort();
    expect(tagged).toEqual(["dash-anchor.spec.ts", "hidden-chart.spec.ts"]);
  });
});

describe("CI: a cut-off job keeps its report (AC-9QTPNR.3)", () => {
  for (const [name, artifact] of [
    ["e2e-ui", "playwright-report-${{ matrix.os }}-${{ matrix.shard }}"],
    ["e2e-ui-perf", "playwright-report-perf-${{ matrix.os }}"],
  ] as const) {
    it(`${name} uploads on failure or cancellation, under its own name`, () => {
      const lines = job(name);
      expect(ifOf(lines, "Upload Playwright report")).toBe("if: failure() || cancelled()");
      expect(lines.map((l) => l.trim())).toContain(`name: ${artifact}`);
    });
  }
});
