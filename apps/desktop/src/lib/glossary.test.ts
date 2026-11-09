// FR CW66T1: the glossary that every info popover reads (AC .1, .2, .3, .9).
import { existsSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { slugify } from "../../scripts/readme/readme-features";
import { GLOSSARY, type GlossaryKey } from "./glossary";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, "../../../..");

const entries = Object.entries(GLOSSARY) as [
  GlossaryKey,
  { term: string; short: string; long?: string; source: string },
][];

function text(key: string): string {
  const e = (GLOSSARY as Record<string, { short: string; long?: string }>)[key];
  return e ? `${e.short} ${e.long ?? ""}` : "";
}

/** The sections AC .2 lists, by glossary key. */
const SECTIONS = [
  "section.meter",
  "section.stats",
  "section.chart",
  "section.ruler",
  "section.segmentMarks",
  "section.ed",
  "section.subChart",
  "section.pd",
  "section.records",
  "section.trigger",
  "section.cable",
  "section.console",
  "section.firmware",
  "section.settings",
  "section.recorder",
] as const;

/** The abbreviation groups AC .3 lists, each with the spellings an entry's term may use. */
const TERM_GROUPS: readonly (readonly string[])[] = [
  ["VBUS"],
  ["IBUS"],
  ["D+", "D−", "D-"],
  ["CC1", "CC2"],
  ["SPS"],
  ["PD"],
  ["PPS"],
  ["AVS"],
  ["EPR"],
  ["SPR"],
  ["SOP", "SOP'", "SOP''"],
  ["VCONN"],
  ["DRP"],
  ["QC"],
  ["UFCS"],
  ["AFC", "FCP", "SCP"],
  ["BC1.2"],
  ["ΔT", "ΔAh", "ΔWh"],
  ["E.D."],
  ["Ah"],
  ["Wh"],
  ["PDO", "RDO"],
  ["VDM"],
  ["e-marker"],
  // FR 62VEDM (AC .1, .3): the Trigger card titles and the sweep's finds.
  ["VFCP"],
  ["TFCP"],
  ["SFCP"],
  ["VID", "PID"],
  ["WebUSB", "WebHID"],
];

/** Abbreviations the app certainly shows: the readout, the rate picker and the chart. */
const ALWAYS_SHOWN = ["VBUS", "IBUS", "SPS", "PD", "PPS", "EPR", "SOP", "E.D.", "Ah", "Wh"];

function norm(s: string): string {
  return s.replace(/−/g, "-").toLowerCase();
}

/** The spellings an entry's term carries: each "/"-, ","- or space-separated part, and
 * the whole term without spaces ("BC 1.2" counts as "BC1.2"). */
function termTokens(term: string): Set<string> {
  const parts = term.split(/[/,]|\s+/).filter((p) => p !== "");
  return new Set([...parts, term.replace(/\s+/g, "")].map(norm));
}

function entryFor(group: readonly string[]): GlossaryKey | undefined {
  return entries.find(
    ([key, e]) => key.startsWith("term.") && group.some((a) => termTokens(e.term).has(norm(a))),
  )?.[0];
}

function frNotes(): string {
  // The FR moves to specs/frs/archive/ when its milestone is archived.
  const live = join(repoRoot, "specs/frs/CW66T1.md");
  const path = existsSync(live) ? live : join(repoRoot, "specs/frs/archive/CW66T1.md");
  const fr = readFileSync(path, "utf8");
  const at = fr.indexOf("\n## Notes");
  expect(at, "the FR has a ## Notes section").toBeGreaterThan(-1);
  const rest = fr.slice(at + "\n## Notes".length);
  const end = rest.search(/\n## /);
  return end === -1 ? rest : rest.slice(0, end);
}

describe("AC-CW66T1.1: one glossary of keyed entries", () => {
  it("holds entries keyed section.* or term.*, each with a term and a short line", () => {
    expect(entries.length).toBeGreaterThanOrEqual(SECTIONS.length + ALWAYS_SHOWN.length);
    for (const [key, e] of entries) {
      expect(key, "key prefix").toMatch(/^(section|term)\.[A-Za-z][A-Za-z0-9]*$/);
      expect(e.term.trim(), `${key}.term`).not.toBe("");
      expect(e.short.trim().length, `${key}.short`).toBeGreaterThan(10);
    }
  });

  it("keeps the short line short and the longer line optional but longer", () => {
    for (const [key, e] of entries) {
      expect(e.short.length, `${key}.short is one short line`).toBeLessThanOrEqual(160);
      expect(e.short, `${key}.short is one line`).not.toMatch(/\n/);
      if (e.long !== undefined) {
        expect(e.long.trim().length, `${key}.long`).toBeGreaterThan(e.short.length / 2);
      }
    }
  });
});

describe("AC-CW66T1.2: an entry per section, saying what it does and its risk", () => {
  it("has a section entry for every listed section", () => {
    const keys = entries.map(([k]) => k as string);
    for (const s of SECTIONS) expect(keys, s).toContain(s);
  });

  it("the firmware entry warns that it rewrites the meter's firmware", () => {
    const t = text("section.firmware");
    expect(t).toMatch(/firmware/i);
    expect(t).toMatch(/rewrit|overwrit|replac|erase/i);
  });

  it("the trigger entry warns that VBUS can rise above 5 V", () => {
    const t = text("section.trigger");
    expect(t).toMatch(/VBUS|voltage/i);
    expect(t).toMatch(/(above|over|beyond|higher than|more than)\s+5\s?V/i);
  });

  it("the console entry warns that it sends raw messages on the bus", () => {
    const t = text("section.console");
    expect(t).toMatch(/raw/i);
    expect(t).toMatch(/message/i);
    expect(t).toMatch(/\bbus\b|\bCC\b|cable|line/i);
  });

  it("the settings entry warns that it writes to the meter", () => {
    const t = text("section.settings");
    expect(t).toMatch(/writ/i);
    expect(t).toMatch(/meter/i);
  });
});

describe("AC-CW66T1.3: abbreviation entries, or a note naming the ones the UI never shows", () => {
  it("has an entry for the abbreviations the app always shows", () => {
    for (const a of ALWAYS_SHOWN) expect(entryFor([a]), a).toBeDefined();
  });

  it("every listed group has a term entry or is named in the FR Notes", () => {
    const notes = frNotes();
    const missing = TERM_GROUPS.filter((g) => entryFor(g) === undefined);
    for (const g of missing) {
      expect(
        g.some((a) => notes.includes(a)),
        `${g.join("/")} has no glossary entry, so the FR Notes must name it`,
      ).toBe(true);
    }
  });

  it("a term entry names one of the listed abbreviations", () => {
    for (const [key, e] of entries) {
      if (!key.startsWith("term.")) continue;
      const tokens = termTokens(e.term);
      expect(
        TERM_GROUPS.some((g) => g.some((a) => tokens.has(norm(a)))),
        `${key} (${e.term}) is one of AC .3's abbreviations`,
      ).toBe(true);
    }
  });
});

describe("AC-CW66T1.9: every entry records where its text comes from", () => {
  const protocol = readFileSync(join(repoRoot, "docs/protocol.md"), "utf8");
  const anchors = new Set(
    protocol
      .split("\n")
      .filter((l) => /^#{1,6} /.test(l))
      .map((l) => slugify(l.replace(/^#{1,6} /, ""))),
  );

  it("each source is our definition, a protocol.md section or a named spec", () => {
    for (const [key, e] of entries) {
      const s = e.source;
      if (s === "our definition") continue;
      const doc = /^docs\/protocol\.md#([a-z0-9_-]+)$/.exec(s);
      if (doc) {
        expect(anchors, `${key}: anchor ${doc[1]} exists in docs/protocol.md`).toContain(doc[1]);
        continue;
      }
      expect(s, `${key}: a named spec`).toMatch(/^(USB PD \d|USB Type-C|BC ?1\.2|QC \d|UFCS)/);
    }
  });

  it("never cites vendor material", () => {
    for (const [key, e] of entries) {
      expect(e.source, key).not.toMatch(/chargerlab|power-z lab|mtools|downloaded\/|vendor/i);
    }
  });

  it("the E.D. texts say it is this app's own measure, defined by us", () => {
    const ed = entryFor(["E.D."]);
    expect(ed).toBeDefined();
    for (const key of ["section.ed", ed as string]) {
      expect(text(key), key).toMatch(/this app|our own|open-powerz/i);
      expect((GLOSSARY as Record<string, { source: string }>)[key]?.source).toBe("our definition");
    }
  });
});

/** FR 62VEDM: a source is a docs/protocol.md section that exists, or a named spec. */
function sourcedOutsideUs(source: string): boolean {
  const protocol = readFileSync(join(repoRoot, "docs/protocol.md"), "utf8");
  const anchors = new Set(
    protocol
      .split("\n")
      .filter((l) => /^#{1,6} /.test(l))
      .map((l) => slugify(l.replace(/^#{1,6} /, ""))),
  );
  const doc = /^docs\/protocol\.md#([a-z0-9_-]+)$/.exec(source);
  if (doc) return anchors.has(doc[1] as string);
  return /^(USB PD \d|USB Type-C|BC ?1\.2|QC \d|UFCS)/.test(source);
}

describe("AC-62VEDM.1: VFCP, TFCP and SFCP have short sourced entries", () => {
  const cases: [string, RegExp][] = [
    ["VFCP", /vivo/i],
    ["TFCP", /Transsion|Tecno|Infinix/i],
    ["SFCP", /Spreadtrum|Unisoc/i],
  ];
  for (const [abbr, maker] of cases) {
    it(`${abbr} has a term entry naming its phone maker, from protocol.md or a named spec`, () => {
      const key = entryFor([abbr]);
      expect(key, `${abbr} has a term.* entry`).toBeDefined();
      const e = GLOSSARY[key as GlossaryKey];
      expect(e.short, `${abbr}.short names the maker`).toMatch(maker);
      expect(e.short.length).toBeLessThanOrEqual(160);
      expect(sourcedOutsideUs(e.source), `${abbr} source ${e.source}`).toBe(true);
    });
  }

  it("each of the three has its own entry", () => {
    const keys = ["VFCP", "TFCP", "SFCP"].map((a) => entryFor([a]));
    expect(keys.every((k) => k !== undefined)).toBe(true);
    expect(new Set(keys).size).toBe(3);
  });
});

describe("AC-62VEDM.3: entries for the abbreviations the sweep found", () => {
  it("VID / PID has a term entry", () => {
    const key = entryFor(["VID", "PID"]);
    expect(key).toBeDefined();
    expect(text(key as string)).toMatch(/vendor/i);
    expect(text(key as string)).toMatch(/product/i);
  });

  it("WebUSB / WebHID has a term entry", () => {
    const key = entryFor(["WebUSB", "WebHID"]);
    expect(key).toBeDefined();
    expect(text(key as string)).toMatch(/browser/i);
  });
});
