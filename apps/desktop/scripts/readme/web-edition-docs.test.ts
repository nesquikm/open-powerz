// FR QFTCX2: the web edition is documented — a README section, a docs page on
// browser support and permissions, the docs index link, and the project
// CLAUDE.md / technical spec updated for the powerz-wasm crate and web backend.
// FR W8SHXF: the browser can now write too; the page, README and the web feature
// map (WEB_FEATURES) must agree on what stays desktop-only.
import { existsSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { WEB_FEATURES, type Features } from "../../src/lib/web/features";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, "../../../..");
const read = (rel: string): string => readFileSync(join(repoRoot, rel), "utf8");

const PAGES_URL = "https://nesquikm.github.io/open-powerz/";

/** The browser-support page (FR W8SHXF). */
const WEB_PAGE = "docs/how-to/web-edition.md";

/** How the docs name each feature of the web feature map (AC-W8SHXF.3). */
const FEATURE_NAMES: Record<keyof Features, RegExp> = {
  firmwareUpdate: /firmware update/i,
  logoUpload: /startup logo/i,
  settingsWrite: /settings? (write|chang)|chang\w* (the )?(meter'?s? )?settings/i,
  recordDelete: /delet\w* (the |an? )?(offline )?record/i,
  trigger: /protocol trigger/i,
  settingsRead: /read\w* (the )?(meter'?s? )?settings|settings? read/i,
  recordsList: /(offline )?record(s|ings)? list|list\w* (the )?(offline )?record/i,
  recordDownload: /download\w* (an? |the )?(offline )?record|record\w* download/i,
  fileReveal: /reveal\w* (a |the )?(saved )?file|show\w* (a |the )?(saved )?file/i,
};

const FEATURE_KEYS = Object.keys(FEATURE_NAMES) as (keyof Features)[];

/** The browser features that go over Web Serial (CDC); the rest use WebUSB. */
const NEEDS_WEB_SERIAL = new Set<keyof Features>(["settingsWrite", "trigger"]);

/** The `## ` section that lists the features staying in the desktop app. */
function desktopOnlySection(markdown: string): string | undefined {
  return h2Section(markdown, /desktop[- ]only|desktop app only|stays? in the desktop/i);
}

/** Paragraphs, list items and table rows: the units a claim must sit in together. */
function blocks(markdown: string): string[] {
  const out: string[] = [];
  for (const para of markdown.split(/\n\s*\n/)) {
    const lines = para.split("\n");
    if (lines.every((l) => /^\s*\|/.test(l))) {
      out.push(...lines);
      continue;
    }
    let cur: string[] = [];
    for (const l of lines) {
      if (/^\s*([-*]|\d+\.)\s/.test(l) && cur.length > 0) {
        out.push(cur.join(" "));
        cur = [];
      }
      cur.push(l.trim());
    }
    if (cur.length > 0) out.push(cur.join(" "));
  }
  return out;
}

/** The body of a `## <heading>` section, up to the next `## ` heading. */
function h2Section(markdown: string, heading: RegExp): string | undefined {
  const lines = markdown.split("\n");
  const start = lines.findIndex((l) => /^## /.test(l) && heading.test(l.slice(3)));
  if (start < 0) return undefined;
  const rest = lines.slice(start + 1);
  const end = rest.findIndex((l) => /^## /.test(l));
  return (end < 0 ? rest : rest.slice(0, end)).join("\n");
}

/** Relative markdown links in the docs index that point at a web-edition page. */
function webDocsLinks(index: string): string[] {
  const links: string[] = [];
  for (const m of index.matchAll(/\[([^\]]+)\]\(([^)#\s]+)(?:#[^)]*)?\)/g)) {
    const text = m[1] ?? "";
    const target = m[2] ?? "";
    if (/^[a-z]+:/i.test(target)) continue;
    if (/web edition|browser/i.test(text) || /web|browser/i.test(target)) links.push(target);
  }
  return links;
}

/** The docs page the index links for the web edition (first that exists). */
function webDocsPage(): string | undefined {
  const index = read("docs/README.md");
  for (const target of webDocsLinks(index)) {
    const path = join(repoRoot, "docs", target);
    if (existsSync(path) && path.endsWith(".md")) return readFileSync(path, "utf8");
  }
  return undefined;
}

describe("README Web edition section (AC-QFTCX2.1)", () => {
  const readme = read("README.md");
  const section = h2Section(readme, /^web edition\b/i) ?? "";

  it("has a ## Web edition section", () => {
    expect(h2Section(readme, /^web edition\b/i)).toBeDefined();
  });

  it("links the GitHub Pages address", () => {
    expect(section).toContain(PAGES_URL);
  });

  it("names Chrome and Edge on desktop as the supported browsers", () => {
    expect(section).toMatch(/\bChrome\b/);
    expect(section).toMatch(/\bEdge\b/);
    expect(section).toMatch(/desktop/i);
  });

  it("mentions the demo meter", () => {
    expect(section).toMatch(/demo (meter|device)/i);
  });

  it("names the two desktop-only features (AC-W8SHXF.2)", () => {
    expect(section).toMatch(FEATURE_NAMES.firmwareUpdate);
    expect(section).toMatch(FEATURE_NAMES.fileReveal);
    expect(section).toMatch(/desktop[- ]only|desktop app only|only in the desktop app/i);
  });

  it("no longer says the browser is read-only (AC-W8SHXF.2)", () => {
    expect(section).not.toMatch(/read[- ]only|only reads|reads and streams/i);
  });
});

describe("web edition docs page (AC-QFTCX2.2)", () => {
  const page = webDocsPage() ?? "";

  it("exists and is linked from the docs index", () => {
    expect(webDocsPage()).toBeDefined();
  });

  it("covers browser support (Chrome and Edge)", () => {
    expect(page).toMatch(/\bChrome\b/);
    expect(page).toMatch(/\bEdge\b/);
    expect(page).toMatch(/WebUSB/);
  });

  it("explains the device picker and the permission flow", () => {
    expect(page).toMatch(/picker/i);
    expect(page).toMatch(/permission/i);
  });

  it("describes the WebHID fallback", () => {
    expect(page).toMatch(/WebHID/);
    expect(page).toMatch(/fall ?back/i);
  });

  it("covers Linux udev access", () => {
    expect(page).toMatch(/Linux/);
    expect(page).toMatch(/udev/);
  });

  it("gives Windows driver notes for WebUSB", () => {
    expect(page).toMatch(/Windows/);
    expect(page).toMatch(/WinUSB/);
  });

  it("no longer says the web edition only reads and streams (AC-W8SHXF.1)", () => {
    expect(page).not.toMatch(/read[- ]only|only reads|reads and streams/i);
    expect(page).toMatch(/desktop app/i);
  });
});

describe("browser-support page matches the web feature map (AC-W8SHXF.1, AC-W8SHXF.3)", () => {
  const page = read(WEB_PAGE);
  const desktopOnly = desktopOnlySection(page) ?? "";
  const rest = page.replace(desktopOnly, "");
  const units = blocks(rest);

  it("has a ## section for the features that stay desktop-only", () => {
    expect(desktopOnlySection(page)).toBeDefined();
  });

  it("names every feature the web build switches off as desktop-only (AC-W8SHXF.3)", () => {
    const off = FEATURE_KEYS.filter((k) => !WEB_FEATURES[k]);
    expect(off.length).toBeGreaterThan(0);
    for (const key of off) expect(desktopOnly, key).toMatch(FEATURE_NAMES[key]);
  });

  it("does not name a feature the web build switches on as desktop-only (AC-W8SHXF.3)", () => {
    for (const key of FEATURE_KEYS.filter((k) => WEB_FEATURES[k])) {
      expect(desktopOnly, key).not.toMatch(FEATURE_NAMES[key]);
    }
  });

  it("lists every feature the web build switches on outside the desktop-only section (AC-W8SHXF.1)", () => {
    for (const key of FEATURE_KEYS.filter((k) => WEB_FEATURES[k])) {
      expect(rest, key).toMatch(FEATURE_NAMES[key]);
    }
  });

  it("names the interface each browser feature needs (AC-W8SHXF.1)", () => {
    for (const key of FEATURE_KEYS.filter((k) => WEB_FEATURES[k])) {
      const iface = NEEDS_WEB_SERIAL.has(key) ? /Web Serial/ : /WebUSB/;
      const unit = units.find((u) => FEATURE_NAMES[key].test(u) && iface.test(u));
      expect(unit, `${key} with ${String(iface)}`).toBeDefined();
    }
  });

  it("says settings changes and protocol triggering need Web Serial (AC-W8SHXF.1)", () => {
    expect(page).toMatch(/Web Serial/);
    for (const key of NEEDS_WEB_SERIAL) {
      expect(
        units.some((u) => FEATURE_NAMES[key].test(u) && /Web Serial/.test(u)),
        key,
      ).toBe(true);
    }
  });

  it("gives the reason firmware update stays desktop-only: a tab could brick the meter (AC-W8SHXF.1)", () => {
    const unit = blocks(desktopOnly).find((u) => FEATURE_NAMES.firmwareUpdate.test(u));
    expect(unit).toBeDefined();
    expect(unit ?? "").toMatch(/interrupt/i);
    expect(unit ?? "").toMatch(/brick/i);
  });

  it("gives the reason showing a file stays desktop-only: no file manager from a browser (AC-W8SHXF.1)", () => {
    const unit = blocks(desktopOnly).find((u) => FEATURE_NAMES.fileReveal.test(u));
    expect(unit).toBeDefined();
    expect(unit ?? "").toMatch(/file manager/i);
    expect(unit ?? "").toMatch(/browser|tab|page/i);
  });

  it("mentions the one-time reload protocol triggering needs (AC-W8SHXF.1)", () => {
    const unit = units.find((u) => /reload/i.test(u) && /cross-origin isolat/i.test(u));
    expect(unit).toBeDefined();
    expect(page).toMatch(/service worker/i);
    expect(page).toMatch(/once|one-time|first time/i);
  });
});

describe("CLAUDE.md and technical spec know the web edition (AC-QFTCX2.3)", () => {
  const claude = read("CLAUDE.md");
  const tree = h2Section(claude, /^Architecture\b/) ?? "";
  const keyCommands = h2Section(claude, /^Key Commands\b/) ?? "";

  it("the Architecture tree lists crates/powerz-wasm and the web backend folder", () => {
    expect(tree).toMatch(/powerz-wasm\//);
    expect(tree).toMatch(/\bweb\/\s+#/);
  });

  it("the Architecture tree lists crates/powerz-trigger (AC-W8SHXF.1)", () => {
    expect(tree).toMatch(/powerz-trigger\//);
  });

  it("the gating-rule chain runs bun run build:wasm before bun run test", () => {
    const gate = /\*\*Gating rule:\*\*[\s\S]*?```bash\n([^\n]+)\n```/.exec(claude)?.[1] ?? "";
    const wasm = gate.indexOf("bun run build:wasm");
    const test = gate.indexOf("bun run test");
    expect(wasm).toBeGreaterThanOrEqual(0);
    expect(test).toBeGreaterThan(wasm);
  });

  it("Key Commands lists build:wasm and build:pages", () => {
    const body = keyCommands.split("**Gating rule:**")[0] ?? "";
    expect(body).toContain("bun run build:wasm");
    expect(body).toContain("bun run build:pages");
  });

  it("the technical spec lists the powerz-wasm crate and the web backend folder", () => {
    const spec = read("specs/technical-spec.md");
    expect(spec).toMatch(/crates\/powerz-wasm/);
    expect(spec).toMatch(/src\/lib\/web/);
  });
});

describe("docs index links the web edition page (AC-QFTCX2.4)", () => {
  it("docs/README.md has a link to a web edition page that exists", () => {
    const index = read("docs/README.md");
    const links = webDocsLinks(index);
    expect(links.length).toBeGreaterThan(0);
    expect(links.some((t) => existsSync(join(repoRoot, "docs", t)))).toBe(true);
  });
});
