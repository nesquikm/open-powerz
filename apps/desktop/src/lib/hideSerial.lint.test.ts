/**
 * FR AXHK2Q, Technical Design: every site that shows a serial goes through one helper.
 * A component reading `.serial` directly fails here, so a new tile or dialog cannot
 * bring the number back past the "Hide serial numbers" switch (AC-AXHK2Q.2).
 *
 * Destructuring, bracket reads and the backend's label count too, and the other UI modules
 * are held to the same rule. The only reads a component may keep: through `displaySerial` or `tileTitle`, which
 * mask it, and the plain field copy that fills in a listing.
 */

import { readdirSync, readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { toPosixPath } from "./posixPath";

/** A field read, a destructured field, a bracket read, or the backend's label, which holds
 * the serial ("KM003C — <serial>"). */
const SERIAL_READ =
  /\.(serial|uid)\b|\{[^{}]*\b(serial|uid)\b[^{}]*\}\s*=|\[["'](serial|uid)["']\]|\b(device|listing|d)\.label\b/;
const ALLOWED = [/displaySerial\(/, /tileTitle\(/, /^\s*serial: \w+\.serial,\s*$/];

function directReads(source: string): string[] {
  return source
    .replace(/<style>[\s\S]*?<\/style>/g, "")
    .split("\n")
    .filter((line) => SERIAL_READ.test(line) && !ALLOWED.some((ok) => ok.test(line)))
    .map((line) => line.trim());
}

describe("components read serials through the helper (AC-AXHK2Q.2)", () => {
  const components = readdirSync(new URL("./components/", import.meta.url), { recursive: true })
    .map(String)
    .map(toPosixPath)
    .filter((name) => name.endsWith(".svelte"))
    .map((name) => new URL(`./components/${name}`, import.meta.url));
  const files = [new URL("../App.svelte", import.meta.url), ...components];

  it("finds the components", () => {
    expect(components.length).toBeGreaterThan(20);
  });

  it.each(files.map((f) => [f.pathname.split("/").pop() ?? "", f] as const))(
    "%s has no direct read",
    (_name, file) => {
      expect(directReads(readFileSync(file, "utf8"))).toEqual([]);
    },
  );

  it("a stylesheet's .serial class is no read", () => {
    expect(directReads("<style>\n  .serial {\n    color: red;\n  }\n</style>")).toEqual([]);
  });

  it("the rule catches a direct read", () => {
    expect(directReads("<span>{details.serial}</span>")).toEqual(["<span>{details.serial}</span>"]);
    expect(directReads("{displaySerial(details.serial, hidden)}")).toEqual([]);
    expect(directReads("const { serial } = details;")).toHaveLength(1);
    expect(directReads('{details["serial"]}')).toHaveLength(1);
    expect(directReads("<option>{device.label}</option>")).toHaveLength(1);
  });

  it("no other module of the UI reads a serial", () => {
    // These hold the serial, match on it or mask it; a new one must argue its way in here.
    const known = new Set([
      "deviceDetails.ts",
      "firmwareUpdate.svelte.ts", // matches the updated meter by serial (AC-AXHK2M.15), never shows it
      "format.ts",
      "meterGrid.ts",
      "mockBackend.ts",
      "session.svelte.ts",
      "web/session.ts", // hands the meter serial to the UI in its connect/attach events, like the desktop backend
    ]);
    const modules = readdirSync(new URL("./", import.meta.url), { recursive: true })
      .map(String)
      .map(toPosixPath)
      .filter((name) => name.endsWith(".ts") && !name.includes(".test."))
      .filter((name) => !known.has(name));
    expect(modules.length).toBeGreaterThan(20);
    const reads = modules.filter((name) =>
      /\.(serial|uid)\b/.test(readFileSync(new URL(`./${name}`, import.meta.url), "utf8")),
    );
    expect(reads).toEqual([]);
  });
});

describe("module names", () => {
  // AC-HT1EMH.1/.2: readdirSync yields backslashes on Windows; the known list uses forward slashes.
  it("a Windows-style name normalizes to forward slashes", () => {
    expect(toPosixPath("web\\session.ts")).toBe("web/session.ts");
    expect(toPosixPath("a\\b\\c.svelte")).toBe("a/b/c.svelte");
  });

  it("forward-slash and flat names stay unchanged", () => {
    expect(toPosixPath("web/session.ts")).toBe("web/session.ts");
    expect(toPosixPath("format.ts")).toBe("format.ts");
  });
});

describe("the app wires the switch to the helper's callers (AC-AXHK2Q.2)", () => {
  it("the picker and the add dialog pass the option to the label", () => {
    for (const name of ["DevicePicker", "AddMeterDialog"]) {
      const source = readFileSync(new URL(`./components/${name}.svelte`, import.meta.url), "utf8");
      expect(source, name).not.toMatch(/deviceOptionLabel\(\w+\)/);
    }
  });
});
