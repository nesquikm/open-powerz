// FR SPYPSD (AC-SPYPSD.4): no README line or docs page calls a feature the web
// edition has (recording, export, settings, records, logo, triggering, PD capture,
// PDF export) desktop-only; only firmware update, the recorder's folder and showing
// a saved file stay desktop-only, as the web feature map (WEB_FEATURES) says.
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join, posix, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { WEB_FEATURES, type Features } from "../../src/lib/web/features";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, "../../../..");
const read = (rel: string): string => readFileSync(join(repoRoot, rel), "utf8");

/** README.md plus every .md file under docs/, as repo-relative posix paths. */
function pages(): string[] {
  const entries = readdirSync(join(repoRoot, "docs"), { recursive: true, encoding: "utf8" });
  const docs = entries
    .filter((e) => e.endsWith(".md"))
    .map((e) => posix.join("docs", e.split("\\").join("/")))
    .sort();
  return ["README.md", ...docs];
}

/** A phrase that says something is desktop-only. "desktop app only after …" (an
 * adverb, not a claim) and "what stays in the desktop app" (a pointer) are not. */
const CLAIM =
  /desktop[- ]only|desktop app only(?![ ]+[a-z])|only in the desktop app|(?<!what )stays? in the desktop app|leaves? to the desktop app/i;

/** A heading over a section of desktop-only features ("What stays in the desktop app"). */
const SECTION =
  /desktop[- ]only|desktop app only|only in the desktop app|stays? in the desktop app/i;

/** The three features that stay desktop-only, as the docs name them (headings,
 * prose and backend method names). */
const ALLOWED: RegExp[] = [
  /firmware( update| file)?/gi,
  /`?firmware\w+`?/gi,
  /(the )?recorder'?s? (save )?folder|recording folder|`?chooseRecordingFolder\w*`?(\(\):[^|]*)?/gi,
  /(show|reveal)\w* (a |the )?(saved )?file( in the file manager)?|`?revealPath`?/gi,
];

/** The web edition's features no page may call desktop-only. */
const FORBIDDEN: { name: string; re: RegExp }[] = [
  { name: "recording", re: /\brecord(ing|er)?s?\b/i },
  { name: "export", re: /\bexport/i },
  { name: "settings", re: /\bsettings?\b/i },
  { name: "records", re: /\boffline records?\b/i },
  { name: "logo", re: /\blogo\b/i },
  { name: "triggering", re: /\btrigger/i },
  { name: "PD capture", re: /\bPD capture|\.pdcap\b|\bcapture\b/i },
  { name: "PDF export", re: /\bPDF\b/i },
];

interface Claim {
  page: string;
  /** What the claim says is desktop-only. */
  subject: string;
  /** A list or section of desktop-only features (must name an allowed one). */
  listed: boolean;
}

/** Fenced code blanked, inline code kept (method names count). */
function prose(markdown: string): string[] {
  let fenced = false;
  return markdown.split("\n").map((line) => {
    if (/^\s{0,3}(`{3,}|~{3,})/.test(line)) {
      fenced = !fenced;
      return "";
    }
    return fenced ? "" : line;
  });
}

/** Markdown links reduced to their text. */
const unlink = (s: string): string => s.replace(/\[([^\]]*)\]\([^)]*\)/g, "$1");

/** The subject of a list item in a desktop-only list: its bold lead, or the item
 * without its parenthesised explanation. */
function itemSubject(item: string): string {
  const text = unlink(item.replace(/^\s*([-*]|\d+\.)\s+/, ""));
  const bold = /^\*\*([^*]+)\*\*/.exec(text);
  if (bold) return bold[1] ?? "";
  return text.replace(/\([^)]*\)/g, "").split(/[.;:]\s/)[0] ?? text;
}

/** Every desktop-only claim in a page. */
function claims(page: string, markdown: string): Claim[] {
  const out: Claim[] = [];
  const lines = prose(markdown);
  // Group into blocks: paragraphs, list items, table rows, headings.
  type Block = { kind: "para" | "item" | "row" | "heading"; text: string; heading: string };
  const blocks: Block[] = [];
  let heading = "";
  let cur: Block | undefined;
  const flush = () => {
    if (cur) blocks.push(cur);
    cur = undefined;
  };
  for (const line of lines) {
    if (line.trim() === "") {
      flush();
      continue;
    }
    const h = /^\s{0,3}#{1,6}\s+(.*)$/.exec(line);
    if (h) {
      flush();
      heading = h[1] ?? "";
      blocks.push({ kind: "heading", text: heading, heading });
      continue;
    }
    if (/^\s*\|/.test(line)) {
      flush();
      blocks.push({ kind: "row", text: line, heading });
      continue;
    }
    if (/^\s*([-*]|\d+\.)\s/.test(line)) {
      flush();
      cur = { kind: "item", text: line.trim(), heading };
      continue;
    }
    if (cur) cur.text += ` ${line.trim()}`;
    else cur = { kind: "para", text: line.trim(), heading };
  }
  flush();

  let listIntro = false; // the list right after a "…desktop-only…:" paragraph
  blocks.forEach((b) => {
    const inSection = SECTION.test(b.heading) && b.kind !== "heading";
    if (b.kind === "heading") {
      listIntro = false;
      if (CLAIM.test(b.text)) out.push({ page, subject: b.text.replace(CLAIM, ""), listed: false });
      return;
    }
    if (b.kind === "item" && (inSection || listIntro)) {
      out.push({ page, subject: itemSubject(b.text), listed: true });
      return;
    }
    if (b.kind !== "item") listIntro = false;
    if (b.kind === "row") {
      if (CLAIM.test(b.text)) {
        const firstCell = b.text.split("|")[1] ?? "";
        out.push({ page, subject: firstCell, listed: true });
      }
      return;
    }
    const text = unlink(b.text);
    if (b.kind === "para" && CLAIM.test(text) && /:\s*$/.test(text)) listIntro = true;
    for (const sentence of text.split(/(?<=[.!?])\s+(?=[A-Z"`*])/)) {
      if (CLAIM.test(sentence)) out.push({ page, subject: sentence, listed: false });
    }
  });
  return out;
}

/** What is left of a claim's subject once the allowed features are taken out. */
function withoutAllowed(subject: string): string {
  return ALLOWED.reduce((s, re) => s.replace(re, " "), subject);
}

const namesAllowed = (subject: string): boolean =>
  ALLOWED.some((re) => {
    re.lastIndex = 0;
    const hit = re.test(subject);
    re.lastIndex = 0;
    return hit;
  });

describe("desktop-only claim finder (AC-SPYPSD.4)", () => {
  it("finds sentences, list items under a desktop-only intro and table rows", () => {
    const found = claims(
      "x.md",
      [
        "Logo upload is desktop-only.",
        "",
        "These stay desktop-only:",
        "",
        "- firmware update",
        "- PD capture (in the browser too)",
        "",
        "| `foo()` | Does foo. Desktop only. |",
        "",
        "The desktop app only after the typed version sends it.",
      ].join("\n"),
    );
    expect(found.map((c) => c.subject.trim())).toEqual([
      "Logo upload is desktop-only.",
      "These stay desktop-only:",
      "firmware update",
      "PD capture",
      "`foo()`",
    ]);
  });

  it("flags a web feature called desktop-only and passes the three that are", () => {
    const bad = (s: string) =>
      FORBIDDEN.filter((f) => f.re.test(withoutAllowed(s))).map((f) => f.name);
    expect(bad("PDF export stays in the desktop app.")).toEqual(["export", "PDF export"]);
    expect(bad("Protocol triggering is desktop-only.")).toEqual(["triggering"]);
    expect(
      bad(
        "Firmware update, revealing a file and choosing the recording folder stay in the desktop app.",
      ),
    ).toEqual([]);
    expect(bad("Choosing the recorder's folder")).toEqual([]);
  });
});

describe("no page calls a web-edition feature desktop-only (AC-SPYPSD.4)", () => {
  const all = pages().flatMap((p) => claims(p, read(p)));

  it("finds the desktop-only claims in the README and the web edition page", () => {
    expect(all.some((c) => c.page === "README.md" && c.listed)).toBe(true);
    expect(all.some((c) => c.page === "docs/how-to/web-edition.md" && c.listed)).toBe(true);
  });

  it("no claim names recording, export, settings, records, logo, triggering, PD capture or PDF export", () => {
    const offenders = all.flatMap((c) =>
      FORBIDDEN.filter((f) => f.re.test(withoutAllowed(c.subject))).map(
        (f) => `${c.page}: "${c.subject.trim()}" calls ${f.name} desktop-only`,
      ),
    );
    expect(offenders).toEqual([]);
  });

  it("every item listed as desktop-only is firmware update, the recorder's folder or showing a saved file", () => {
    const strays = all
      .filter((c) => c.listed && !namesAllowed(c.subject))
      .map((c) => `${c.page}: "${c.subject.trim()}"`);
    expect(strays).toEqual([]);
  });

  it("the README and the web edition page list all three as desktop-only", () => {
    const three: { name: string; re: RegExp }[] = [
      { name: "firmware update", re: /firmware update/i },
      { name: "the recorder's folder", re: /recorder'?s? (save )?folder|recording folder/i },
      { name: "showing a saved file", re: /(show|reveal)\w* (a |the )?(saved )?file/i },
    ];
    for (const page of ["README.md", "docs/how-to/web-edition.md"]) {
      const listed = all.filter((c) => c.page === page && c.listed).map((c) => c.subject);
      for (const f of three)
        expect(
          listed.some((s) => f.re.test(s)),
          `${page}: ${f.name}`,
        ).toBe(true);
    }
  });
});

describe("the web feature map agrees (AC-SPYPSD.4)", () => {
  it("switches off only firmware update, file reveal and the recorder's folder", () => {
    const off = (Object.keys(WEB_FEATURES) as (keyof Features)[])
      .filter((k) => !WEB_FEATURES[k])
      .sort();
    expect(off).toEqual(["fileReveal", "firmwareUpdate", "recordingFolder"]);
  });
});
