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
 * AC-9QTPNR.7: both UI jobs cache the browsers per OS and Playwright version, and bound
 * the download with its own time limit and one retry.
 */

import { spawnSync } from "node:child_process";
import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
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
    // Not on browsers whose setup failed (AC-9QTPNR.7).
    expect(ifOf(perf, "UI memory (WebKit)")).toContain("steps.ready.outcome == 'success'");
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

/** The lines of the step named `name` in `lines`, up to the next step. */
function stepOf(lines: string[], name: string): string[] {
  const at = lines.findIndex((l) => l.trim() === `- name: ${name}`);
  if (at < 0) throw new Error(`no step ${name}`);
  const body: string[] = [];
  for (const l of lines.slice(at + 1)) {
    if (/^\s+- /.test(l)) break;
    body.push(l.trim());
  }
  return body;
}

describe("CI: the browsers are cached and their download bounded (AC-9QTPNR.7)", () => {
  for (const name of ["e2e-ui", "e2e-ui-perf"]) {
    const lines = job(name);

    it(`${name} caches the browsers per OS and exact Playwright version`, () => {
      expect(runOf(lines, "Playwright version")).toContain("@playwright/test@");
      const cache = stepOf(lines, "Playwright browser cache");
      expect(cache.some((l) => /^uses: actions\/cache@[0-9a-f]{40} # v4/.test(l))).toBe(true);
      expect(cache).toContain(
        "key: playwright-${{ runner.os }}-${{ steps.playwright.outputs.version }}",
      );
      for (const path of [
        "~/.cache/ms-playwright",
        "~/Library/Caches/ms-playwright",
        "~/AppData/Local/ms-playwright",
      ]) {
        expect(cache).toContain(path);
      }
    });

    it(`${name} gives the download its own time limit and one retry`, () => {
      const first = stepOf(lines, "Playwright browsers");
      const retry = stepOf(lines, "Playwright browsers (retry)");
      expect(first).toContain("if: steps.browser-cache.outputs.cache-hit != 'true'");
      expect(first).toContain("continue-on-error: true");
      expect(first).toContain("timeout-minutes: 8");
      expect(retry).toContain(
        "if: steps.browser-cache.outputs.cache-hit != 'true' && steps.browsers.outcome == 'failure'",
      );
      expect(retry).toContain("timeout-minutes: 8");
    });

    it(`${name} installs Linux's system packages offline from a per-image .deb cache`, () => {
      expect(lines.join("\n")).not.toContain("--with-deps");
      expect(runOf(lines, "Playwright system packages (Linux)")).toBe(
        'run: ../../.github/scripts/playwright-deps.sh list >> "$GITHUB_OUTPUT"',
      );
      const restore = stepOf(lines, "Playwright system package cache (Linux)");
      const save = stepOf(lines, "Playwright system package cache save (Linux)");
      expect(restore.some((l) => /^uses: actions\/cache\/restore@[0-9a-f]{40} # v4/.test(l))).toBe(
        true,
      );
      expect(save.some((l) => /^uses: actions\/cache\/save@[0-9a-f]{40} # v4/.test(l))).toBe(true);
      for (const step of [restore, save]) {
        expect(step).toContain("path: ~/.cache/playwright-debs");
        expect(step).toContain("key: ${{ steps.packages.outputs.key }}");
      }
      const download = stepOf(lines, "Playwright system package download (Linux)");
      expect(download).toContain("continue-on-error: true");
      expect(download).toContain("timeout-minutes: 8");
      expect(download).toContain("run: ../../.github/scripts/playwright-deps.sh download");
      expect(stepOf(lines, "Playwright system package install (Linux)")).toContain(
        "run: ../../.github/scripts/playwright-deps.sh install",
      );
    });

    it(`${name} clears a killed apt's locks before its one retry`, () => {
      const retry = stepOf(lines, "Playwright system package download (retry)");
      expect(retry).toContain("timeout-minutes: 8");
      expect(retry.find((l) => l.startsWith("if:"))).toContain(
        "steps.package-download.outcome == 'failure'",
      );
      const script = retry.join("\n");
      expect(script.indexOf("playwright-deps.sh unlock")).toBeGreaterThan(-1);
      expect(script.indexOf("playwright-deps.sh unlock")).toBeLessThan(
        script.indexOf("playwright-deps.sh download"),
      );
    });

    it(`${name} marks the browsers ready only after a setup that worked`, () => {
      const ready = stepOf(lines, "Browsers ready");
      expect(ready).toContain("id: ready");
      expect(
        ready.some((l) => l.startsWith("if:")),
        "runs only on success",
      ).toBe(false);
    });
  }
});

const DEPS_SCRIPT = fileURLToPath(
  new URL("../../../../.github/scripts/playwright-deps.sh", import.meta.url),
);

/** The script's `parse` on `input`: its stdout lines and exit status. */
function parseDryRun(input: string): { lines: string[]; status: number } {
  const r = spawnSync("bash", [DEPS_SCRIPT, "parse"], { input, encoding: "utf8" });
  return { lines: r.stdout.split("\n").filter((l) => l !== ""), status: r.status ?? -1 };
}

describe.skipIf(process.platform === "win32")(
  "CI: the system package list comes from Playwright's dry run (AC-9QTPNR.7)",
  () => {
    it("lists the missing packages, sorted", () => {
      const out = parseDryRun(
        "Missing system dependencies (3):\n  libwoff1\n  gstreamer1.0-libav\n  libavif16\n",
      );
      expect(out).toEqual({
        lines: ["gstreamer1.0-libav", "libavif16", "libwoff1"],
        status: 0,
      });
    });

    it("lists nothing when everything is installed", () => {
      expect(parseDryRun("All system dependencies are installed.\n")).toEqual({
        lines: [],
        status: 0,
      });
    });

    it("refuses output it does not recognise", () => {
      expect(parseDryRun("something else\n").status).not.toBe(0);
    });

    it("keeps the cache key per runner image, Playwright version and list", () => {
      const text = readFileSync(DEPS_SCRIPT, "utf8");
      expect(text).toContain("${ImageOS:-unknown}-${ImageVersion:-unknown}");
      expect(text).toContain("bunx playwright --version");
      expect(text).toContain('sha256sum "${LIST}"');
    });
  },
);
