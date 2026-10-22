/**
 * AC-9CFMD3.3: the reveal action uses the official Tauri opener plugin, pinned like the
 * other dependencies, with a capability that allows revealing a file only.
 */

import { readFileSync, readdirSync } from "node:fs";
import { describe, expect, it } from "vitest";

const TAURI = new URL("../../src-tauri/", import.meta.url);

function read(rel: string): string {
  return readFileSync(new URL(rel, TAURI), "utf8");
}

/** Every permission the capability files grant (strings, or `{ identifier }` objects). */
function grantedPermissions(): string[] {
  const dir = new URL("capabilities/", TAURI);
  return readdirSync(dir)
    .filter((f) => f.endsWith(".json"))
    .flatMap((f) => {
      const cap = JSON.parse(readFileSync(new URL(f, dir), "utf8")) as {
        permissions?: (string | { identifier: string })[];
      };
      return (cap.permissions ?? []).map((p) => (typeof p === "string" ? p : p.identifier));
    });
}

describe("opener plugin (AC-9CFMD3.3)", () => {
  it("grants the reveal permission and no other opener permission", () => {
    const opener = grantedPermissions().filter((p) => p.startsWith("opener:"));
    expect(opener).toEqual(["opener:allow-reveal-item-in-dir"]);
  });

  it("grants no way to open URLs or paths", () => {
    const granted = grantedPermissions();
    for (const p of ["opener:default", "opener:allow-open-url", "opener:allow-open-path"]) {
      expect(granted).not.toContain(p);
    }
  });

  it("pins tauri-plugin-opener to an exact version, like tauri-plugin-dialog", () => {
    const cargo = read("Cargo.toml");
    const dialog = /^tauri-plugin-dialog = "(\d+\.\d+\.\d+)"$/m.exec(cargo);
    expect(dialog, "tauri-plugin-dialog is pinned as x.y.z").not.toBeNull();
    const opener = /^tauri-plugin-opener = "(\d+\.\d+\.\d+)"$/m.exec(cargo);
    expect(opener, 'tauri-plugin-opener = "x.y.z" in src-tauri/Cargo.toml').not.toBeNull();
    expect(opener?.[1]?.startsWith("2.")).toBe(true);
  });

  it("registers the plugin in the Tauri builder", () => {
    expect(read("src/lib.rs")).toMatch(/\.plugin\(\s*tauri_plugin_opener::init\(\)\s*\)/);
  });
});
