// FR G30K4S: build:wasm picks the cargo it runs. With rustup on the PATH it builds
// through `rustup run <channel>` using the repository's pinned toolchain (AC-G30K4S.1);
// without rustup it checks the active rustc's sysroot for the wasm32 target and stops
// before cargo with a clear fix when the target is missing (AC-G30K4S.2). The command
// choice is a pure function with injected probes, so this test needs no Rust
// (AC-G30K4S.3).
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { type CargoProbes, WASM_TARGET, pickCargoCommand } from "./wasm-cargo";

const here = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = join(here, "../../..");
const TOOLCHAIN_TOML = readFileSync(join(REPO_ROOT, "rust-toolchain.toml"), "utf8");
const CHANNEL = /^\s*channel\s*=\s*"([^"]+)"/m.exec(TOOLCHAIN_TOML)?.[1] ?? "";

const CARGO_ARGS = ["build", "-p", "powerz-wasm", "--release", "--target", WASM_TARGET, "--locked"];

function probes(overrides: Partial<CargoProbes>): CargoProbes {
  return {
    hasRustup: () => false,
    toolchainFile: () => TOOLCHAIN_TOML,
    rustcSysroot: () => "/opt/homebrew/Cellar/rust/1.96.0",
    pathExists: () => false,
    ...overrides,
  };
}

describe("pickCargoCommand (FR G30K4S)", () => {
  it("names the wasm32-unknown-unknown target", () => {
    expect(WASM_TARGET).toBe("wasm32-unknown-unknown");
    expect(CHANNEL).toBe("1.99.1");
  });

  describe("AC-G30K4S.1: rustup on the PATH", () => {
    it("runs cargo as `rustup run <channel from rust-toolchain.toml> cargo …`", () => {
      const choice = pickCargoCommand(CARGO_ARGS, probes({ hasRustup: () => true }));
      expect(choice).toEqual({
        ok: true,
        command: "rustup",
        args: ["run", CHANNEL, "cargo", ...CARGO_ARGS],
      });
    });

    it("reads the channel from the toolchain file, not a hard-coded version", () => {
      const toml = '[toolchain]\nchannel = "1.42.7"\nprofile = "minimal"\n';
      const choice = pickCargoCommand(
        CARGO_ARGS,
        probes({ hasRustup: () => true, toolchainFile: () => toml }),
      );
      expect(choice).toEqual({
        ok: true,
        command: "rustup",
        args: ["run", "1.42.7", "cargo", ...CARGO_ARGS],
      });
    });

    it("never runs the bare cargo when rustup is present, even if the sysroot lacks the target", () => {
      const choice = pickCargoCommand(
        CARGO_ARGS,
        probes({ hasRustup: () => true, pathExists: () => false }),
      );
      expect(choice.ok).toBe(true);
      if (choice.ok) expect(choice.command).not.toBe("cargo");
    });
  });

  describe("AC-G30K4S.2: no rustup on the PATH", () => {
    it("stops before cargo when the active rustc has no wasm32 target, naming the target and the fix", () => {
      const seen: string[] = [];
      const choice = pickCargoCommand(
        CARGO_ARGS,
        probes({
          rustcSysroot: () => "/opt/homebrew/Cellar/rust/1.96.0",
          pathExists: (p) => {
            seen.push(p);
            return false;
          },
        }),
      );
      expect(choice.ok).toBe(false);
      if (!choice.ok) {
        expect(choice.message).toContain("wasm32-unknown-unknown");
        expect(choice.message).toContain("rustup target add wasm32-unknown-unknown");
      }
      expect(seen).toContain(
        join("/opt/homebrew/Cellar/rust/1.96.0", "lib", "rustlib", "wasm32-unknown-unknown"),
      );
    });

    it("stops with the same fix when rustc cannot report a sysroot", () => {
      const choice = pickCargoCommand(CARGO_ARGS, probes({ rustcSysroot: () => undefined }));
      expect(choice.ok).toBe(false);
      if (!choice.ok) expect(choice.message).toContain("rustup target add wasm32-unknown-unknown");
    });

    it("runs the bare cargo when the active rustc has the target installed", () => {
      const sysroot = "/usr/local/rust";
      const targetDir = join(sysroot, "lib", "rustlib", "wasm32-unknown-unknown");
      const choice = pickCargoCommand(
        CARGO_ARGS,
        probes({ rustcSysroot: () => sysroot, pathExists: (p) => p === targetDir }),
      );
      expect(choice).toEqual({ ok: true, command: "cargo", args: CARGO_ARGS });
    });
  });

  describe("AC-G30K4S.3: build-wasm.ts uses the command choice", () => {
    const script = readFileSync(join(here, "build-wasm.ts"), "utf8");

    it("imports pickCargoCommand instead of calling the bare cargo", () => {
      expect(script).toMatch(/from\s+"\.\/wasm-cargo"/);
      expect(script).toContain("pickCargoCommand(");
      expect(script).not.toMatch(/execFileSync\(\s*"cargo"/);
    });
  });
});
