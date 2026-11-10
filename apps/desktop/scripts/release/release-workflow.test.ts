/**
 * FR E1ZJ2N: a version tag builds and publishes a GitHub Release.
 *
 * Line-based structural checks of .github/workflows/release.yml (no YAML library is
 * available), in the style of e2e/ui/ciWorkflow.test.ts. AC-E1ZJ2N.11 (the PR dry run
 * and the manual smoke test of the downloaded files) is verified after the PR.
 */

import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { scanWorkflow } from "./toolchain-pin";

const here = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(here, "../../../..");
const WORKFLOW_PATH = join(REPO_ROOT, ".github/workflows/release.yml");
const TAURI_ACTION_SHA = "1deb371b0cd8bd54025b384f1cd735e725c4060f";

// Read lazily so a missing workflow fails each test on its own, not the module load.
const text = (): string => (existsSync(WORKFLOW_PATH) ? readFileSync(WORKFLOW_PATH, "utf8") : "");
const lines = (): string[] => text().split(/\r?\n/);

const indentOf = (line: string): number => line.length - line.trimStart().length;
const isComment = (line: string): boolean => line.trim().startsWith("#");

/** The lines of the YAML block opened by the key on line `i` of `src` (more indented than it). */
function blockAfter(src: string[], i: number): string[] {
  const base = indentOf(src[i] ?? "");
  const out: string[] = [];
  for (const line of src.slice(i + 1)) {
    if (line.trim() !== "" && indentOf(line) <= base) break;
    out.push(line);
  }
  return out;
}

/** The block of the top-level key `key:` (e.g. `on`, `jobs`, `permissions`), or []. */
function topBlock(key: string, src: string[] = lines()): string[] {
  const at = src.findIndex((l) => new RegExp(`^["']?${key}["']?:\\s*$`).test(l));
  return at < 0 ? [] : blockAfter(src, at);
}

/** The jobs as { id, lines } (job ids sit one level under `jobs:`). */
function jobs(src: string[] = lines()): { id: string; lines: string[] }[] {
  const body = topBlock("jobs", src);
  const keyed = body.filter((l) => /^\s+[\w-]+:\s*$/.test(l) && !isComment(l));
  if (keyed.length === 0) return [];
  const jobIndent = Math.min(...keyed.map(indentOf));
  const out: { id: string; lines: string[] }[] = [];
  body.forEach((l, k) => {
    const m = /^\s+([\w-]+):\s*$/.exec(l);
    if (m && indentOf(l) === jobIndent) out.push({ id: m[1] ?? "", lines: blockAfter(body, k) });
  });
  return out;
}

/** A job's text without comment lines, so a comment never satisfies an assertion. */
const jobText = (j: { lines: string[] }): string => j.lines.filter((l) => !isComment(l)).join("\n");
const findJob = (pred: (t: string) => boolean) => jobs().find((j) => pred(jobText(j)));

const checkJob = () => findJob((t) => /versions\.ts/.test(t));
const desktopJob = () => findJob((t) => /tauri-apps\/tauri-action@/.test(t));
const cliJob = () => findJob((t) => /cargo build --release -p powerz-cli/.test(t));
const publishJob = () => findJob((t) => /contents:\s*write/.test(t));

/** The text of the publish job's run step that calls publish.ts, or "". */
function publishRun(): string {
  const j = publishJob();
  if (!j) return "";
  const src = j.lines;
  for (let i = 0; i < src.length; i++) {
    if (!/^\s*(?:-\s+)?run:/.test(src[i] ?? "")) continue;
    const script = [src[i] ?? "", ...blockAfter(src, i)].filter((l) => !isComment(l)).join("\n");
    if (/publish\.ts/.test(script)) return script;
  }
  return "";
}

/** The `needs:` of a job, inline (`needs: a` / `needs: [a, b]`) or as a list. */
function needsOf(j: { lines: string[] }): string[] {
  const at = j.lines.findIndex((l) => /^\s*needs:/.test(l));
  if (at < 0) return [];
  const inline = (j.lines[at] ?? "").replace(/^\s*needs:\s*/, "").trim();
  if (inline !== "")
    return inline
      .replace(/[[\]"']/g, "")
      .split(/\s*,\s*/)
      .filter(Boolean);
  return blockAfter(j.lines, at)
    .map((l) => /^\s*-\s*([\w-]+)/.exec(l)?.[1])
    .filter((v): v is string => v !== undefined);
}

describe("release.yml exists", () => {
  it("is at .github/workflows/release.yml", () => {
    expect(existsSync(WORKFLOW_PATH), "no .github/workflows/release.yml").toBe(true);
  });
});

describe("triggers (AC-E1ZJ2N.1)", () => {
  it("starts on a pushed v* tag", () => {
    const on = topBlock("on");
    const pushAt = on.findIndex((l) => /^\s+push:\s*$/.test(l));
    expect(pushAt, "no `push:` trigger").toBeGreaterThanOrEqual(0);
    const push = blockAfter(on, pushAt).join("\n");
    expect(push).toMatch(/tags:/);
    expect(push).toMatch(/["']v\*["']|-\s*v\*\s*$|\[\s*["']?v\*["']?\s*\]/m);
    expect(push, "a branch push must not start a release").not.toMatch(/branches:/);
  });

  it("has a manual run that takes a required `tag` input", () => {
    const on = topBlock("on");
    const wdAt = on.findIndex((l) => /^\s+workflow_dispatch:\s*$/.test(l));
    expect(wdAt, "no `workflow_dispatch:` trigger").toBeGreaterThanOrEqual(0);
    const wd = blockAfter(on, wdAt);
    const tagAt = wd.findIndex((l) => /^\s+tag:\s*$/.test(l));
    expect(tagAt, "workflow_dispatch has no `tag` input").toBeGreaterThanOrEqual(0);
    expect(blockAfter(wd, tagAt).join("\n")).toMatch(/required:\s*true/);
  });
});

describe("manual run for a tag that predates the workflow (AC-E1ZJ2N.1)", () => {
  it("checks an existing tag with this workflow's own scripts", () => {
    const t = jobText(checkJob() ?? { lines: [] });
    // The tag goes into its own directory; the scripts come from the workflow's commit.
    expect(t).toMatch(/path:\s*release-src/);
    expect(t).toMatch(/ref:\s*\$\{\{\s*format\('refs\/tags\/\{0\}',\s*inputs\.tag\)/);
    expect(t).toMatch(/versions\.ts "\$TAG" --root "\$SRC"/);
  });

  it.each([
    ["desktop", desktopJob],
    ["cli", cliJob],
  ])("%s job falls back to this workflow's rust-toolchain.toml for an older tag", (_, job) => {
    const t = jobText(job() ?? { lines: [] });
    expect(t).toMatch(/path:\s*\.release-tools/);
    expect(t).toMatch(/sparse-checkout:\s*rust-toolchain\.toml/);
    expect(t).toMatch(
      /\[ -f rust-toolchain\.toml \] \|\| cp \.release-tools\/rust-toolchain\.toml \./,
    );
    // The fallback must be in place before rustup reads the file.
    expect(t.indexOf(".release-tools/rust-toolchain.toml")).toBeLessThan(
      t.indexOf("rustup toolchain install"),
    );
  });
});

describe("version check before any build (AC-E1ZJ2N.2)", () => {
  it("has a check job that runs versions.ts", () => {
    const j = checkJob();
    expect(j, "no job runs versions.ts").toBeDefined();
    expect(jobText(j!)).toMatch(/bun\s+\S*versions\.ts/);
  });

  it("makes the desktop and cli jobs wait for the check job", () => {
    const check = checkJob();
    const desktop = desktopJob();
    const cli = cliJob();
    expect(check && desktop && cli, "check, desktop and cli jobs must all exist").toBeTruthy();
    expect(needsOf(desktop!)).toContain(check!.id);
    expect(needsOf(cli!)).toContain(check!.id);
  });

  it("exposes the check results as job outputs", () => {
    const j = checkJob();
    expect(j).toBeDefined();
    expect(jobText(j!)).toMatch(/^\s+outputs:\s*$/m);
    expect(jobText(j!)).toMatch(/GITHUB_OUTPUT/);
  });
});

describe("desktop builds with tauri-action (AC-E1ZJ2N.3)", () => {
  it("pins tauri-action to the approved commit", () => {
    const j = desktopJob();
    expect(j, "no job uses tauri-apps/tauri-action").toBeDefined();
    expect(jobText(j!)).toContain(`tauri-apps/tauri-action@${TAURI_ACTION_SHA}`);
  });

  it("builds a macOS universal .dmg, Windows .msi + NSIS and Linux AppImage + .deb", () => {
    const j = desktopJob();
    expect(j).toBeDefined();
    const t = jobText(j!);
    expect(t).toMatch(/macos-/);
    expect(t).toMatch(/--target universal-apple-darwin/);
    expect(t).toMatch(/--bundles dmg\b/);
    expect(t).toMatch(/windows-/);
    expect(t).toMatch(/--bundles msi,nsis\b/);
    expect(t).toMatch(/ubuntu-22\.04/);
    expect(t).toMatch(/--bundles appimage,deb\b/);
  });

  it("does not let tauri-action create or upload to a release", () => {
    const j = desktopJob();
    expect(j, "no job uses tauri-apps/tauri-action").toBeDefined();
    const t = jobText(j!);
    expect(t).not.toMatch(/^\s+(tagName|releaseId|releaseName|releaseBody):/m);
  });

  it("signs nothing and needs no secret", () => {
    const all = text();
    expect(all, "release.yml is missing or empty").not.toBe("");
    expect(all).not.toMatch(/\bAPPLE_[A-Z_]+/);
    expect(all).not.toMatch(/\bTAURI_SIGNING_[A-Z_]+/);
    expect(all).not.toMatch(/\bWINDOWS_CERTIFICATE\b/);
    expect(all).not.toMatch(/secrets\./);
  });
});

describe("CLI builds (AC-E1ZJ2N.4)", () => {
  it("builds powerz-cli in release mode and merges the macOS binaries with lipo", () => {
    const j = cliJob();
    expect(j, "no job runs `cargo build --release -p powerz-cli`").toBeDefined();
    const t = jobText(j!);
    expect(t).toMatch(/lipo -create/);
    expect(t).toMatch(/aarch64-apple-darwin/);
    expect(t).toMatch(/x86_64-apple-darwin/);
  });

  it("packs per-OS archives with the LICENSE and README", () => {
    const t = jobText(cliJob() ?? { lines: [] });
    expect(t).toMatch(/powerz-\S*macos-universal\.tar\.gz/);
    expect(t).toMatch(/powerz-\S*linux-x86_64\.tar\.gz/);
    expect(t).toMatch(/powerz-\S*windows-x86_64\.zip/);
    expect(t).toMatch(/\bLICENSE\b/);
    expect(t).toMatch(/\bREADME\.md\b/);
    expect(t).toMatch(/\btar -czf\b[^\n]*\.tar\.gz/);
    expect(t).toMatch(/\b7z a\b[^\n]*\.zip/);
  });
});

describe("publishing (AC-E1ZJ2N.5–7)", () => {
  it("has a publish job that needs the desktop and cli builds", () => {
    const p = publishJob();
    expect(p, "no job has `contents: write`").toBeDefined();
    const n = needsOf(p!);
    expect(n).toContain(desktopJob()?.id ?? "<desktop>");
    expect(n).toContain(cliJob()?.id ?? "<cli>");
    expect(n).toContain(checkJob()?.id ?? "<check>");
  });

  it("titles the release from the check outputs and uses the notes file (AC-E1ZJ2N.5)", () => {
    const t = jobText(publishJob() ?? { lines: [] });
    expect(t).toMatch(/TITLE:\s*\$\{\{\s*needs\.check\.outputs\.title\s*\}\}/);
    expect(publishRun()).toMatch(/release-notes\.md/);
  });

  it("marks a pre-release tag as a pre-release (AC-E1ZJ2N.5)", () => {
    const t = jobText(publishJob() ?? { lines: [] });
    expect(t).toMatch(/PRERELEASE:\s*\$\{\{\s*needs\.check\.outputs\.prerelease\s*\}\}/);
  });

  it("attaches a SHA256SUMS.txt made with sha256sum (AC-E1ZJ2N.6)", () => {
    const assemble = findJob((t) => /sha256sum\s+--\s+\*\s*>\s*SHA256SUMS\.txt/.test(t));
    expect(assemble, "no job writes SHA256SUMS.txt over every file").toBeDefined();
    expect(needsOf(publishJob() ?? { lines: [] })).toContain(assemble?.id);
    expect(jobText(publishJob() ?? { lines: [] })).toMatch(
      /sha256sum --check --strict SHA256SUMS\.txt/,
    );
  });

  it("refuses to publish an incomplete file set (AC-E1ZJ2N.7)", () => {
    const t = jobText(
      findJob((x) => /SHA256SUMS\.txt/.test(x) && !/contents:\s*write/.test(x)) ?? { lines: [] },
    );
    for (const name of ["_universal.dmg", "_x64-setup.exe", ".msi", ".AppImage", ".deb"]) {
      expect(t).toContain(name);
    }
    expect(t).toMatch(/macos-universal\.tar\.gz/);
    expect(t).toMatch(/linux-x86_64\.tar\.gz/);
    expect(t).toMatch(/windows-x86_64\.zip/);
    // The .msi is only expected for a final release, and stray files fail the check too.
    expect(t).toMatch(/if \[ "\$PRERELEASE" != "true" \]; then expected\+=\("\*\.msi"\)/);
    expect(t).toMatch(/\[ "\$total" -ne "\$\{#expected\[@\]\}" \]/);
  });

  it("builds only the NSIS installer for a pre-release on Windows (WiX needs a numeric version)", () => {
    const t = jobText(desktopJob() ?? { lines: [] });
    expect(t).toMatch(/prerelease_args:\s*--bundles nsis/);
    expect(t).toMatch(
      /needs\.check\.outputs\.prerelease == 'true' && matrix\.prerelease_args \|\| matrix\.args/,
    );
  });
});

// FR 4V3DHP: the draft/upload/check/publish logic moved into publish.ts (tested in
// publish.test.ts against a fake GitHub API); the job only runs it.
describe("publish job runs the tested publish script (AC-4V3DHP.1–4)", () => {
  it("runs publish.ts with bun on the notes file and every dist file", () => {
    const run = publishRun();
    expect(run, "no run step calls publish.ts").toMatch(
      /bun\s+apps\/desktop\/scripts\/release\/publish\.ts\b/,
    );
    expect(run).toMatch(/\$\{?RUNNER_TEMP\}?"?\/notes\/release-notes\.md/);
    expect(run).toMatch(/\$\{?RUNNER_TEMP\}?"?\/dist\/\*/);
  });

  it("passes the tag, title and pre-release flag from the check job", () => {
    const t = jobText(publishJob() ?? { lines: [] });
    expect(t).toMatch(/TAG:\s*\$\{\{\s*needs\.check\.outputs\.tag\s*\}\}/);
    expect(t).toMatch(/TITLE:\s*\$\{\{\s*needs\.check\.outputs\.title\s*\}\}/);
    expect(t).toMatch(/PRERELEASE:\s*\$\{\{\s*needs\.check\.outputs\.prerelease\s*\}\}/);
    expect(t).toMatch(/GH_TOKEN:\s*\$\{\{\s*github\.token\s*\}\}/);
  });

  it("gives the write token to the publish step only, and caps the job's run time", () => {
    const j = publishJob() ?? { lines: [] };
    const tokenLines = j.lines.filter((l) => /GH_TOKEN:/.test(l));
    expect(tokenLines, "GH_TOKEN appears once").toHaveLength(1);
    const step = j.lines.findIndex((l) => /name:\s*Publish the release/.test(l));
    expect(step, "no `Publish the release` step").toBeGreaterThanOrEqual(0);
    expect(blockAfter(j.lines, step).join("\n")).toMatch(/GH_TOKEN:/);
    expect(jobText(j)).toMatch(/^\s+timeout-minutes:\s*\d+/m);
  });

  it("checks out only the release scripts from the workflow's own commit", () => {
    const j = publishJob() ?? { lines: [] };
    const at = j.lines.findIndex((l) => /uses:\s*actions\/checkout@[0-9a-f]{40}\s+#/.test(l));
    expect(at, "the publish job has no pinned actions/checkout").toBeGreaterThanOrEqual(0);
    const block = blockAfter(j.lines, at).join("\n");
    expect(block).toMatch(/persist-credentials:\s*false/);
    expect(block).toMatch(/sparse-checkout:\s*\|?\s*\n?\s*apps\/desktop\/scripts\/release\b/);
    // The workflow's commit, never the tag: an older tag may not carry publish.ts.
    expect(block).not.toMatch(/^\s*ref:/m);
    expect(block).not.toMatch(/^\s*path:/m);
  });

  it("sets up bun pinned like the check job", () => {
    const t = jobText(publishJob() ?? { lines: [] });
    expect(t).toMatch(/uses:\s*oven-sh\/setup-bun@0c5077e51419868618aeaa5fe8019c62421857d6\s+#/);
    expect(t).toMatch(/bun-version:\s*1\.3\.14/);
  });

  it("verifies the checksums and sets up bun before publishing", () => {
    const t = jobText(publishJob() ?? { lines: [] });
    const publish = t.search(/publish\.ts/);
    expect(publish).toBeGreaterThanOrEqual(0);
    expect(t.search(/sha256sum --check --strict SHA256SUMS\.txt/)).toBeLessThan(publish);
    expect(t.search(/oven-sh\/setup-bun@/)).toBeLessThan(publish);
    expect(t.search(/actions\/checkout@/)).toBeLessThan(publish);
  });

  it("no longer looks the release up with gh after creating it (AC-4V3DHP.6)", () => {
    const t = jobText(publishJob() ?? { lines: [] });
    expect(t).not.toMatch(/gh release (create|edit|upload|delete-asset)/);
    expect(t).not.toMatch(/gh api [^\n]*releases/);
  });
});

describe("pull request dry run (AC-E1ZJ2N.8)", () => {
  it("runs on pull requests that touch the release setup", () => {
    const on = topBlock("on");
    const prAt = on.findIndex((l) => /^\s+pull_request:\s*$/.test(l));
    expect(prAt, "no `pull_request:` trigger").toBeGreaterThanOrEqual(0);
    const pr = blockAfter(on, prAt).join("\n");
    expect(pr).toMatch(/paths:/);
    expect(pr).toContain(".github/workflows/release.yml");
    expect(pr).toContain("apps/desktop/scripts/release/**");
    expect(pr).toContain("apps/desktop/src-tauri/tauri.conf.json");
  });

  it("never publishes from a pull request", () => {
    const p = publishJob();
    expect(p).toBeDefined();
    const cond = p!.lines.find((l) => /^\s*if:/.test(l)) ?? "";
    expect(cond, "the publish job has no `if:`").toMatch(/pull_request/);
    expect(cond).toMatch(
      /!=\s*['"]pull_request['"]|!\s*\(?\s*github\.event_name\s*==\s*['"]pull_request/,
    );
  });

  it("uploads the built files as workflow artifacts", () => {
    const uploads = lines().filter((l) => /uses:\s*actions\/upload-artifact@/.test(l));
    expect(uploads.length, "no actions/upload-artifact step").toBeGreaterThanOrEqual(2);
  });

  it("checks versions against Cargo on a pull request (no tag to read)", () => {
    expect(jobText(checkJob() ?? { lines: [] })).toMatch(/--from-cargo/);
  });
});

describe("permissions, pins and tokens (AC-E1ZJ2N.9)", () => {
  it("defaults to read-only contents at the top level", () => {
    const perms = topBlock("permissions").join("\n");
    expect(perms, "no top-level `permissions:` block").toMatch(/contents:\s*read/);
    expect(perms).not.toMatch(/write/);
  });

  it("grants contents: write to exactly one job, the publish job", () => {
    const writers = jobs().filter((j) => /contents:\s*write/.test(jobText(j)));
    expect(writers.map((j) => j.id)).toHaveLength(1);
    expect(jobText(writers[0] ?? { lines: [] })).toMatch(/publish\.ts/);
  });

  it("pins every action to a full commit SHA with a version comment", () => {
    // A local reusable workflow (`uses: ./.github/workflows/…`, FR QFTCX1) lives in this repo: no pin.
    const uses = lines().filter(
      (l) => /^\s*-?\s*uses:/.test(l) && !isComment(l) && !/uses:\s*\.\//.test(l),
    );
    expect(uses.length).toBeGreaterThan(3);
    const offenders = uses.filter(
      (l) => !/uses:\s*[\w.-]+\/[\w./-]+@[0-9a-f]{40}\s+#\s+\S/.test(l),
    );
    expect(offenders, "actions not pinned to a 40-hex SHA + `# version`").toEqual([]);
  });

  it("uses only the built-in GITHUB_TOKEN", () => {
    const all = text();
    expect(all).toMatch(/github\.token|GITHUB_TOKEN/);
    const tokens = [...all.matchAll(/\$\{\{\s*([^}]*token[^}]*)\}\}/gi)].map((m) => m[1]?.trim());
    for (const t of tokens) expect(t).toBe("github.token");
    expect(all).not.toMatch(/secrets\./);
  });

  it("never puts an untrusted expression straight into a run script", () => {
    // Inputs, refs, event data and job outputs reach the shell through env: only.
    const src = lines();
    const bad: string[] = [];
    src.forEach((l, i) => {
      if (!/^\s*(?:-\s+)?run:/.test(l)) return;
      const script = [l, ...blockAfter(src, i)].join("\n");
      const m = script.match(
        /\$\{\{[^}]*\b(?:inputs|github\.(?:ref|head_ref|event)|needs\.[\w-]+\.outputs|matrix)\b[^}]*\}\}/g,
      );
      if (m) bad.push(`line ${i + 1}: ${m.join(", ")}`);
    });
    expect(bad).toEqual([]);
  });

  it("checks out without persisting credentials", () => {
    const src = lines();
    const checkouts = src
      .map((l, i) => ({ l, i }))
      .filter(({ l }) => /uses:\s*actions\/checkout@/.test(l));
    expect(checkouts.length).toBeGreaterThan(0);
    for (const { i } of checkouts) {
      expect(blockAfter(src, i).join("\n")).toMatch(/persist-credentials:\s*false/);
    }
  });

  it("installs Rust from rust-toolchain.toml, never its own version", () => {
    const all = text();
    expect(all).toMatch(/rustup toolchain install/);
    expect(all).not.toMatch(/^\s+toolchain:/m);
    expect(scanWorkflow(all, "release.yml")).toEqual([]);
  });
});

// ---------------------------------------------------------------------------
// FR 4PJRVN, AC-4PJRVN.6: a CI job builds and tests with the declared MSRV
// ---------------------------------------------------------------------------

const CI_PATH = join(REPO_ROOT, ".github/workflows/ci.yml");
const CARGO_TOML = join(REPO_ROOT, "Cargo.toml");
const ciText = (): string => (existsSync(CI_PATH) ? readFileSync(CI_PATH, "utf8") : "");

/** The ci.yml job that reads `rust-version` from Cargo.toml, or undefined. */
const msrvJob = () => jobs(ciText().split(/\r?\n/)).find((j) => /rust-version/.test(jobText(j)));

/**
 * The literal 1.x versions in a job's text that could name a Rust toolchain. Action pins
 * (`@<sha>`, `# v2.9.3`) and the bun pin are other tools' versions: bun's own 1.x line
 * would otherwise trip this as soon as its minor reaches two digits.
 */
const hardCodedRustVersions = (jobBody: string): string[] =>
  jobBody
    .replace(/@[0-9a-f]{40}/g, "")
    .replace(/\bv\d+\.\d+\.\d+\b/g, "")
    .replace(/^\s*bun-version:.*$/gm, "")
    .match(/\b1\.\d{2,3}(?:\.\d+)?\b/g) ?? [];

describe("MSRV job in ci.yml (AC-4PJRVN.6)", () => {
  it("Cargo.toml still declares rust-version = 1.96 for the workspace", () => {
    expect(readFileSync(CARGO_TOML, "utf8")).toMatch(/^rust-version\s*=\s*"1\.96"\s*$/m);
  });

  it("has a job that reads the MSRV from Cargo.toml's rust-version at run time", () => {
    const j = msrvJob();
    expect(j, "no ci.yml job reads rust-version").toBeDefined();
    const t = jobText(j!);
    expect(t).toMatch(/Cargo\.toml/);
    expect(t).toMatch(/rust-version/);
  });

  it("never hard-codes a Rust version in the job", () => {
    const t = jobText(msrvJob() ?? { lines: [] });
    expect(t, "the MSRV job is missing").not.toBe("");
    expect(hardCodedRustVersions(t)).toEqual([]);
  });

  it("tells a hard-coded Rust version from bun's pin", () => {
    expect(hardCodedRustVersions("          bun-version: 1.10.2")).toEqual([]);
    expect(hardCodedRustVersions('channel = "1.96"')).toEqual(["1.96"]);
    expect(hardCodedRustVersions("rustup toolchain install 1.96.0")).toEqual(["1.96.0"]);
  });

  // AC-AXHK2H.3: with no -p and no --workspace, cargo builds and tests the default members.
  it("builds and tests the default members with --locked", () => {
    const t = jobText(msrvJob() ?? { lines: [] });
    expect(t).toMatch(/\bcargo build --locked\s*$/m);
    expect(t).toMatch(/\bcargo test --locked\s*$/m);
    // `--all-targets` on the desktop check is fine; only the workspace-wide flags are not.
    expect(t).not.toMatch(/--workspace|--all(?![-\w])/);
  });

  it("installs the Linux libraries the CLI crates link against", () => {
    const t = jobText(msrvJob() ?? { lines: [] });
    expect(t).toMatch(/libudev-dev/);
    expect(t).toMatch(/libusb-1\.0-0-dev/);
  });

  it("keeps the gate job on the pinned toolchain", () => {
    const gate = jobs(ciText().split(/\r?\n/)).find((j) => j.id === "gate");
    expect(gate, "the gate job is gone").toBeDefined();
    expect(jobText(gate!)).toMatch(/rustup toolchain install\s*$/m);
  });

  it("passes the toolchain-pin scan (no named version, channel or cargo +toolchain)", () => {
    expect(msrvJob(), "the MSRV job is missing").toBeDefined();
    expect(scanWorkflow(ciText(), "ci.yml")).toEqual([]);
  });
});

// ---------------------------------------------------------------------------
// FR AXHK2H: the MSRV job covers the desktop crate. AC-AXHK2H.5 is proved only by the
// job's run on the PR, so nothing here stands in for it.
// ---------------------------------------------------------------------------

/** The belief AC-AXHK2H.4 removes, also across a comment wrapped over two lines. */
const FALSE_MSRV_CLAIM =
  /\b(?:needs?|requires?)\s+(?:an?\s+)?(?:newer|later|higher)\s+(?:version\s+of\s+)?Rust\b/i;

/** Joins wrapped comment lines, so a claim split over two of them still matches. */
const unwrapComments = (src: string): string =>
  src.replace(/\r?\n[ \t]*(?:\/\/+|#+|\*|<!--)?[ \t]*/g, " ");

/** Records of the bug itself; they describe the claim, they do not make it. */
const isBugRecord = (path: string): boolean =>
  path === "specs/frs/AXHK2H.md" ||
  path.startsWith("specs/frs/archive/") ||
  path.startsWith("specs/plan/archive/");

/** The tracked text files that can hold a comment or a doc. */
function commentAndDocFiles(): string[] {
  return execFileSync("git", ["ls-files"], { cwd: REPO_ROOT, encoding: "utf8" })
    .split(/\r?\n/)
    .filter((p) => /\.(?:md|rs|toml|ts|js|svelte|ya?ml|sh)$/.test(p))
    .filter((p) => !isBugRecord(p) && existsSync(join(REPO_ROOT, p)));
}

/** The comment lines directly above the `msrv:` job key in ci.yml. */
function msrvJobComment(): string {
  const src = ciText().split(/\r?\n/);
  const at = src.findIndex((l) => /^\s+msrv:\s*$/.test(l));
  expect(at, "ci.yml has no msrv job").toBeGreaterThanOrEqual(0);
  let first = at;
  while (first > 0 && isComment(src[first - 1] ?? "")) first--;
  return src.slice(first, at).join("\n");
}

describe("MSRV job covers the desktop crate (FR AXHK2H)", () => {
  // AC-AXHK2H.1 (regression) and the command of AC-AXHK2H.2.
  it("the MSRV job also checks the desktop crate", () => {
    const t = jobText(msrvJob() ?? { lines: [] });
    expect(t, "the MSRV job is missing").not.toBe("");
    expect(t).toMatch(/\bcargo check -p open-powerz-desktop --all-targets --locked\s*$/m);
  });

  it("runs the desktop check on the MSRV toolchain, after the job installs it (AC-AXHK2H.2)", () => {
    const t = jobText(msrvJob() ?? { lines: [] });
    const toolchain = t.indexOf("rustup toolchain install");
    const check = t.indexOf("cargo check -p open-powerz-desktop");
    expect(toolchain, "the job installs no toolchain").toBeGreaterThanOrEqual(0);
    expect(check, "the job has no desktop check").toBeGreaterThanOrEqual(0);
    expect(toolchain).toBeLessThan(check);
  });

  it("puts the toolchain on Cargo.toml's rust-version before installing it (AC-AXHK2H.2)", () => {
    const t = jobText(msrvJob() ?? { lines: [] });
    // Without this rewrite the job silently runs on rust-toolchain.toml's pinned channel.
    const read = /^\s*(\w+)="\$\(sed\b[^\n]*\brust-version\b[^\n]*\bCargo\.toml\b/m.exec(t);
    expect(read, "the job does not read rust-version from Cargo.toml").not.toBeNull();
    const name = read?.[1] ?? "";
    const assigned = t.match(new RegExp(`^\\s*${name}=`, "gm")) ?? [];
    expect(assigned, `$${name} must hold only the value read from Cargo.toml`).toHaveLength(1);

    const write = new RegExp(
      `^\\s*sed\\s[^\\n]*-i\\b[^\\n]*\\bchannel\\b[^\\n]*\\$\\{?${name}\\b[^\\n]*\\brust-toolchain\\.toml\\b`,
      "m",
    ).exec(t);
    expect(write, `no line writes $${name} into rust-toolchain.toml's channel`).not.toBeNull();

    // Bare, so rustup takes the version from the rewritten file.
    const install = t.search(/^\s*rustup toolchain install\s*$/m);
    expect(install, "the job does not install from rust-toolchain.toml").toBeGreaterThanOrEqual(0);
    expect(read?.index ?? -1).toBeLessThan(write?.index ?? -1);
    expect(write?.index ?? Infinity).toBeLessThan(install);
  });

  it("builds the web UI before the desktop check (AC-AXHK2H.2)", () => {
    // tauri::generate_context! needs apps/desktop/dist, so the order matters.
    const t = jobText(msrvJob() ?? { lines: [] });
    const setupBun = t.search(/uses:\s*oven-sh\/setup-bun@[0-9a-f]{40}\s+#/);
    const install = t.indexOf("bun install --frozen-lockfile");
    const web = t.indexOf("bun run build:web");
    const check = t.indexOf("cargo check -p open-powerz-desktop");
    expect(setupBun, "the job does not set up a pinned bun").toBeGreaterThanOrEqual(0);
    expect(install, "the job does not install the JS packages").toBeGreaterThan(setupBun);
    expect(web, "the job does not build the web UI").toBeGreaterThan(install);
    expect(check, "the desktop check must follow the web build").toBeGreaterThan(web);
  });

  it("installs the Linux libraries the Tauri crate links (AC-AXHK2H.2)", () => {
    const t = jobText(msrvJob() ?? { lines: [] });
    for (const lib of ["libwebkit2gtk-4.1-dev", "libappindicator3-dev", "librsvg2-dev"]) {
      expect(t).toContain(lib);
    }
  });

  it("no longer describes the job as leaving the Tauri app out (AC-AXHK2H.4)", () => {
    expect(msrvJobComment()).not.toMatch(/not\s+(?:#\s+)?the\s+(?:#\s+)?Tauri/i);
  });

  it("recognizes the removed claim, so the scan below can fail (AC-AXHK2H.4)", () => {
    const wrapped = ["// the Tauri crate needs a", "// newer Rust"].join("\n");
    expect(unwrapComments(wrapped)).toMatch(FALSE_MSRV_CLAIM);
  });

  it("leaves no comment or doc saying the Tauri crate is ahead of the MSRV (AC-AXHK2H.4)", () => {
    const files = commentAndDocFiles();
    expect(files).toContain("apps/desktop/scripts/release/release-workflow.test.ts");
    expect(files).toContain("README.md");
    const offenders = files.filter((p) =>
      FALSE_MSRV_CLAIM.test(unwrapComments(readFileSync(join(REPO_ROOT, p), "utf8"))),
    );
    expect(offenders).toEqual([]);
  });
});

// ---------------------------------------------------------------------------
// FR W8SHXE: the bundler's tool downloads are cached, and a failed packaging step
// runs once more before the job fails.
// ---------------------------------------------------------------------------

const CACHE_SHA = "0057852bfaa89a56745cba8c7296529d2fc39830";

/** The steps of a job as their non-comment lines, each step starting at its `- ` line. */
function stepsOf(j: { lines: string[] }): string[][] {
  const src = j.lines;
  const at = src.findIndex((l) => /^\s*steps:\s*$/.test(l));
  if (at < 0) return [];
  const body = blockAfter(src, at).filter((l) => !isComment(l));
  const starts = body.filter((l) => /^\s*-\s/.test(l));
  if (starts.length === 0) return [];
  const stepIndent = Math.min(...starts.map(indentOf));
  const out: string[][] = [];
  for (const l of body) {
    if (indentOf(l) === stepIndent && /^\s*-\s/.test(l)) out.push([l]);
    else if (out.length > 0 && l.trim() !== "") out[out.length - 1]?.push(l);
  }
  return out;
}

const desktopSteps = (): string[] =>
  stepsOf(desktopJob() ?? { lines: [] }).map((s) => s.join("\n"));

/** The value of `key:` in a step's text (first occurrence, trailing `# comment` dropped), or "". */
function field(step: string, key: string): string {
  const m = new RegExp(`^\\s*(?:-\\s+)?${key}:\\s*(.*)$`, "m").exec(step);
  return (m?.[1] ?? "").replace(/\s+#\s.*$/, "").trim();
}

const isTauriActionStep = (s: string): boolean =>
  new RegExp(`uses:\\s*tauri-apps/tauri-action@${TAURI_ACTION_SHA}\\b`).test(s);

/** The step that reads the Tauri CLI version from bun.lock into a step output. */
const cliVersionStep = (): string | undefined =>
  desktopSteps().find(
    (s) => /bun\.lock/.test(s) && /@tauri-apps\/cli@/.test(s) && /GITHUB_OUTPUT/.test(s),
  );

const cacheSteps = (): string[] =>
  desktopSteps().filter((s) =>
    new RegExp(`uses:\\s*actions/cache@${CACHE_SHA}\\s+#\\s+v4\\.3\\.0`).test(s),
  );

describe("bundler tool cache (AC-W8SHXE.1)", () => {
  it("reads the Tauri CLI version from bun.lock into a step output, with an id", () => {
    const s = cliVersionStep();
    expect(s, "no desktop step reads @tauri-apps/cli@<version> from bun.lock").toBeDefined();
    expect(field(s ?? "", "id"), "the CLI-version step has no id").toMatch(/^[\w-]+$/);
  });

  it("restores and saves the tool folder with a pinned actions/cache v4.3.0", () => {
    expect(cacheSteps(), "no actions/cache@<v4.3.0 sha> step in the desktop job").toHaveLength(1);
  });

  it("caches only on Linux and Windows", () => {
    const s = cacheSteps()[0] ?? "";
    expect(field(s, "if")).toMatch(/^runner\.os\s*!=\s*'macOS'$/);
  });

  it("covers the Linux and Windows tool folders", () => {
    const s = cacheSteps()[0] ?? "";
    expect(s).toMatch(/^\s*~\/\.cache\/tauri\s*$/m);
    expect(s).toMatch(/^\s*~\\AppData\\Local\\tauri\s*$/m);
    // Both folders sit under the cache step's `path:` key, not elsewhere in it.
    const src = s.split("\n");
    const at = src.findIndex((l) => /^\s*path:\s*\|?\s*$/.test(l));
    expect(at, "the cache step has no block `path:`").toBeGreaterThanOrEqual(0);
    const paths = blockAfter(src, at).join("\n");
    expect(paths).toContain("~/.cache/tauri");
    expect(paths).toContain("~\\AppData\\Local\\tauri");
  });

  it("keys the cache on the runner OS and the CLI version step output", () => {
    const id = field(cliVersionStep() ?? "", "id");
    expect(id, "no CLI-version step id").not.toBe("");
    const key = field(cacheSteps()[0] ?? "", "key");
    expect(key).toMatch(/\$\{\{\s*runner\.os\s*\}\}/);
    expect(key).toMatch(new RegExp(`\\$\\{\\{\\s*steps\\.${id}\\.outputs\\.[\\w-]+\\s*\\}\\}`));
  });

  it("reads the version and restores the cache before the first build", () => {
    const steps = desktopSteps();
    const read = steps.findIndex((s) => s === cliVersionStep());
    const cache = steps.findIndex((s) => s === cacheSteps()[0]);
    const build = steps.findIndex(isTauriActionStep);
    expect(read).toBeGreaterThanOrEqual(0);
    expect(cache).toBeGreaterThanOrEqual(0);
    expect(build).toBeGreaterThanOrEqual(0);
    expect(read).toBeLessThan(cache);
    expect(cache).toBeLessThan(build);
  });
});

describe("packaging retry (AC-W8SHXE.2)", () => {
  const builds = (): string[] => desktopSteps().filter(isTauriActionStep);

  it("has exactly two tauri-action steps", () => {
    expect(builds()).toHaveLength(2);
  });

  it("lets the first build fail without failing the job, under id build", () => {
    const first = builds()[0] ?? "";
    expect(field(first, "id")).toBe("build");
    expect(field(first, "continue-on-error")).toBe("true");
    expect(field(first, "if")).toBe("");
  });

  it("runs the second build only when the first failed, and lets it fail the job", () => {
    const second = builds()[1] ?? "";
    expect(field(second, "if")).toMatch(/^steps\.build\.outcome\s*==\s*'failure'$/);
    expect(second).not.toMatch(/^\s*continue-on-error:/m);
  });

  it("retries with the same inputs", () => {
    const [first = "", second = ""] = builds();
    for (const key of ["projectPath", "tauriScript", "args"]) {
      expect(field(first, key), `first build has no ${key}`).not.toBe("");
      expect(field(second, key), `retry differs in ${key}`).toBe(field(first, key));
    }
  });

  it("collects the bundles after both builds", () => {
    const steps = desktopSteps();
    const collect = steps.findIndex((s) => /name:\s*Collect the bundles/.test(s));
    const lastBuild = steps.map((s, i) => (isTauriActionStep(s) ? i : -1)).filter((i) => i >= 0);
    expect(lastBuild).toHaveLength(2);
    expect(collect).toBeGreaterThan(Math.max(...lastBuild));
  });
});

describe("pins and lint after the cache and retry (AC-W8SHXE.3)", () => {
  it("pins every uses: in the desktop job to a 40-hex SHA with a version comment", () => {
    const uses = desktopSteps()
      .flatMap((s) => s.split("\n"))
      .filter((l) => /^\s*-?\s*uses:/.test(l));
    expect(uses.some((l) => l.includes(`actions/cache@${CACHE_SHA}`))).toBe(true);
    const offenders = uses.filter(
      (l) => !/uses:\s*[\w.-]+\/[\w./-]+@[0-9a-f]{40}\s+#\s+\S/.test(l),
    );
    expect(offenders).toEqual([]);
  });

  it("still passes the toolchain-pin scan", () => {
    expect(scanWorkflow(text(), "release.yml")).toEqual([]);
  });
});
