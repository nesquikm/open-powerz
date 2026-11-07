// Builds the web edition's WebAssembly module (FR QFTCWY, AC .4).
//
//   bun run build:wasm
//
// 1. cargo build -p powerz-wasm --release --target wasm32-unknown-unknown --locked
// 2. wasm-bindgen --target web --out-dir src/wasm ../../target/wasm32-unknown-unknown/release/powerz_wasm.wasm
//
// The wasm-bindgen CLI must be the exact version the crate pins (`wasm-bindgen = "=X.Y.Z"`
// in crates/powerz-wasm/Cargo.toml): the glue it writes only works with that runtime.
// src/wasm/ is git-ignored.
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const DESKTOP = join(dirname(fileURLToPath(import.meta.url)), "..");
const REPO_ROOT = join(DESKTOP, "../..");
const OUT_DIR = "src/wasm";
const TARGET = "wasm32-unknown-unknown";
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

execFileSync("cargo", ["build", "-p", "powerz-wasm", "--release", "--target", TARGET, "--locked"], {
  cwd: REPO_ROOT,
  stdio: "inherit",
});
execFileSync("wasm-bindgen", ["--target", "web", "--out-dir", OUT_DIR, relative(DESKTOP, MODULE)], {
  cwd: DESKTOP,
  stdio: "inherit",
});
console.log(`build:wasm: wrote ${OUT_DIR}/ (wasm-bindgen ${installed})`);
