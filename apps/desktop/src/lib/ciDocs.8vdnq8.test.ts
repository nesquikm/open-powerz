/**
 * FR 8VDNQ8: the API docs of the whole workspace build with warnings as errors.
 *
 * The regression check of AC-8VDNQ8.1 is the cargo doc command itself, which CI runs
 * (AC-8VDNQ8.2). The source checks here catch its two known causes without running cargo.
 *
 * Line-based and CRLF safe, in the style of web/wasmBuild.test.ts.
 */

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const REPO_ROOT = fileURLToPath(new URL("../../../../", import.meta.url));
const WATCH = "crates/powerz-cli/src/commands/watch.rs";
const RECORDING_FILE = "crates/powerz-wasm/src/recording_file.rs";

const linesOf = (rel: string): string[] =>
  readFileSync(join(REPO_ROOT, rel), "utf8").split(/\r?\n/);

/** The doc comment directly above line `at`, attributes between them skipped. */
function docsAbove(lines: string[], at: number): string {
  const docs: string[] = [];
  for (let i = at - 1; i >= 0; i -= 1) {
    const line = lines[i] ?? "";
    if (line.startsWith("///")) docs.unshift(line.slice(3).trim());
    else if (!line.startsWith("#[")) break;
  }
  return docs.join(" ");
}

/** The doc text of the item whose declaration starts with `decl`, on one line. */
function docsOf(rel: string, decl: string): string {
  const lines = linesOf(rel).map((l) => l.trim());
  const at = lines.findIndex((l) => l.startsWith(decl));
  expect(at, `${rel}: ${decl}`).toBeGreaterThan(-1);
  return docsAbove(lines, at);
}

/** The docs of every `pub` item of the file, and its module docs. */
function publicDocs(rel: string): string {
  const lines = linesOf(rel).map((l) => l.trim());
  const module = lines.filter((l) => l.startsWith("//!")).map((l) => l.slice(3).trim());
  const items = lines.flatMap((l, i) => (/^pub\s/.test(l) ? [docsAbove(lines, i)] : []));
  return [module.join(" "), ...items].join("\n");
}

/** A rustdoc link to `name`: [`name`], [name], [`path::name`], or text with such a target. */
const linkTo = (name: string): RegExp =>
  new RegExp(`\\[\`?(?:\\w+::)*${name}(?:\\(\\))?\`?\\]|\\]\\((?:\\w+::)*${name}(?:\\(\\))?\\)`);

describe("public docs link no private item (AC-8VDNQ8.1)", () => {
  it("watch.rs: no public doc links MAX_START_ECHOES", () => {
    expect(publicDocs(WATCH)).not.toMatch(linkTo("MAX_START_ECHOES"));
  });

  it("watch.rs: the watch_cdc docs still name MAX_START_ECHOES as code", () => {
    expect(docsOf(WATCH, "pub fn watch_cdc")).toContain("`MAX_START_ECHOES`");
  });

  it("recording_file.rs: no public doc links db_select_rows_sql", () => {
    expect(publicDocs(RECORDING_FILE)).not.toMatch(linkTo("db_select_rows_sql"));
  });

  it("recording_file.rs: the push docs still name db_select_rows_sql as code", () => {
    expect(docsOf(RECORDING_FILE, "pub fn push(&mut self, elapsed")).toContain(
      "`db_select_rows_sql`",
    );
  });

  it.each([WATCH, RECORDING_FILE])("%s allows no rustdoc lint to get there", (rel) => {
    expect(linesOf(rel).join("\n")).not.toMatch(
      /\b(?:allow|expect)\s*\([^)]*\brustdoc::(?:private_intra_doc_links|all)\b/,
    );
  });
});

/** The lines of every ci.yml step named "Rust docs", comments left out. */
function rustDocsSteps(): string[][] {
  const lines = linesOf(".github/workflows/ci.yml").filter((l) => !l.trim().startsWith("#"));
  const starts = lines.flatMap((l, i) => (/^ {6}- name:\s*Rust docs\s*$/.test(l) ? [i] : []));
  return starts.map((start) => {
    const rest = lines.slice(start + 1);
    const end = rest.findIndex((l) => l.trim() !== "" && !/^ {7}/.test(l));
    return [lines[start] ?? "", ...(end < 0 ? rest : rest.slice(0, end))];
  });
}

/** The unquoted value of `key:` in the "Rust docs" step. */
function rustDocsValue(key: string): string | undefined {
  const line = rustDocsSteps()
    .flat()
    .find((l) => l.trim().startsWith(`${key}:`));
  return line
    ?.trim()
    .slice(key.length + 1)
    .trim()
    .replace(/^(["'])(.*)\1$/, "$2");
}

describe("CI documents the whole workspace (AC-8VDNQ8.2)", () => {
  it("has exactly one Rust docs step, and it runs on every OS and blocks", () => {
    const steps = rustDocsSteps();
    expect(steps).toHaveLength(1);
    expect(steps.flat().join("\n")).not.toMatch(/^\s*(?:if|continue-on-error):/m);
  });

  it("runs cargo doc for the workspace", () => {
    expect(rustDocsValue("run")).toBe("cargo doc --workspace --no-deps --locked");
  });

  it("treats rustdoc warnings as errors", () => {
    expect(rustDocsValue("RUSTDOCFLAGS")).toBe("-D warnings");
  });
});
