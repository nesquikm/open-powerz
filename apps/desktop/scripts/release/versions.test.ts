// FR E1ZJ2N: before any build, the release check job compares the tag with every
// release file and fails with a clear message on a mismatch (AC-E1ZJ2N.2); a
// pre-release suffix gives a pre-release, and the title/notes come from the
// CHANGELOG (AC-E1ZJ2N.5).
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, it } from "vitest";
import { checkRelease, parseTag } from "./versions";

const here = dirname(fileURLToPath(import.meta.url));
const scriptPath = join(here, "versions.ts");

const cargoToml = (v: string): string => `[workspace]
resolver = "3"
members = ["crates/powerz-protocol", "crates/powerz-cli"]

[workspace.package]
version = "${v}"
edition = "2024"
rust-version = "1.96"

[workspace.dependencies]
powerz-protocol = { path = "crates/powerz-protocol", version = "${v}" }
serde = { version = "1.0.200" }
`;

const packageJson = (v: string): string =>
  `${JSON.stringify({ name: "open-powerz-desktop", private: true, version: v, type: "module", dependencies: { svelte: "5.0.0" } }, null, 2)}\n`;

const tauriConf = (v: string): string =>
  `${JSON.stringify({ $schema: "https://schema.tauri.app/config/2", productName: "open-powerz", version: v, identifier: "dev.openpowerz.app" }, null, 2)}\n`;

const changelog = (v: string, codename = "Special Delivery"): string => `# Changelog

## [${v}] — 2026-10-28 — "${codename}"

### Added
- Release builds on a version tag (E1ZJ2N)

## [0.5.0] — 2026-10-26 — "Flash Gordon"

### Added
- Firmware update
`;

function inputs(v: string, over: Partial<Parameters<typeof checkRelease>[0]> = {}) {
  return {
    tag: `v${v}`,
    cargoToml: cargoToml(v),
    packageJson: packageJson(v),
    tauriConf: tauriConf(v),
    changelog: changelog(v),
    ...over,
  };
}

describe("parseTag (AC-E1ZJ2N.1, AC-E1ZJ2N.5)", () => {
  it("accepts a release tag", () => {
    expect(parseTag("v1.2.3")).toEqual({ version: "1.2.3", prerelease: false });
  });

  it("accepts a pre-release tag and marks it as a pre-release", () => {
    expect(parseTag("v1.2.3-rc.1")).toEqual({ version: "1.2.3-rc.1", prerelease: true });
  });

  it.each(["1.2.3", "v1.2", "vfoo", ""])("rejects %j with a clear message", (tag) => {
    expect(() => parseTag(tag)).toThrow(Error);
    try {
      parseTag(tag);
    } catch (e) {
      const msg = (e as Error).message;
      expect(msg).toContain(JSON.stringify(tag).slice(1, -1));
      expect(msg).toMatch(/vX\.Y\.Z|v\d|tag/i);
    }
  });
});

describe("checkRelease (AC-E1ZJ2N.2)", () => {
  it("returns no messages when every file matches the tag", () => {
    expect(checkRelease(inputs("0.6.0"))).toEqual([]);
  });

  it("returns no messages for a matching pre-release", () => {
    expect(checkRelease(inputs("0.6.0-rc.1"))).toEqual([]);
  });

  it("reports a Cargo workspace version mismatch, naming the file and both versions", () => {
    const msgs = checkRelease(inputs("0.6.0", { cargoToml: cargoToml("0.5.0") }));
    expect(msgs).toHaveLength(1);
    expect(msgs[0]).toContain("Cargo.toml");
    expect(msgs[0]).toContain("0.6.0");
    expect(msgs[0]).toContain("0.5.0");
  });

  it("reads the [workspace.package] version, not a dependency's version", () => {
    // workspace.package says 0.5.0, but a dependency line says 0.6.0: still a mismatch.
    const toml = cargoToml("0.5.0").replace(
      'serde = { version = "1.0.200" }',
      'other = { version = "0.6.0" }',
    );
    const msgs = checkRelease(inputs("0.6.0", { cargoToml: toml }));
    expect(msgs).toHaveLength(1);
    expect(msgs[0]).toContain("Cargo.toml");
  });

  it("reports a desktop package.json mismatch", () => {
    const msgs = checkRelease(inputs("0.6.0", { packageJson: packageJson("0.5.1") }));
    expect(msgs).toHaveLength(1);
    expect(msgs[0]).toContain("package.json");
    expect(msgs[0]).toContain("0.6.0");
    expect(msgs[0]).toContain("0.5.1");
  });

  it("reports a tauri.conf.json mismatch", () => {
    const msgs = checkRelease(inputs("0.6.0", { tauriConf: tauriConf("0.4.0") }));
    expect(msgs).toHaveLength(1);
    expect(msgs[0]).toContain("tauri.conf.json");
    expect(msgs[0]).toContain("0.6.0");
    expect(msgs[0]).toContain("0.4.0");
  });

  it("reports a missing CHANGELOG section, naming the version and CHANGELOG.md", () => {
    const msgs = checkRelease(inputs("0.6.0", { changelog: changelog("0.5.9") }));
    expect(msgs).toHaveLength(1);
    expect(msgs[0]).toContain("CHANGELOG.md");
    expect(msgs[0]).toContain("0.6.0");
  });

  it("does not accept the final version's section for a pre-release tag", () => {
    const msgs = checkRelease(inputs("0.6.0-rc.1", { changelog: changelog("0.6.0") }));
    expect(msgs).toHaveLength(1);
    expect(msgs[0]).toContain("CHANGELOG.md");
    expect(msgs[0]).toContain("0.6.0-rc.1");
  });

  it("reports an empty CHANGELOG section, since it becomes the release notes", () => {
    const empty = "## [0.6.0] — 2026-10-28\n\n## [0.5.0] — 2026-10-26\n\n- old\n";
    const msgs = checkRelease(inputs("0.6.0", { changelog: empty }));
    expect(msgs).toHaveLength(1);
    expect(msgs[0]).toMatch(/CHANGELOG\.md/);
    expect(msgs[0]).toMatch(/empty/);
  });

  it("reports every mismatch at once, one message each", () => {
    const msgs = checkRelease({
      tag: "v0.6.0",
      cargoToml: cargoToml("0.5.0"),
      packageJson: packageJson("0.5.0"),
      tauriConf: tauriConf("0.5.0"),
      changelog: changelog("0.5.0"),
    });
    expect(msgs).toHaveLength(4);
  });
});

describe("versions.ts CLI (AC-E1ZJ2N.2, AC-E1ZJ2N.5)", () => {
  const dirs: string[] = [];
  afterEach(() => {
    for (const d of dirs.splice(0)) rmSync(d, { recursive: true, force: true });
  });

  function fixture(files: { cargo: string; pkg: string; tauri: string; changelog: string }) {
    const root = mkdtempSync(join(tmpdir(), "opz-release-"));
    dirs.push(root);
    mkdirSync(join(root, "apps/desktop/src-tauri"), { recursive: true });
    writeFileSync(join(root, "Cargo.toml"), files.cargo);
    writeFileSync(join(root, "apps/desktop/package.json"), files.pkg);
    writeFileSync(join(root, "apps/desktop/src-tauri/tauri.conf.json"), files.tauri);
    writeFileSync(join(root, "CHANGELOG.md"), files.changelog);
    return { root, notes: join(root, "out", "notes.md") };
  }

  const matching = (v: string) => ({
    cargo: cargoToml(v),
    pkg: packageJson(v),
    tauri: tauriConf(v),
    changelog: changelog(v),
  });

  function run(args: string[]) {
    const r = spawnSync("bun", [scriptPath, ...args], { encoding: "utf8", timeout: 30_000 });
    expect(r.error).toBeUndefined();
    return r;
  }

  it("prints the outputs and writes the notes file when everything matches", () => {
    const { root, notes } = fixture(matching("0.6.0"));
    mkdirSync(dirname(notes), { recursive: true });
    const r = run(["v0.6.0", "--root", root, "--notes", notes]);
    expect(r.stderr).toBe("");
    expect(r.status).toBe(0);
    const lines = r.stdout.split(/\r?\n/);
    expect(lines).toContain("tag=v0.6.0");
    expect(lines).toContain("version=0.6.0");
    expect(lines).toContain("prerelease=false");
    expect(lines).toContain('title=v0.6.0 — "Special Delivery"');
    expect(existsSync(notes)).toBe(true);
    expect(readFileSync(notes, "utf8").trim()).toBe(
      "### Added\n- Release builds on a version tag (E1ZJ2N)",
    );
  });

  it("marks a pre-release tag", () => {
    const { root, notes } = fixture(matching("0.6.0-rc.1"));
    mkdirSync(dirname(notes), { recursive: true });
    const r = run(["v0.6.0-rc.1", "--root", root, "--notes", notes]);
    expect(r.status).toBe(0);
    expect(r.stdout.split(/\r?\n/)).toContain("prerelease=true");
  });

  it("exits 1 with every mismatch on stderr", () => {
    const { root, notes } = fixture({
      ...matching("0.6.0"),
      pkg: packageJson("0.5.0"),
      changelog: changelog("0.5.0"),
    });
    mkdirSync(dirname(notes), { recursive: true });
    const r = run(["v0.6.0", "--root", root, "--notes", notes]);
    expect(r.status).toBe(1);
    expect(r.stderr).toContain("package.json");
    expect(r.stderr).toContain("CHANGELOG.md");
    expect(r.stderr).toContain("0.5.0");
    expect(r.stdout).not.toContain("tag=");
  });

  it("exits 1 for a malformed tag", () => {
    const { root, notes } = fixture(matching("0.6.0"));
    const r = run(["0.6.0", "--root", root, "--notes", notes]);
    expect(r.status).toBe(1);
    expect(r.stderr).toContain("0.6.0");
  });

  it("names a release file it cannot read instead of a missing version", () => {
    const { root } = fixture(matching("0.6.0"));
    rmSync(join(root, "apps/desktop/src-tauri/tauri.conf.json"));
    const r = run(["v0.6.0", "--root", root]);
    expect(r.status).toBe(1);
    expect(r.stdout).toBe("");
    expect(r.stderr).toMatch(/tauri\.conf\.json: cannot read/);
  });

  it("uses v<cargo version> as the tag with --from-cargo", () => {
    const { root, notes } = fixture(matching("0.7.2"));
    mkdirSync(dirname(notes), { recursive: true });
    const r = run(["--from-cargo", "--root", root, "--notes", notes]);
    expect(r.status).toBe(0);
    const lines = r.stdout.split(/\r?\n/);
    expect(lines).toContain("tag=v0.7.2");
    expect(lines).toContain("version=0.7.2");
    expect(existsSync(notes)).toBe(true);
  });
});
