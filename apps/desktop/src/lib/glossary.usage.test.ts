// FR CW66T1: the UI and the glossary agree (AC .1: no popover text inline; AC .10: no
// unknown key in the UI, no entry the UI never uses).
import { readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { GLOSSARY } from "./glossary";

const here = dirname(fileURLToPath(import.meta.url));
const srcRoot = resolve(here, "..");

/** Every UI source file: .svelte and non-test .ts under src/, except the glossary itself. */
function uiFiles(dir = srcRoot): string[] {
  const out: string[] = [];
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) {
      out.push(...uiFiles(path));
    } else if (
      (name.endsWith(".svelte") || name.endsWith(".ts")) &&
      !name.endsWith(".test.ts") &&
      !name.endsWith(".d.ts") &&
      name !== "glossary.ts"
    ) {
      out.push(path);
    }
  }
  return out;
}

const KEY_LITERAL = /["'`]((?:section|term)\.[A-Za-z][A-Za-z0-9]*)["'`]/g;
const TRIGGER_TAG = /<(InfoButton|Term)\b([^>]*?)(\/?)>/g;

/** The glossary keys a source names: string literals like "term.vbus" or "section.chart". */
function usedKeys(source: string): string[] {
  return [...source.matchAll(KEY_LITERAL)].map((m) => m[1] as string);
}

/** Problems with how a source uses InfoButton/Term: a dynamic key, text passed inline. */
function triggerProblems(source: string): string[] {
  const problems: string[] = [];
  for (const m of source.matchAll(TRIGGER_TAG)) {
    const [whole, tag, attrs, selfClosing] = m as unknown as [string, string, string, string];
    if (!/\bkey="[^"]+"/.test(attrs)) problems.push(`${tag} without a literal key: ${whole}`);
    if (/\b(text|short|long|description|popover|label|title)\s*=/.test(attrs)) {
      problems.push(`${tag} with popover text inline: ${whole}`);
    }
    if (tag === "InfoButton" && selfClosing !== "/") {
      problems.push(`InfoButton with children: ${whole}`);
    }
  }
  return problems;
}

function check(sources: Record<string, string>, glossary: Record<string, unknown>): string[] {
  const used = new Set<string>();
  const problems: string[] = [];
  for (const [file, source] of Object.entries(sources)) {
    for (const key of usedKeys(source)) {
      used.add(key);
      if (!(key in glossary)) problems.push(`${file} uses unknown glossary key ${key}`);
    }
  }
  for (const key of Object.keys(glossary)) {
    if (!used.has(key)) problems.push(`glossary entry ${key} is used nowhere in the UI`);
  }
  return problems;
}

const sources = Object.fromEntries(
  uiFiles().map((f) => [relative(srcRoot, f), readFileSync(f, "utf8")]),
);

describe("AC-CW66T1.10: the UI and the glossary agree", () => {
  it("the check catches an unknown key and an unused entry", () => {
    const problems = check(
      { "A.svelte": '<InfoButton key="section.chart" /> <Term key="term.nope" />' },
      { "section.chart": {}, "term.vbus": {} },
    );
    expect(problems).toEqual([
      "A.svelte uses unknown glossary key term.nope",
      "glossary entry term.vbus is used nowhere in the UI",
    ]);
  });

  it("the UI renders InfoButton and Term triggers", () => {
    const all = Object.values(sources).join("\n");
    expect(all).toMatch(/<InfoButton\b/);
    expect(all).toMatch(/<Term\b/);
  });

  it("every key the UI uses exists, and every entry is used", () => {
    expect(check(sources, GLOSSARY)).toEqual([]);
  });
});

describe("AC-CW66T1.1: components pass only keys", () => {
  it("every InfoButton and Term gets a literal key and no text", () => {
    const problems = Object.entries(sources).flatMap(([file, s]) =>
      triggerProblems(s).map((p) => `${file}: ${p}`),
    );
    expect(problems).toEqual([]);
  });

  it("no component writes a glossary line inline", () => {
    const hits: string[] = [];
    for (const [key, e] of Object.entries(GLOSSARY)) {
      for (const line of [e.short, e.long].filter((l): l is string => !!l)) {
        for (const [file, s] of Object.entries(sources)) {
          if (s.includes(line)) hits.push(`${file} holds the text of ${key}`);
        }
      }
    }
    expect(Object.keys(GLOSSARY).length).toBeGreaterThan(0);
    expect(hits).toEqual([]);
  });
});
