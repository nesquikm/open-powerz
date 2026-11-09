// FR 62VEDM: the Trigger card titles, the PD emulation block, the cable facts and the other
// abbreviations the sweep found are glossary terms (AC .1, .2, .3), checked on the sources.
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { GLOSSARY, type GlossaryKey } from "../glossary";

const here = dirname(fileURLToPath(import.meta.url));

function source(name: string): string {
  return readFileSync(join(here, name), "utf8");
}

/** The term.* key whose term carries `abbr` as one of its "/"-, ","- or space-separated parts. */
function keyFor(abbr: string): GlossaryKey | undefined {
  return (Object.keys(GLOSSARY) as GlossaryKey[]).find(
    (k) =>
      k.startsWith("term.") &&
      GLOSSARY[k].term
        .split(/[/,]|\s+/)
        .filter((p) => p !== "")
        .some((p) => p.toLowerCase() === abbr.toLowerCase()),
  );
}

function escape(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/** A Term opened with this literal key (its attributes may wrap onto the next line). */
function termTag(key: string): RegExp {
  return new RegExp(`<Term\\s+key="${escape(key)}"`);
}

/** Every `<tag ...>...</tag>` block of a source. */
function blocks(src: string, tag: string): string[] {
  return [...src.matchAll(new RegExp(`<${tag}\\b[^>]*>[\\s\\S]*?</${tag}>`, "g"))].map((m) => m[0]);
}

/** The `<section ...>` block that holds `marker`, up to its closing tag. */
function sectionWith(src: string, marker: string): string {
  const at = src.indexOf(marker);
  expect(at, `the source holds ${marker}`).toBeGreaterThan(-1);
  const start = src.lastIndexOf("<section", at);
  const end = src.indexOf("</section>", at);
  expect(start).toBeGreaterThan(-1);
  expect(end).toBeGreaterThan(at);
  return src.slice(start, end);
}

const trigger = source("TriggerView.svelte");

/** The protocol card title: the h2 every protocol section renders. */
function cardTitle(): string {
  const title = blocks(trigger, "h2").find((b) => b.includes('id="trigger-{s.protocol}-title"'));
  expect(title, "TriggerView has the protocol card title").toBeDefined();
  return title as string;
}

describe("AC-62VEDM.1: the VFCP, TFCP and SFCP card titles open their entries", () => {
  for (const abbr of ["VFCP", "TFCP", "SFCP"]) {
    it(`the ${abbr} title is a Term with its entry's key`, () => {
      const key = keyFor(abbr);
      expect(key, `${abbr} has a glossary entry`).toBeDefined();
      const title = cardTitle();
      expect(title).toContain(`"${abbr.toLowerCase()}"`);
      expect(title).toMatch(termTag(key as string));
    });
  }
});

describe("AC-62VEDM.2: the PD title, the PD emulation block and the cable facts", () => {
  it('the "USB PD" card title is a Term for term.pd', () => {
    const title = cardTitle();
    expect(title).toContain('"pd"');
    expect(title).toMatch(termTag("term.pd"));
  });

  it('the "PD emulation" heading wraps PD with term.pd', () => {
    const h3 = blocks(trigger, "h3").find((b) => /emulation/.test(b));
    expect(h3, "the PD emulation heading").toBeDefined();
    expect(h3).toMatch(termTag("term.pd"));
  });

  it("the E-marker label is a Term for term.emarker", () => {
    const dt = blocks(trigger, "dt").find((b) => /e-marker/i.test(b));
    expect(dt, "the E-marker label").toBeDefined();
    expect(dt).toMatch(termTag("term.emarker"));
  });

  it("the cable card wraps EPR, and VID / PID with their entry", () => {
    const cable = sectionWith(trigger, 'aria-labelledby="trigger-cable-title"');
    expect(cable).toMatch(termTag("term.epr"));
    const vidPid = keyFor("VID");
    expect(vidPid, "VID / PID has a glossary entry").toBeDefined();
    expect(keyFor("PID")).toBe(vidPid);
    expect(cable).toMatch(termTag(vidPid as string));
  });
});

describe("AC-62VEDM.3: the other abbreviations the sweep found", () => {
  it("the PD console's cable identity wraps VID / PID", () => {
    const vidPid = keyFor("VID");
    expect(vidPid, "VID / PID has a glossary entry").toBeDefined();
    const cable = sectionWith(source("PdConsoleView.svelte"), 'aria-labelledby="pd-cable-title"');
    expect(cable).toMatch(termTag(vidPid as string));
  });

  it("the PD analyzer's VBUS and IBUS column headers are Terms", () => {
    const pd = source("PdAnalyzer.svelte");
    expect(pd).toMatch(termTag("term.vbus"));
    expect(pd).toMatch(termTag("term.ibus"));
  });

  it("the unsupported-browser notice wraps WebUSB / WebHID", () => {
    const web = keyFor("WebUSB");
    expect(web, "WebUSB / WebHID has a glossary entry").toBeDefined();
    expect(keyFor("WebHID")).toBe(web);
    expect(source("NotSupported.svelte")).toMatch(termTag(web as string));
  });

  it("the meter settings view wraps SPS", () => {
    expect(source("MeterSettingsView.svelte")).toMatch(termTag("term.sps"));
  });
});
