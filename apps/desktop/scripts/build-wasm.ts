// Builds the web edition's WebAssembly module (FR QFTCWY, AC .4).
//
//   bun run build:wasm
//
// 1. rustup run <channel> cargo build -p powerz-wasm --release --target wasm32-unknown-unknown --locked
//    (<channel> from rust-toolchain.toml, RUSTC pinned to that toolchain; FR G30K4S). Without rustup
//    it runs the bare cargo only if the active rustc has the wasm32 target, otherwise it stops.
// 2. wasm-bindgen --target web --out-dir src/wasm ../../target/wasm32-unknown-unknown/release/powerz_wasm.wasm
//
// The wasm-bindgen CLI must be the exact version the crate pins (`wasm-bindgen = "=X.Y.Z"`
// in crates/powerz-wasm/Cargo.toml): the glue it writes only works with that runtime.
// src/wasm/ is git-ignored.
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { WASM_TARGET, pickCargoCommand } from "./wasm-cargo";

const DESKTOP = join(dirname(fileURLToPath(import.meta.url)), "..");
const REPO_ROOT = join(DESKTOP, "../..");
const OUT_DIR = "src/wasm";
const TARGET = WASM_TARGET;
const MODULE = join(REPO_ROOT, "target", TARGET, "release", "powerz_wasm.wasm");

function fail(message: string): never {
  console.error(`build:wasm: ${message}`);
  process.exit(1);
}

function pinnedVersion(): string {
  const manifest = readFileSync(join(REPO_ROOT, "crates/powerz-wasm/Cargo.toml"), "utf8");
  const pin = /^\s*wasm-bindgen\s*=\s*"=(\d+\.\d+\.\d+)"/m.exec(manifest)?.[1];
  if (pin === undefined)
    fail('crates/powerz-wasm/Cargo.toml has no exact wasm-bindgen = "=X.Y.Z" pin');
  return pin;
}

function installedVersion(): string | undefined {
  try {
    const out = execFileSync("wasm-bindgen", ["--version"], { encoding: "utf8" });
    return /(\d+\.\d+\.\d+)/.exec(out)?.[1];
  } catch {
    return undefined;
  }
}

const pin = pinnedVersion();
const installed = installedVersion();
const install = `cargo install wasm-bindgen-cli --version ${pin} --locked`;
if (installed === undefined)
  fail(`wasm-bindgen CLI not found on PATH; install it with: ${install}`);
if (installed !== pin) {
  fail(
    `wasm-bindgen CLI is ${installed}, but powerz-wasm pins ${pin}; install it with: ${install}`,
  );
}

function hasRustup(): boolean {
  try {
    execFileSync("rustup", ["--version"], { stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
}

function rustcSysroot(): string | undefined {
  try {
    return execFileSync("rustc", ["--print", "sysroot"], { encoding: "utf8" }).trim() || undefined;
  } catch {
    return undefined;
  }
}

// FR G30K4S: through rustup when present, so the pinned toolchain (with its wasm target) builds,
// not whatever cargo is first on the PATH.
const cargo = pickCargoCommand(
  ["build", "-p", "powerz-wasm", "--release", "--target", TARGET, "--locked"],
  {
    hasRustup,
    toolchainFile: () => readFileSync(join(REPO_ROOT, "rust-toolchain.toml"), "utf8"),
    rustcSysroot,
    pathExists: existsSync,
  },
);
if (!cargo.ok) fail(cargo.message);
// `rustup run` does not put the toolchain's bin first on the PATH, so cargo would still spawn
// the first `rustc` there (e.g. Homebrew's, without the wasm target). Point it at the pinned one.
const env = { ...process.env };
const channel = cargo.args[1]; // pickCargoCommand's rustup form: ["run", <channel>, "cargo", …]
if (cargo.command === "rustup" && channel !== undefined) {
  try {
    env.RUSTC = execFileSync("rustup", ["which", "--toolchain", channel, "rustc"], {
      encoding: "utf8",
    }).trim();
  } catch {
    fail(`Rust ${channel} is not installed; install it with: rustup toolchain install ${channel}`);
  }
}
execFileSync(cargo.command, cargo.args, { cwd: REPO_ROOT, stdio: "inherit", env });
execFileSync("wasm-bindgen", ["--target", "web", "--out-dir", OUT_DIR, relative(DESKTOP, MODULE)], {
  cwd: DESKTOP,
  stdio: "inherit",
});
console.log(`build:wasm: wrote ${OUT_DIR}/ (wasm-bindgen ${installed})`);
