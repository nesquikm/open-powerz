/**
 * FR E1ZJ2N: a version tag builds and publishes a GitHub Release.
 *
 * Line-based structural checks of .github/workflows/release.yml (no YAML library is
 * available), in the style of e2e/ui/ciWorkflow.test.ts. AC-E1ZJ2N.11 (the PR dry run
 * and the manual smoke test of the downloaded files) is verified after the PR.
 */

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
function topBlock(key: string): string[] {
  const src = lines();
  const at = src.findIndex((l) => new RegExp(`^["']?${key}["']?:\\s*$`).test(l));
  return at < 0 ? [] : blockAfter(src, at);
}

/** The jobs as { id, lines } (job ids sit one level under `jobs:`). */
function jobs(): { id: string; lines: string[] }[] {
  const body = topBlock("jobs");
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
    expect(t).toMatch(/gh release create/);
    expect(t).toMatch(/needs\.[\w-]+\.outputs\.title/);
    expect(t).toMatch(/--notes-file/);
  });

  it("marks a pre-release tag as a pre-release (AC-E1ZJ2N.5)", () => {
    const t = jobText(publishJob() ?? { lines: [] });
    expect(t).toMatch(/--prerelease/);
    expect(t).toMatch(/needs\.[\w-]+\.outputs\.prerelease/);
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

  it("checks every file reached the release before deleting leftovers", () => {
    const t = jobText(publishJob() ?? { lines: [] });
    const verify = t.search(/grep -Fxq -- "\$\(basename "\$f"\)"/);
    const del = t.search(/gh release delete-asset/);
    expect(verify, "no post-upload check").toBeGreaterThanOrEqual(0);
    expect(verify).toBeLessThan(del);
  });

  it("reuses an existing release (drafts included) on a re-run instead of creating another", () => {
    const t = jobText(publishJob() ?? { lines: [] });
    expect(t).toMatch(/gh api "repos\/\$GH_REPO\/releases" --paginate/);
    expect(t).toMatch(/if \[ -n "\$ids" \]; then\s+gh release edit/);
    expect(t).toMatch(/else\s+gh release create "\$TAG" --draft --verify-tag/);
  });

  it("builds only the NSIS installer for a pre-release on Windows (WiX needs a numeric version)", () => {
    const t = jobText(desktopJob() ?? { lines: [] });
    expect(t).toMatch(/prerelease_args:\s*--bundles nsis/);
    expect(t).toMatch(
      /needs\.check\.outputs\.prerelease == 'true' && matrix\.prerelease_args \|\| matrix\.args/,
    );
  });

  it("creates a draft, uploads with --clobber, then publishes (AC-E1ZJ2N.7)", () => {
    const t = jobText(publishJob() ?? { lines: [] });
    const draft = t.search(/--draft(?!=)/);
    const upload = t.search(/gh release upload[^\n]*--clobber|--clobber[^\n]*gh release upload/);
    const publish = t.search(/--draft=false/);
    expect(draft, "release is not created as a draft").toBeGreaterThanOrEqual(0);
    expect(upload, "assets are not uploaded with --clobber").toBeGreaterThanOrEqual(0);
    expect(publish, "the draft is never published").toBeGreaterThanOrEqual(0);
    expect(draft).toBeLessThan(upload);
    expect(upload).toBeLessThan(publish);
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
    expect(jobText(writers[0] ?? { lines: [] })).toMatch(/gh release create/);
  });

  it("pins every action to a full commit SHA with a version comment", () => {
    const uses = lines().filter((l) => /^\s*-?\s*uses:/.test(l) && !isComment(l));
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
