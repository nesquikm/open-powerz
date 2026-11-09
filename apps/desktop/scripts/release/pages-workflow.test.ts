/**
 * FR QFTCX1: the release deploys the web edition to GitHub Pages, and a maintainer can
 * deploy main by hand.
 *
 * AC-QFTCX1.5: release.yml has a `pages` job that needs the check job, is skipped on pull
 * request dry runs, builds the site and deploys it with actions/upload-pages-artifact and
 * actions/deploy-pages (permissions `pages: write` + `id-token: write`, environment
 * `github-pages`).
 * AC-QFTCX1.6: pages.yml (`workflow_dispatch`) deploys main the same way; both share one
 * concurrency group for Pages. The deploy steps live in one reusable workflow
 * (`workflow_call`) that both call (Technical Design).
 *
 * Line-based structural checks (no YAML library is available), in the style of
 * release-workflow.test.ts; actionlint runs over the files when it is installed.
 */

import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const REPO_ROOT = fileURLToPath(new URL("../../../../", import.meta.url));
const WORKFLOWS = join(REPO_ROOT, ".github/workflows");

const read = (name: string): string => {
  const p = join(WORKFLOWS, name);
  return existsSync(p) ? readFileSync(p, "utf8") : "";
};
const linesOf = (name: string): string[] => read(name).split(/\r?\n/);

const indentOf = (line: string): number => line.length - line.trimStart().length;
const isComment = (line: string): boolean => line.trim().startsWith("#");
const noComments = (src: string[]): string => src.filter((l) => !isComment(l)).join("\n");

/** The lines of the YAML block opened by the key on line `i` of `src`. */
function blockAfter(src: string[], i: number): string[] {
  const base = indentOf(src[i] ?? "");
  const out: string[] = [];
  for (const line of src.slice(i + 1)) {
    if (line.trim() !== "" && indentOf(line) <= base) break;
    out.push(line);
  }
  return out;
}

/** The block of the top-level key `key:`, or []. */
function topBlock(src: string[], key: string): string[] {
  const at = src.findIndex((l) => new RegExp(`^["']?${key}["']?:\\s*$`).test(l));
  return at < 0 ? [] : blockAfter(src, at);
}

/** The jobs as { id, lines }. */
function jobs(src: string[]): { id: string; lines: string[] }[] {
  const body = topBlock(src, "jobs");
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

/** The local reusable workflow a job calls (`uses: ./.github/workflows/x.yml`), or null. */
function reusableOf(jobLines: string[]): string | null {
  return (
    /^\s*uses:\s*\.\/\.github\/workflows\/([\w.-]+\.ya?ml)\s*$/m.exec(noComments(jobLines))?.[1] ??
    null
  );
}

/** A caller job's text plus the reusable workflow it calls: everything the deploy runs. */
function deployText(jobLines: string[]): string {
  const reusable = reusableOf(jobLines);
  return [noComments(jobLines), reusable ? noComments(linesOf(reusable)) : ""].join("\n");
}

/** The `group:` of the `concurrency:` block (or `concurrency: <group>`) in `src`, at any
 * depth; the first one found. */
function concurrencyGroup(src: string[]): string | null {
  for (let i = 0; i < src.length; i++) {
    const line = src[i] ?? "";
    if (isComment(line)) continue;
    const inline = /^\s*concurrency:\s*(\S.*)$/.exec(line);
    if (inline) return inline[1]!.trim().replace(/^["']|["']$/g, "");
    if (/^\s*concurrency:\s*$/.test(line)) {
      const g = blockAfter(src, i)
        .map((l) => /^\s*group:\s*(.+)$/.exec(l)?.[1])
        .find((v) => v !== undefined);
      if (g) return g.trim().replace(/^["']|["']$/g, "");
    }
  }
  return null;
}

/** The Pages concurrency group a deploy through `callerJob` runs under: the reusable
 * workflow's, else the caller job's, else (`top` only) the caller workflow's. */
function pagesGroup(callerJob: string[], callerTop: string[] | null): string | null {
  const reusable = reusableOf(callerJob);
  return (
    (reusable ? concurrencyGroup(linesOf(reusable)) : null) ??
    concurrencyGroup(callerJob) ??
    (callerTop ? concurrencyGroup(callerTop) : null)
  );
}

const releasePages = () => jobs(linesOf("release.yml")).find((j) => j.id === "pages");
const manualJobs = () => jobs(linesOf("pages.yml"));
/** The pages.yml job that deploys (calls the reusable workflow or deploys itself). */
const manualDeploy = () =>
  manualJobs().find(
    (j) => reusableOf(j.lines) !== null || /deploy-pages@/.test(noComments(j.lines)),
  );

/** The job-level `permissions:` block text of a job. */
function permissionsOf(jobLines: string[]): string {
  const at = jobLines.findIndex((l) => /^\s*permissions:\s*$/.test(l));
  return at < 0 ? "" : noComments(blockAfter(jobLines, at));
}

/** The jobs a job needs (`needs: x`, `needs: [x, y]` or a list block). */
function needsOf(jobLines: string[]): string[] {
  const at = jobLines.findIndex((l) => /^\s*needs:/.test(l));
  if (at < 0) return [];
  const head = (jobLines[at] ?? "").replace(/^\s*needs:\s*/, "");
  if (head.trim() !== "")
    return head
      .replace(/[[\]]/g, "")
      .split(",")
      .map((s) => s.trim());
  return blockAfter(jobLines, at)
    .map((l) => /^\s*-\s*([\w-]+)/.exec(l)?.[1])
    .filter((v): v is string => v !== undefined);
}

function expectDeploys(text: string): void {
  expect(text).toMatch(/build:pages/);
  expect(text).toMatch(/uses:\s*actions\/upload-pages-artifact@[0-9a-f]{40}\s+#\s*\S/);
  expect(text).toMatch(/uses:\s*actions\/deploy-pages@[0-9a-f]{40}\s+#\s*\S/);
  expect(text).toMatch(/path:\s*["']?(?:\.\/)?apps\/desktop\/dist-pages\/?["']?/);
  expect(text).toMatch(/environment:\s*(?:github-pages\b|\n\s+name:\s*github-pages\b)/);
  const build = text.search(/build:pages/);
  const upload = text.search(/actions\/upload-pages-artifact@/);
  const deploy = text.search(/actions\/deploy-pages@/);
  expect(build).toBeLessThan(upload);
  expect(upload).toBeLessThan(deploy);
}

describe("release.yml deploys the site on a release (AC-QFTCX1.5)", () => {
  it("has a pages job that needs the check job", () => {
    const j = releasePages();
    expect(j, "release.yml has no `pages` job").toBeDefined();
    expect(needsOf(j?.lines ?? [])).toContain("check");
  });

  it("skips the pages job on pull request dry runs", () => {
    const cond = (releasePages()?.lines ?? [])
      .filter((l) => !isComment(l))
      .find((l) => /^\s*if:/.test(l));
    expect(cond, "the pages job has no `if:`").toBeDefined();
    expect(cond).toMatch(/github\.event_name\s*!=\s*'pull_request'/);
  });

  it("grants pages: write and id-token: write to the pages job, and no contents: write", () => {
    const perms = permissionsOf(releasePages()?.lines ?? []);
    expect(perms).toMatch(/pages:\s*write/);
    expect(perms).toMatch(/id-token:\s*write/);
    expect(perms).not.toMatch(/contents:\s*write/);
  });

  it("builds the site and deploys it with upload-pages-artifact and deploy-pages", () => {
    expectDeploys(deployText(releasePages()?.lines ?? []));
  });

  it("keeps the top-level permissions read-only", () => {
    const top = noComments(topBlock(linesOf("release.yml"), "permissions"));
    expect(top).toMatch(/contents:\s*read/);
    expect(top).not.toMatch(/write/);
  });
});

describe("pages.yml deploys main by hand (AC-QFTCX1.6)", () => {
  it("exists and runs on workflow_dispatch only, never for a pull request", () => {
    expect(read("pages.yml"), ".github/workflows/pages.yml is missing").not.toBe("");
    const on = noComments(topBlock(linesOf("pages.yml"), "on"));
    const inline = linesOf("pages.yml").find((l) => /^["']?on["']?:\s*\S/.test(l)) ?? "";
    expect(`${on}\n${inline}`).toMatch(/workflow_dispatch/);
    expect(`${on}\n${inline}`).not.toMatch(/pull_request/);
  });

  it("deploys the main branch", () => {
    const j = manualDeploy();
    expect(j, "pages.yml has no deploying job").toBeDefined();
    const all = `${noComments(linesOf("pages.yml"))}\n${deployText(j?.lines ?? [])}`;
    expect(all).toMatch(/refs\/heads\/main\b|ref:\s*["']?main["']?\s*$/m);
  });

  it("deploys the same way as the release", () => {
    expectDeploys(deployText(manualDeploy()?.lines ?? []));
    const perms = permissionsOf(manualDeploy()?.lines ?? []);
    expect(perms).toMatch(/pages:\s*write/);
    expect(perms).toMatch(/id-token:\s*write/);
  });

  it("calls the same reusable workflow as the release's pages job", () => {
    const fromRelease = reusableOf(releasePages()?.lines ?? []);
    const fromManual = reusableOf(manualDeploy()?.lines ?? []);
    expect(fromRelease, "the release pages job calls a local reusable workflow").not.toBeNull();
    expect(fromManual).toBe(fromRelease);
    const on = noComments(topBlock(linesOf(fromRelease ?? ""), "on"));
    expect(on).toMatch(/workflow_call/);
  });

  it("shares one Pages concurrency group with the release", () => {
    const release = pagesGroup(releasePages()?.lines ?? [], null);
    const manual = pagesGroup(manualDeploy()?.lines ?? [], linesOf("pages.yml"));
    expect(release, "the release pages deploy has a concurrency group").not.toBeNull();
    expect(release).not.toMatch(/release-/);
    expect(manual).toBe(release);
  });
});

describe("the Pages workflows are well formed (AC-QFTCX1.5, AC-QFTCX1.6)", () => {
  const files = (): string[] => {
    const reusable = reusableOf(releasePages()?.lines ?? []);
    return ["release.yml", "pages.yml", ...(reusable ? [reusable] : [])];
  };

  it("pins every remote action to a full commit SHA with a version comment", () => {
    const offenders = files().flatMap((f) =>
      linesOf(f)
        .filter((l) => /^\s*-?\s*uses:/.test(l) && !isComment(l))
        .filter((l) => !/uses:\s*\.\//.test(l))
        .filter((l) => !/uses:\s*[\w.-]+\/[\w./-]+@[0-9a-f]{40}\s+#\s+\S/.test(l))
        .map((l) => `${f}: ${l.trim()}`),
    );
    expect(read("pages.yml")).not.toBe("");
    expect(offenders).toEqual([]);
  });

  const actionlint = (() => {
    try {
      execFileSync("actionlint", ["-version"], { stdio: "pipe" });
      return true;
    } catch {
      return false;
    }
  })();

  it.skipIf(!actionlint)("passes actionlint", () => {
    const paths = files().map((f) => join(WORKFLOWS, f));
    expect(() =>
      execFileSync("actionlint", paths, { cwd: REPO_ROOT, stdio: "pipe" }),
    ).not.toThrow();
  });
});
