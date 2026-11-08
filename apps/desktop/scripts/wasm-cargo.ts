// FR G30K4S: picks the cargo command build:wasm runs. Pure, with injected probes.
import { join } from "node:path";

export const WASM_TARGET = "wasm32-unknown-unknown";

export interface CargoProbes {
  /** True when `rustup` is on the PATH. */
  hasRustup: () => boolean;
  /** Contents of the repository's rust-toolchain.toml. */
  toolchainFile: () => string;
  /** The active rustc's sysroot, or undefined when rustc cannot report one. */
  rustcSysroot: () => string | undefined;
  pathExists: (path: string) => boolean;
}

export type CargoChoice =
  { ok: true; command: string; args: string[] } | { ok: false; message: string };

const FIX = `rustup target add ${WASM_TARGET}`;

function toolchainChannel(toml: string): string | undefined {
  return /^\s*channel\s*=\s*"([^"]+)"/m.exec(toml)?.[1];
}

export function pickCargoCommand(cargoArgs: string[], probes: CargoProbes): CargoChoice {
  if (probes.hasRustup()) {
    const channel = toolchainChannel(probes.toolchainFile());
    if (!channel) {
      return { ok: false, message: 'rust-toolchain.toml has no `channel = "…"` line.' };
    }
    return { ok: true, command: "rustup", args: ["run", channel, "cargo", ...cargoArgs] };
  }
  const sysroot = probes.rustcSysroot();
  if (sysroot && probes.pathExists(join(sysroot, "lib", "rustlib", WASM_TARGET))) {
    return { ok: true, command: "cargo", args: cargoArgs };
  }
  return {
    ok: false,
    message:
      `The active rustc has no ${WASM_TARGET} target. Install rustup and run \`${FIX}\`, ` +
      "or put a Rust toolchain with that target first on the PATH.",
  };
}
