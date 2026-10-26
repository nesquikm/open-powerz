// FR E1ZJ2N, AC-E1ZJ2N.10: an install guide for the unsigned release files, linked
// from the README and the docs index.
import { existsSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(here, "../../../..");
const GUIDE = join(REPO_ROOT, "docs/how-to/install-a-release.md");
const read = (p: string): string => (existsSync(p) ? readFileSync(p, "utf8") : "");

describe("install guide (AC-E1ZJ2N.10)", () => {
  it("exists at docs/how-to/install-a-release.md", () => {
    expect(existsSync(GUIDE), "no docs/how-to/install-a-release.md").toBe(true);
  });

  it("explains checking SHA256SUMS.txt", () => {
    expect(read(GUIDE)).toContain("SHA256SUMS.txt");
  });

  it("explains opening the unsigned app on macOS (Gatekeeper)", () => {
    const g = read(GUIDE);
    expect(g).toMatch(/Gatekeeper/);
    expect(g).toMatch(/Open Anyway|xattr/);
    expect(g).toMatch(/\.dmg/);
  });

  it("explains the Windows SmartScreen warning", () => {
    const g = read(GUIDE);
    expect(g).toMatch(/SmartScreen/);
    expect(g).toMatch(/Run anyway/);
    expect(g).toMatch(/\.msi|\.exe/);
  });

  it("covers the Linux AppImage and .deb with the udev rule from the Linux docs", () => {
    const g = read(GUIDE);
    expect(g).toMatch(/AppImage/);
    expect(g).toMatch(/\.deb\b/);
    expect(g).toMatch(/udev/);
    expect(g).toMatch(/\]\(\.\.\/linux\.md(#[\w-]+)?\)/);
  });

  it("covers the command line tool archives", () => {
    const g = read(GUIDE);
    expect(g).toMatch(/\.tar\.gz/);
    expect(g).toMatch(/\.zip/);
    expect(g).toMatch(/powerz/);
  });
});

describe("links to the release and the guide (AC-E1ZJ2N.10)", () => {
  it("README links the latest release and the install guide", () => {
    const readme = read(join(REPO_ROOT, "README.md"));
    expect(readme).toContain("https://github.com/nesquikm/open-powerz/releases/latest");
    expect(readme).toMatch(/\]\(docs\/how-to\/install-a-release\.md\)/);
  });

  it("docs/README.md lists the guide among the how-to guides", () => {
    const index = read(join(REPO_ROOT, "docs/README.md"));
    const at = index.search(/^## How-to guides/m);
    expect(at, "docs/README.md has no How-to guides section").toBeGreaterThanOrEqual(0);
    const rest = index.slice(at);
    const next = rest.slice(1).search(/^## /m);
    const section = next < 0 ? rest : rest.slice(0, next + 1);
    expect(section).toMatch(/\]\(how-to\/install-a-release\.md\)/);
  });
});
