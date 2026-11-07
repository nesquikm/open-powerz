/**
 * FR QFTCWY, AC-QFTCWY.4: `bun run build:wasm` builds the powerz-wasm crate in release
 * mode for wasm32 and runs a wasm-bindgen-cli pinned to the crate's exact wasm-bindgen
 * version into a git-ignored folder of the web app (apps/desktop/src/wasm/); the CI gate
 * runs it before the Vitest step, and the output module is at most 1.5 MB.
 *
 * Line-based checks of package.json scripts and .github/workflows/ci.yml (no YAML library
 * is available), in the style of e2e/ui/ciWorkflow.test.ts. The size check reads the
 * module the gate built just before `bun run test`.
 */

import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const REPO_ROOT = fileURLToPath(new URL("../../../../../", import.meta.url));
const DESKTOP = join(REPO_ROOT, "apps/desktop");
const OUT_DIR = join(DESKTOP, "src/wasm");
const MODULE = join(OUT_DIR, "powerz_wasm_bg.wasm");
const GLUE = join(OUT_DIR, "powerz_wasm.js");
const MAX_MODULE_BYTES = 1_500_000;

const read = (rel: string): string => {
  const p = join(REPO_ROOT, rel);
  return existsSync(p) ? readFileSync(p, "utf8") : "";
};

const scripts = (rel: string): Record<string, string> => {
  const text = read(rel);
  return text === ""
    ? {}
    : ((JSON.parse(text) as { scripts?: Record<string, string> }).scripts ?? {});
};

/** The desktop build:wasm command plus any repo script file it runs (scripts/*.ts|js|sh). */
function buildWasmText(): string {
  const cmd = scripts("apps/desktop/package.json")["build:wasm"] ?? "";
  const files = [...cmd.matchAll(/(?:^|\s)((?:\.\/)?scripts\/[\w./-]+\.(?:ts|js|mjs|sh))/g)].map(
    (m) => m[1] ?? "",
  );
  return [cmd, ...files.map((f) => read(join("apps/desktop", f)))].join("\n");
}

/** The exact version the crate pins: `wasm-bindgen = "=X.Y.Z"`. */
function pinnedWasmBindgen(): string | undefined {
  return /^\s*wasm-bindgen\s*=\s*(?:\{[^}]*version\s*=\s*)?"=(\d+\.\d+\.\d+)"/m.exec(
    read("crates/powerz-wasm/Cargo.toml"),
  )?.[1];
}

const ciLines = (): string[] => read(".github/workflows/ci.yml").split(/\r?\n/);

const indentOf = (line: string): number => line.length - line.trimStart().length;

/** The lines of the `gate:` job (one level under `jobs:`). */
function gateJob(): string[] {
  const lines = ciLines();
  const at = lines.findIndex((l) => /^ {2}gate:\s*$/.test(l));
  if (at < 0) return [];
  const out: string[] = [];
  for (const line of lines.slice(at + 1)) {
    if (line.trim() !== "" && indentOf(line) <= 2) break;
    out.push(line);
  }
  return out.filter((l) => !l.trim().startsWith("#"));
}

/** The lines of the step (`- name: ...` item) that holds line `at`. */
function stepOf(lines: string[], at: number): string[] {
  const isStart = (l: string): boolean => /^\s*- /.test(l);
  let start = at;
  while (start > 0 && !isStart(lines[start] ?? "")) start -= 1;
  let end = at + 1;
  while (end < lines.length && !isStart(lines[end] ?? "")) end += 1;
  return lines.slice(start, end);
}

const lineOf = (lines: string[], re: RegExp): number => lines.findIndex((l) => re.test(l));

describe("build:wasm script (AC-QFTCWY.4)", () => {
  it("the root package.json delegates build:wasm to apps/desktop", () => {
    expect(scripts("package.json")["build:wasm"]).toBe("bun run --cwd apps/desktop build:wasm");
  });

  it("builds powerz-wasm in release mode for wasm32-unknown-unknown", () => {
    const text = buildWasmText();
    expect(text).toMatch(/cargo\b[^\n]*\bbuild\b/);
    expect(text).toMatch(/(-p|--package)[ =]?powerz-wasm/);
    expect(text).toContain("--release");
    expect(text).toContain("wasm32-unknown-unknown");
  });

  it("runs wasm-bindgen into the web app's src/wasm folder", () => {
    const text = buildWasmText();
    expect(text).toMatch(/wasm-bindgen\b/);
    expect(text).toMatch(/--out-dir[ =]+["']?(\.\/)?src\/wasm\b/);
    expect(text).toContain("powerz_wasm.wasm");
  });

  it("the crate pins wasm-bindgen exactly and the gate installs that CLI version", () => {
    const version = pinnedWasmBindgen();
    expect(version, 'crates/powerz-wasm/Cargo.toml: wasm-bindgen = "=X.Y.Z"').toMatch(
      /^\d+\.\d+\.\d+$/,
    );
    const lock = read("Cargo.lock");
    expect(lock).toContain(`name = "wasm-bindgen"\nversion = "${version}"`);
    const gate = gateJob().join("\n");
    expect(gate).toMatch(/cargo install[^\n]*wasm-bindgen-cli/);
    expect(gate).toContain(version);
  });

  it("the output folder is git-ignored", () => {
    const out = execFileSync(
      "git",
      ["check-ignore", "-q", "--no-index", "apps/desktop/src/wasm/powerz_wasm.js"],
      { cwd: REPO_ROOT, stdio: "pipe" },
    );
    expect(out.toString()).toBe("");
  });
});

describe("gate order (AC-QFTCWY.4)", () => {
  it("the gate job adds the wasm32 target and runs build:wasm before bun run test", () => {
    const gate = gateJob();
    const target = lineOf(gate, /rustup target add[^\n]*wasm32-unknown-unknown/);
    const build = lineOf(gate, /\bbun run build:wasm\b/);
    const test = lineOf(gate, /^\s*(run:\s*)?bun run test\s*$/);
    expect(target, "rustup target add wasm32-unknown-unknown in the gate job").toBeGreaterThan(-1);
    expect(build, "bun run build:wasm in the gate job").toBeGreaterThan(-1);
    expect(test, "bun run test in the gate job").toBeGreaterThan(-1);
    expect(target).toBeLessThan(build);
    expect(build).toBeLessThan(test);
    // Neither step is limited to one OS: the Vitest step runs on all three.
    for (const at of [target, build]) {
      expect(stepOf(gate, at).join("\n"), gate[at]).not.toMatch(/^\s*if:/m);
    }
  });
});

describe("built module (AC-QFTCWY.4)", () => {
  it("build:wasm left the module and its JS glue in src/wasm", () => {
    expect(existsSync(MODULE), `${MODULE} (run bun run build:wasm)`).toBe(true);
    expect(existsSync(GLUE), `${GLUE} (run bun run build:wasm)`).toBe(true);
  });

  it("the module is at most 1.5 MB", () => {
    expect(existsSync(MODULE)).toBe(true);
    const size = statSync(MODULE).size;
    expect(size).toBeGreaterThan(0);
    expect(size).toBeLessThanOrEqual(MAX_MODULE_BYTES);
  });
});
