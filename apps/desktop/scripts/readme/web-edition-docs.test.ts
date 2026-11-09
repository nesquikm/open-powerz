// FR QFTCX2: the web edition is documented — a README section, a docs page on
// browser support and permissions, the docs index link, and the project
// CLAUDE.md / technical spec updated for the powerz-wasm crate and web backend.
import { existsSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, "../../../..");
const read = (rel: string): string => readFileSync(join(repoRoot, rel), "utf8");

const PAGES_URL = "https://nesquikm.github.io/open-powerz/";

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

  it("lists the desktop-only features", () => {
    for (const feature of [
      /firmware update/i,
      /startup logo/i,
      /settings? (write|chang)/i,
      /delet\w* (offline )?record/i,
      /protocol trigger/i,
    ]) {
      expect(section).toMatch(feature);
    }
    expect(section).toMatch(/desktop[- ]only|desktop app only|only in the desktop app/i);
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

  it("states that the web edition only reads and streams", () => {
    expect(page).toMatch(/read[- ]only|only reads|reads and streams/i);
    expect(page).toMatch(/desktop app/i);
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
