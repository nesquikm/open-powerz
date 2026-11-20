// FR BKSE70 (AC .4, .5): the web UI shows the same old-bootloader text as
// the engine. The engine keeps it in `LEGACY_BOOTLOADER_HELP` (flash.rs);
// the UI copy is that text with its first letter capitalised.
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { LEGACY_BOOTLOADER_HELP } from "./deviceDetails";

/** The Rust string literal of `LEGACY_BOOTLOADER_HELP`, with its `\`
 * line continuations folded the way rustc folds them. */
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

describe("the old-bootloader help text (BKSE70)", () => {
  it("is the engine's text with a capital first letter", () => {
    const engine = engineText();
    expect(LEGACY_BOOTLOADER_HELP).toBe(engine.charAt(0).toUpperCase() + engine.slice(1));
  });
});
