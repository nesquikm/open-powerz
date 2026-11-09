/**
 * FR K1GZTR: the shared drop-down style in app.css draws the app's chevron (no native
 * arrow), coloured by the theme's text token, gives the native arrow back in
 * forced-colors mode, and no component overrides that arrow or its padding.
 */

import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const SRC = fileURLToPath(new URL(".", import.meta.url));
const APP_CSS = readFileSync(join(SRC, "app.css"), "utf8");

interface Rule {
  selectors: string[];
  body: string;
  /** The enclosing at-rule prelude (e.g. `@media (forced-colors: active)`), or "". */
  atRule: string;
}

/** Flat list of style rules, with the at-rule each sits in (one level deep is enough). */
function rules(css: string): Rule[] {
  const text = css.replace(/\/\*[\s\S]*?\*\//g, "");
  const out: Rule[] = [];
  const walk = (src: string, atRule: string) => {
    let i = 0;
    while (i < src.length) {
      const open = src.indexOf("{", i);
      if (open < 0) break;
      const prelude = src.slice(i, open).trim();
      let depth = 1;
      let j = open + 1;
      while (j < src.length && depth > 0) {
        if (src[j] === "{") depth++;
        else if (src[j] === "}") depth--;
        j++;
      }
      const body = src.slice(open + 1, j - 1);
      if (prelude.startsWith("@")) walk(body, prelude);
      else
        out.push({
          selectors: prelude.split(",").map((s) => s.trim().replace(/\s+/g, " ")),
          body,
          atRule,
        });
      i = j;
    }
  };
  walk(text, "");
  return out;
}

const decl = (body: string, prop: string): string[] =>
  [...body.matchAll(new RegExp(`(?:^|[;{\\s])${prop}\\s*:\\s*([^;]+)`, "g"))].map((m) =>
    (m[1] ?? "").trim(),
  );

const ALL = rules(APP_CSS);
/** Top-level rules whose selector list names bare `select`. */
const SELECT_RULES = ALL.filter((r) => r.atRule === "" && r.selectors.includes("select"));

describe("shared drop-down style in app.css (AC-K1GZTR.1)", () => {
  it("has a select rule that turns the native arrow off", () => {
    const appearance = SELECT_RULES.flatMap((r) => decl(r.body, "appearance"));
    expect(appearance).toContain("none");
  });

  it("draws the chevron as two gradients coloured by var(--text), on selects only", () => {
    const withChevron = SELECT_RULES.filter((r) => /linear-gradient\(/.test(r.body));
    expect(withChevron.length, "a select rule carries the chevron").toBeGreaterThan(0);
    for (const r of withChevron) {
      expect(r.selectors, "the chevron rule must not style buttons").not.toContain("button");
      const images = decl(r.body, "background-image").join(" ");
      const gradients = images.match(/linear-gradient\([^)]*\)/g) ?? [];
      expect(gradients.length, "two gradient layers").toBeGreaterThanOrEqual(2);
      for (const g of gradients) expect(g, "coloured by the text token").toContain("var(--text)");
      expect(decl(r.body, "background-repeat").join(" ")).toMatch(/no-repeat/);
      expect(decl(r.body, "background-position").length, "chevron is positioned").toBeGreaterThan(
        0,
      );
      expect(decl(r.body, "background-size").length, "chevron is sized").toBeGreaterThan(0);
      // No color literals: theme colors come from lib/theme.ts.
      expect(r.body).not.toMatch(/#[0-9a-f]{3,8}\b|\brgba?\(|\bhsla?\(/i);
    }
  });

  it("sizes the right padding apart from the left, to leave room for the chevron (AC-K1GZTR.3)", () => {
    const right = SELECT_RULES.flatMap((r) => [
      ...decl(r.body, "padding-right"),
      ...decl(r.body, "padding-inline-end"),
    ]);
    expect(right.length, "a select rule sets padding-right").toBeGreaterThan(0);
  });
});

describe("forced-colors mode (AC-K1GZTR.4)", () => {
  it("gives the native arrow back for selects", () => {
    const forced = ALL.filter(
      (r) => /forced-colors\s*:\s*active/.test(r.atRule) && r.selectors.includes("select"),
    );
    expect(forced.length, "an @media (forced-colors: active) select rule").toBeGreaterThan(0);
    const appearance = forced.flatMap((r) => decl(r.body, "appearance"));
    expect(appearance.some((v) => v === "auto" || v === "menulist")).toBe(true);
  });
});

/** Every .svelte file under src/. */
function svelteFiles(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const p = join(dir, e.name);
    if (e.isDirectory()) return svelteFiles(p);
    return e.name.endsWith(".svelte") ? [p] : [];
  });
}

describe("components keep the shared arrow (AC-K1GZTR.1, AC-K1GZTR.3)", () => {
  const OVERRIDES = [
    "appearance",
    "-webkit-appearance",
    "background",
    "background-image",
    "background-position",
    "background-size",
    "padding",
    "padding-right",
    "padding-inline",
    "padding-inline-end",
  ];
  for (const file of svelteFiles(SRC)) {
    const styles = [...readFileSync(file, "utf8").matchAll(/<style[^>]*>([\s\S]*?)<\/style>/g)]
      .map((m) => m[1])
      .join("\n");
    if (!styles) continue;
    const name = file.slice(SRC.length);
    it(`${name} does not override the drop-down arrow or its right padding`, () => {
      const bad = rules(styles)
        .filter((r) => r.selectors.some((s) => /(^|[\s>+~(])select\b|:global\(select\)/.test(s)))
        .flatMap((r) =>
          OVERRIDES.filter((p) => decl(r.body, p).length > 0).map(
            (p) => `${r.selectors.join(", ")} { ${p} }`,
          ),
        );
      expect(bad).toEqual([]);
    });
  }
});
