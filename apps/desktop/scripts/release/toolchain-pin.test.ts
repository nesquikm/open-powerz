// FR E1ZJ2M: one exact Rust toolchain, pinned in rust-toolchain.toml at the
// repository root. Workflows install from that file and never name their own
// Rust version or channel; this scan is the gate check that enforces it.
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, it } from "vitest";
import {
  isExactStableVersion,
  readToolchainPin,
  scanWorkflow,
  scanWorkflowDir,
} from "./toolchain-pin";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, "../../../..");
const scriptPath = join(here, "toolchain-pin.ts");

function runCli(dir: string): ReturnType<typeof spawnSync> {
  const r = spawnSync("bun", [scriptPath, dir], { encoding: "utf8", timeout: 30_000 });
  expect(r.error).toBeUndefined();
  return r;
}

function expectSingleFinding(text: string, line: number): void {
  const findings = scanWorkflow(text, "ci.yml");
  expect(findings).toHaveLength(1);
  const [f] = findings;
  expect(f?.file).toBe("ci.yml");
  expect(f?.line).toBe(line);
  expect(f?.reason.trim().length).toBeGreaterThan(0);
}

describe("scanWorkflow: flagged patterns (AC-E1ZJ2M.2)", () => {
  it("flags a toolchain: key", () => {
    const text = [
      "jobs:",
      "  build:",
      "    steps:",
      "      - uses: some/action@v1",
      "        with:",
      "          toolchain: 1.99.1",
    ].join("\n");
    const findings = scanWorkflow(text, "ci.yml");
    expect(findings.map((f) => f.line)).toContain(6);
    expect(findings.every((f) => f.file === "ci.yml" && f.reason.length > 0)).toBe(true);
  });

  it("flags any dtolnay/rust-toolchain use", () => {
    const text = [
      "steps:",
      "  - uses: actions/checkout@v4",
      "  - uses: dtolnay/rust-toolchain@master",
    ].join("\n");
    expectSingleFinding(text, 3);
  });

  it("flags dtolnay/rust-toolchain together with its toolchain: key", () => {
    const text = [
      "steps:",
      "  - uses: dtolnay/rust-toolchain@b725eda525362e7cf34af9a7243cac1e3f01a7ec",
      "    with:",
      "      toolchain: stable",
    ].join("\n");
    const lines = scanWorkflow(text, "release.yml").map((f) => f.line);
    expect(lines).toContain(2);
    expect(lines).toContain(4);
  });

  it("flags rustup toolchain install with an explicit toolchain", () => {
    expectSingleFinding("steps:\n  - run: rustup toolchain install stable", 2);
  });

  it("flags rustup install <name>", () => {
    expectSingleFinding("steps:\n  - run: rustup install 1.99.1", 2);
  });

  it("flags rustup default <name>", () => {
    expectSingleFinding("steps:\n  - run: rustup default nightly", 2);
  });

  it("flags rustup override set <name>", () => {
    expectSingleFinding("steps:\n  - run: rustup override set beta", 2);
  });

  it("flags cargo +<toolchain>", () => {
    expectSingleFinding("steps:\n  - run: cargo +nightly fmt --all --check", 2);
  });

  it("flags RUSTUP_TOOLCHAIN", () => {
    const text = ["env:", "  RUSTUP_TOOLCHAIN: stable", "jobs: {}"].join("\n");
    expectSingleFinding(text, 2);
  });

  it("flags a hard-coded toolchain inside a multi-line run block", () => {
    const text = [
      "steps:",
      "  - run: |",
      "      echo hi",
      "      rustup toolchain install 1.98.0 --profile minimal",
    ].join("\n");
    expectSingleFinding(text, 4);
  });

  it("reports 1-based line numbers for several findings in order", () => {
    const text = [
      "steps:",
      "  - run: rustup default stable",
      "  - run: echo ok",
      "  - run: cargo +1.99.1 build",
    ].join("\n");
    expect(scanWorkflow(text, "x.yml").map((f) => f.line)).toEqual([2, 4]);
  });
});

describe("scanWorkflow: allowed forms", () => {
  it("returns no findings for a clean workflow", () => {
    const text = [
      "name: CI",
      "on: [push]",
      "jobs:",
      "  gate:",
      "    runs-on: ubuntu-latest",
      "    steps:",
      "      - uses: actions/checkout@v4",
      "      - run: rustup show",
      "      - run: cargo fmt --all --check",
      "      - run: cargo clippy --workspace --all-targets --locked -- -D warnings",
      "      - run: cargo test --workspace --locked",
    ].join("\n");
    expect(scanWorkflow(text, "ci.yml")).toEqual([]);
  });

  it("allows bare rustup toolchain install (reads rust-toolchain.toml)", () => {
    expect(scanWorkflow("steps:\n  - run: rustup toolchain install", "ci.yml")).toEqual([]);
  });

  it("allows bare rustup toolchain install followed by flags or &&", () => {
    const text = [
      "steps:",
      "  - run: rustup toolchain install --no-self-update",
      "  - run: rustup toolchain install && rustup show",
    ].join("\n");
    expect(scanWorkflow(text, "ci.yml")).toEqual([]);
  });

  it("allows rustup target add <target>", () => {
    const text = [
      "steps:",
      "  - run: rustup target add aarch64-apple-darwin x86_64-apple-darwin",
      "  - run: rustup target add wasm32-unknown-unknown",
    ].join("\n");
    expect(scanWorkflow(text, "release.yml")).toEqual([]);
  });

  it("ignores full-line comments", () => {
    const text = [
      "steps:",
      "  # toolchain: stable",
      "  # - uses: dtolnay/rust-toolchain@master",
      "  # rustup default nightly; cargo +nightly fmt; RUSTUP_TOOLCHAIN=beta",
      "  - run: cargo build",
    ].join("\n");
    expect(scanWorkflow(text, "ci.yml")).toEqual([]);
  });

  it("ignores trailing comments", () => {
    const text = [
      "steps:",
      "  - run: rustup toolchain install # never toolchain: stable here",
      "  - uses: actions/checkout@v4 # not dtolnay/rust-toolchain",
    ].join("\n");
    expect(scanWorkflow(text, "ci.yml")).toEqual([]);
  });
});

describe("scanWorkflowDir", () => {
  const made: string[] = [];
  afterEach(() => {
    for (const d of made.splice(0)) rmSync(d, { recursive: true, force: true });
  });
  function fixtureDir(files: Record<string, string>): string {
    const dir = mkdtempSync(join(tmpdir(), "toolchain-pin-"));
    made.push(dir);
    for (const [name, body] of Object.entries(files)) writeFileSync(join(dir, name), body);
    return dir;
  }

  it("scans .yml and .yaml files and reports each file's findings", () => {
    const dir = fixtureDir({
      "ci.yml": "steps:\n  - run: rustup default stable\n",
      "release.yaml": "steps:\n  - run: echo\n  - run: cargo +nightly build\n",
      "clean.yml": "steps:\n  - run: cargo test\n",
      "notes.txt": "rustup default stable\n",
    });
    const findings = scanWorkflowDir(dir);
    expect(findings).toHaveLength(2);
    const byFile = new Map(findings.map((f) => [f.file.split(/[\\/]/).pop(), f.line]));
    expect(byFile.get("ci.yml")).toBe(2);
    expect(byFile.get("release.yaml")).toBe(3);
  });

  it("CLI exits 1 and prints file:line per finding when a workflow pins Rust", () => {
    const dir = fixtureDir({ "ci.yml": "steps:\n  - run: rustup default stable\n" });
    const r = runCli(dir);
    expect(r.status).toBe(1);
    expect(`${r.stdout}${r.stderr}`).toMatch(/ci\.yml:2/);
  });

  it("CLI exits 0 on a clean directory", () => {
    const dir = fixtureDir({ "ci.yml": "steps:\n  - run: cargo test\n" });
    const r = runCli(dir);
    expect(r.status).toBe(0);
  });
});

describe("scanWorkflow: hardening", () => {
  it("reads a rustup command continued with a trailing backslash", () => {
    expectSingleFinding(
      "steps:\n  - run: |\n      rustup toolchain install \\\n        nightly\n",
      3,
    );
  });

  it("does not take a continuation, a quote or a redirection for a toolchain name", () => {
    const text = [
      "steps:",
      "  - run: |",
      "      rustup toolchain install \\",
      "        --profile minimal",
      '  - run: "rustup toolchain install"',
      "  - run: rustup toolchain install 2>&1",
      "  - run: rustup toolchain install > /dev/null",
    ].join("\n");
    expect(scanWorkflow(text, "ci.yml")).toEqual([]);
  });

  it.each([
    ["rustup update stable", 2],
    ["rustup run nightly cargo build", 2],
  ])("flags `%s`", (cmd, line) => {
    expectSingleFinding(`steps:\n  - run: ${cmd}\n`, line);
  });

  it("allows a bare rustup update", () => {
    expect(scanWorkflow("steps:\n  - run: rustup update\n", "ci.yml")).toEqual([]);
  });

  it("flags a toolchain input in a flow mapping or as a quoted key", () => {
    expectSingleFinding(
      "steps:\n  - uses: some/rust-setup@abc\n    with: { toolchain: stable }\n",
      3,
    );
    expectSingleFinding('steps:\n  - with:\n      "toolchain": stable\n', 3);
  });

  it("does not treat a # inside quotes as a comment", () => {
    expectSingleFinding('steps:\n  - run: echo "x #y"; rustup default stable\n', 2);
  });
});

describe("isExactStableVersion", () => {
  it.each(["1.99.1", "1.100.0", "1.96.0"])("accepts %s", (v) => {
    expect(isExactStableVersion(v)).toBe(true);
  });
  it.each(["stable", "beta", "nightly", "1.99", "nightly-2026-01-01", "", "1.99.1-beta"])(
    "rejects %s",
    (v) => {
      expect(isExactStableVersion(v)).toBe(false);
    },
  );
});

describe("readToolchainPin", () => {
  it("parses channel, profile and components from the [toolchain] table", () => {
    const text = [
      "# pinned toolchain",
      "[toolchain]",
      'channel = "1.99.1"',
      'profile = "minimal"',
      'components = ["rustfmt", "clippy"]',
    ].join("\n");
    expect(readToolchainPin(text)).toEqual({
      channel: "1.99.1",
      profile: "minimal",
      components: ["rustfmt", "clippy"],
    });
  });

  it("reads literal strings and a components array over several lines", () => {
    const text = [
      "[toolchain]",
      "channel = '1.99.1'",
      "components = [",
      '  "rustfmt",',
      "  'clippy',",
      "]",
    ].join("\n");
    expect(readToolchainPin(text)).toEqual({
      channel: "1.99.1",
      components: ["rustfmt", "clippy"],
    });
  });

  it("returns an empty component list and no profile when absent", () => {
    const pin = readToolchainPin('[toolchain]\nchannel = "stable"\n');
    expect(pin.channel).toBe("stable");
    expect(pin.profile).toBeUndefined();
    expect(pin.components).toEqual([]);
  });
});

function versionAtLeast(v: string, min: [number, number, number]): boolean {
  const parts = v.split(".").map(Number);
  for (let i = 0; i < 3; i++) {
    const a = parts[i] ?? 0;
    const b = min[i] ?? 0;
    if (a !== b) return a > b;
  }
  return true;
}

describe("repository gate", () => {
  it("AC-E1ZJ2M.1: rust-toolchain.toml pins an exact stable >= 1.99.1, minimal, rustfmt + clippy", () => {
    const file = join(repoRoot, "rust-toolchain.toml");
    expect(existsSync(file)).toBe(true);
    const pin = readToolchainPin(readFileSync(file, "utf8"));
    expect(isExactStableVersion(pin.channel)).toBe(true);
    expect(versionAtLeast(pin.channel, [1, 99, 1])).toBe(true);
    expect(pin.profile).toBe("minimal");
    expect(pin.components).toEqual(expect.arrayContaining(["rustfmt", "clippy"]));
  });

  it("AC-E1ZJ2M.2: no workflow in .github/workflows names its own Rust version or channel", () => {
    const findings = scanWorkflowDir(join(repoRoot, ".github", "workflows"));
    expect(findings).toEqual([]);
  });

  it("AC-E1ZJ2M.4: README says where the toolchain is pinned and how to move it", () => {
    const readme = readFileSync(join(repoRoot, "README.md"), "utf8");
    const start = readme.indexOf("### Rust toolchain");
    expect(start).toBeGreaterThanOrEqual(0);
    const section = readme.slice(start).split(/\n#{2,3} /)[0] ?? "";
    expect(section).toContain("rust-toolchain.toml");
    expect(section).toMatch(/lints/i);
    expect(section).toMatch(/pull request|\bPR\b/);
  });
});

// FR 4PJRVN (AC-4PJRVN.4): an apostrophe that never closes must not hide a trailing
// comment from the comment stripper, or the comment's words get flagged.
describe("scanWorkflow: unpaired apostrophes (AC-4PJRVN.4)", () => {
  it("drops a trailing comment after an unpaired apostrophe in a step name", () => {
    const text = [
      "steps:",
      "  - name: Rust's toolchain # never rustup default stable here",
      "    run: rustup toolchain install",
    ].join("\n");
    expect(scanWorkflow(text, "ci.yml")).toEqual([]);
  });

  it("drops a trailing comment after an unpaired apostrophe in a run line", () => {
    const text = [
      "steps:",
      "  - run: echo don\\'t pin here # cargo +nightly fmt, RUSTUP_TOOLCHAIN=beta",
      "  - run: echo it's built # toolchain: stable",
    ].join("\n");
    expect(scanWorkflow(text, "ci.yml")).toEqual([]);
  });

  it("drops a trailing comment after an apostrophe inside a multi-line run block", () => {
    const text = [
      "steps:",
      "  - run: |",
      '      echo "the pin\'s in rust-toolchain.toml"',
      "      echo that's all # rustup override set nightly",
      "      cargo build --locked",
    ].join("\n");
    expect(scanWorkflow(text, "ci.yml")).toEqual([]);
  });

  it("still flags a real pin on a line with an unpaired apostrophe", () => {
    expectSingleFinding("steps:\n  - run: echo it's here && rustup default nightly\n", 2);
  });

  it("still flags a real pin after a paired quote that holds a #", () => {
    expectSingleFinding("steps:\n  - run: echo 'a # b' && rustup default stable # it's fine\n", 2);
  });
});
