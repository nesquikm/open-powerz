/**
 * FR 2BF5G6: the CI workflow fails a step when any of its commands fails, on every OS,
 * and the repository keeps text files LF so a Windows checkout lints the real code.
 *
 * AC-2BF5G6.1: a workflow-level `defaults: run: shell: bash` (errexit + pipefail on every
 * runner, Windows included), and no step or job swaps in a shell that keeps only the last
 * command's exit code.
 * AC-2BF5G6.2: every step that sets its own PowerShell shell starts its script with
 * `$ErrorActionPreference = 'Stop'`.
 * AC-2BF5G6.3: a repo-root `.gitattributes` pins text files to LF.
 */

import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const REPO_ROOT = fileURLToPath(new URL("../../../../", import.meta.url));
const WORKFLOW = readFileSync(
  new URL("../../../../.github/workflows/ci.yml", import.meta.url),
  "utf8",
);
const LINES = WORKFLOW.split(/\r?\n/);

const indentOf = (line: string): number => line.length - line.trimStart().length;
const isBlank = (line: string): boolean => line.trim() === "" || line.trim().startsWith("#");

/** The lines of the YAML block opened by the key on line `i` (more indented than it). */
function blockAfter(i: number): string[] {
  const base = indentOf(LINES[i] ?? "");
  const out: string[] = [];
  for (const line of LINES.slice(i + 1)) {
    if (line.trim() !== "" && indentOf(line) <= base) break;
    out.push(line);
  }
  return out;
}

interface Step {
  /** Line index of the step's `- ` item. */
  start: number;
  lines: string[];
}

/** Every step item (`- ...` under a `steps:` key) in the workflow. */
function steps(): Step[] {
  const out: Step[] = [];
  LINES.forEach((line, i) => {
    if (!/^\s*steps:\s*$/.test(line)) return;
    const body = blockAfter(i);
    const items = body.filter((l) => /^\s*-\s/.test(l));
    if (items.length === 0)
      throw new Error(`ci.yml line ${i + 1}: a steps: block has no step items`);
    const itemIndent = Math.min(...items.map((l) => indentOf(l)));
    let current: Step | null = null;
    body.forEach((l, k) => {
      if (/^\s*-\s/.test(l) && indentOf(l) === itemIndent) {
        current = { start: i + 1 + k, lines: [l.replace(/^(\s*)-\s/, "$1  ")] };
        out.push(current);
      } else if (current) {
        current.lines.push(l);
      }
    });
  });
  return out;
}

/** The value of `key:` among a step's own keys (first match), or null. */
function stepKey(step: Step, key: string): string | null {
  const re = new RegExp(`^\\s*${key}:\\s*(.*)$`);
  for (const l of step.lines) {
    const m = re.exec(l);
    if (m) return (m[1] ?? "").trim();
  }
  return null;
}

/** The script lines of a step's `run:` (block scalar or inline), comments and blanks dropped. */
function runScript(step: Step): string[] {
  const idx = step.lines.findIndex((l) => /^\s*run:/.test(l));
  if (idx < 0) return [];
  const head = (step.lines[idx] ?? "").replace(/^\s*run:\s*/, "");
  if (!/^[|>][-+]?\s*$/.test(head)) return [head];
  const base = indentOf(step.lines[idx] ?? "");
  const out: string[] = [];
  for (const l of step.lines.slice(idx + 1)) {
    if (l.trim() !== "" && indentOf(l) <= base) break;
    if (!isBlank(l)) out.push(l.trim());
  }
  return out;
}

const stepName = (s: Step): string =>
  stepKey(s, "name") ?? stepKey(s, "uses") ?? `line ${s.start + 1}`;
const isPowerShell = (shell: string | null): boolean =>
  shell !== null && /^(pwsh|powershell)\b/i.test(shell.replace(/["']/g, ""));

describe("CI workflow: a failing command fails its step (AC-2BF5G6.1)", () => {
  it("sets a workflow-level `defaults: run: shell: bash`", () => {
    const at = LINES.findIndex((l) => /^defaults:\s*$/.test(l));
    expect(at, "no top-level `defaults:` key in ci.yml").toBeGreaterThanOrEqual(0);
    const body = blockAfter(at);
    const runAt = body.findIndex((l) => /^\s+run:\s*$/.test(l));
    expect(runAt, "`defaults:` has no `run:` block").toBeGreaterThanOrEqual(0);
    const shell = body
      .slice(runAt + 1)
      .map((l) => /^\s+shell:\s*(\S+)\s*$/.exec(l)?.[1])
      .find((v) => v !== undefined);
    expect(shell, "`defaults: run:` does not set `shell: bash`").toBe("bash");
  });

  it("has no job-level default shell other than bash", () => {
    const jobDefaults = LINES.flatMap((l, i) =>
      /^\s+defaults:\s*$/.test(l) ? [blockAfter(i).join("\n")] : [],
    );
    for (const block of jobDefaults) {
      const shell = /^\s+shell:\s*(\S+)/m.exec(block)?.[1];
      if (shell !== undefined) expect(shell).toBe("bash");
    }
  });

  it("only lets a step choose bash or PowerShell, never cmd or sh", () => {
    const all = steps();
    expect(all.length).toBeGreaterThan(10);
    for (const s of all) {
      const shell = stepKey(s, "shell");
      if (shell === null) continue;
      expect(
        shell === "bash" || isPowerShell(shell),
        `step "${stepName(s)}" runs in shell "${shell}"`,
      ).toBe(true);
    }
  });

  it("covers the multi-command JS step, which carries no shell override of its own", () => {
    const js = steps().find((s) => stepName(s) === "JS lint, typecheck, test, build");
    expect(js, "the JS gate step is missing").toBeDefined();
    expect(runScript(js!).length).toBeGreaterThan(1);
    expect(stepKey(js!, "shell")).toBeNull();
  });
});

describe("CI workflow: PowerShell steps stop on the first error (AC-2BF5G6.2)", () => {
  const pwshSteps = steps().filter((s) => isPowerShell(stepKey(s, "shell")));

  it("finds the PowerShell steps", () => {
    expect(pwshSteps.length).toBeGreaterThan(0);
  });

  it("starts every PowerShell script with $ErrorActionPreference = 'Stop'", () => {
    const offenders = pwshSteps
      .filter((s) => !/^\$ErrorActionPreference\s*=\s*['"]Stop['"]\s*$/.test(runScript(s)[0] ?? ""))
      .map(stepName);
    expect(offenders, "PowerShell steps that do not stop on the first error").toEqual([]);
  });
});

describe("line endings (AC-2BF5G6.3)", () => {
  it("has a repo-root .gitattributes that pins text files to LF", () => {
    const path = `${REPO_ROOT}.gitattributes`;
    expect(existsSync(path), "no .gitattributes at the repository root").toBe(true);
    const text = readFileSync(path, "utf8");
    expect(text).toMatch(/^\*\s+(?=.*\btext(=auto)?\b)(?=.*\beol=lf\b).*$/m);
  });

  it("checks out source, config and workflow files with LF on every platform", () => {
    const files = [
      "package.json",
      "Cargo.toml",
      ".github/workflows/ci.yml",
      "apps/desktop/src/App.svelte",
      "apps/desktop/src/lib/mockBackend.ts",
      "apps/desktop/src-tauri/tests/replug.rs",
      "crates/powerz-protocol/src/lib.rs",
      "docs/protocol.md",
    ];
    const out = execFileSync("git", ["check-attr", "eol", "--", ...files], {
      cwd: REPO_ROOT,
      encoding: "utf8",
    });
    for (const f of files) {
      expect(out, `${f} is not pinned to LF`).toContain(`${f}: eol: lf`);
    }
  });
});
