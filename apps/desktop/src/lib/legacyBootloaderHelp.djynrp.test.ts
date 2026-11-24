// FR DJYNRP (AC .2, .7): the old-bootloader refusal names the CLI flag that
// opts in to the experimental path, in the engine's text and in the UI copy.
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { LEGACY_BOOTLOADER_HELP } from "./deviceDetails";

const FLAG = "--experimental-old-bootloader";

/** The Rust string literal of `LEGACY_BOOTLOADER_HELP`, line continuations folded. */
function engineText(): string {
  const src = readFileSync(
    new URL("../../../../crates/powerz-protocol/src/firmware/flash.rs", import.meta.url),
    "utf8",
  );
  const m = /pub const LEGACY_BOOTLOADER_HELP: &str = "((?:[^"\\]|\\[\s\S])*)";/.exec(src);
  const literal = m?.[1];
  if (literal === undefined) throw new Error("LEGACY_BOOTLOADER_HELP not found in flash.rs");
  return literal.replace(/\\\r?\n\s*/g, "");
}

describe("the old-bootloader help text names the experimental flag (DJYNRP)", () => {
  it("the engine's text names the flag", () => {
    expect(engineText()).toContain(FLAG);
  });

  it("the UI copy names the flag", () => {
    expect(LEGACY_BOOTLOADER_HELP).toContain(FLAG);
  });

  it("the UI copy calls the path experimental, beyond the flag's own name", () => {
    expect(LEGACY_BOOTLOADER_HELP.split(FLAG).join("").toLowerCase()).toContain("experimental");
  });
});
