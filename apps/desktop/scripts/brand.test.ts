// FR 492Z4N: the electron-surf brand kit (SVG sources, PNG renders and the
// generator) lives in assets/brand/, the desktop icon sources are the brand
// app icon and favicon, and the README opens with the brand banner.
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { decodePng, pngSize } from "./icons";

const here = dirname(fileURLToPath(import.meta.url));
const desktopDir = join(here, "..");
const repoRoot = join(desktopDir, "..", "..");
const brandDir = join(repoRoot, "assets", "brand");
const toolsDir = join(brandDir, "tools");
const iconsDir = join(desktopDir, "src-tauri", "icons");

const bytes = (path: string): Uint8Array => new Uint8Array(readFileSync(path));
const text = (path: string): string => readFileSync(path, "utf8");
const brand = (name: string): string => join(brandDir, name);

const svgSources = [
  "meter-logo.svg",
  "meter-logo-wordmark.svg",
  "app-icon.svg",
  "readme-banner.svg",
  "social-preview.svg",
  "favicon.svg",
];

const pngRenders: [string, number, number][] = [
  ["meter-logo.png", 200, 200],
  ["meter-logo-wordmark.png", 200, 200],
  ["app-icon-1024.png", 1024, 1024],
  ["readme-banner.png", 1280, 400],
  ["social-preview.png", 1280, 640],
  ["favicon-16.png", 16, 16],
  ["favicon-32.png", 32, 32],
];

describe("brand folder contents (AC-492Z4N.1)", () => {
  it.each(svgSources)("holds the SVG source %s", (name) => {
    expect(existsSync(brand(name)), name).toBe(true);
    const src = text(brand(name));
    expect(src).toMatch(/<svg[\s>]/);
    expect(src).toContain("</svg>");
  });

  it.each(pngRenders)("holds %s at %i×%i", (name, width, height) => {
    expect(existsSync(brand(name)), name).toBe(true);
    expect(pngSize(bytes(brand(name)))).toEqual({ width, height });
  });

  it.each(["meter-logo.png", "meter-logo-wordmark.png"])(
    "%s has only fully transparent or fully opaque pixels",
    (name) => {
      const { rgba } = decodePng(bytes(brand(name)));
      const partial = new Set<number>();
      for (let i = 3; i < rgba.length; i += 4) {
        const a = rgba[i]!;
        if (a !== 0 && a !== 255) partial.add(a);
      }
      expect([...partial]).toEqual([]);
      // Both values occur: a logo on a transparent ground.
      let opaque = 0;
      let clear = 0;
      for (let i = 3; i < rgba.length; i += 4) {
        if (rgba[i] === 255) opaque++;
        else clear++;
      }
      expect(opaque).toBeGreaterThan(0);
      expect(clear).toBeGreaterThan(0);
    },
  );
});

describe("brand README (AC-492Z4N.2)", () => {
  const readme = (): string => text(brand("README.md"));

  it("exists", () => {
    expect(existsSync(brand("README.md"))).toBe(true);
  });

  it("says the art is original art for this project under the MIT licence", () => {
    const r = readme();
    expect(r).toMatch(/original art/i);
    expect(r).toMatch(/\bMIT\b/);
    expect(r).toMatch(/licen[cs]e/i);
  });

  it("says the lettering is drawn from stroke glyphs with no font", () => {
    const r = readme();
    expect(r).toMatch(/stroke glyphs?/i);
    expect(r).toMatch(/no fonts?\b/i);
  });

  it("says it carries no ChargerLAB or POWER-Z marks", () => {
    const r = readme();
    expect(r).toMatch(/\bno\b[^.\n]*ChargerLAB/i);
    expect(r).toMatch(/POWER-Z/);
  });

  it.each([...svgSources, ...pngRenders.map(([name]) => name)])("lists %s", (name) => {
    expect(readme()).toContain(name);
  });

  it("says how to re-render the assets with the generator", () => {
    const r = readme();
    expect(r).toMatch(/re-?render/i);
    expect(r).toContain("render.py");
    expect(r).toMatch(/rsvg-convert/);
    expect(r).toMatch(/Pillow/);
  });
});

// Python's standard library modules the generator may plausibly use.
const stdlib = new Set([
  "__future__",
  "argparse",
  "base64",
  "collections",
  "dataclasses",
  "functools",
  "glob",
  "hashlib",
  "io",
  "itertools",
  "json",
  "math",
  "os",
  "pathlib",
  "re",
  "shutil",
  "string",
  "struct",
  "subprocess",
  "sys",
  "tempfile",
  "textwrap",
  "typing",
  "zlib",
]);

/** Top-level module names a Python source imports. */
function pythonImports(src: string): string[] {
  const mods: string[] = [];
  for (const line of src.split(/\r?\n/)) {
    const from = /^\s*from\s+([A-Za-z_][\w.]*)\s+import\b/.exec(line);
    if (from) {
      mods.push(from[1]!.split(".")[0]!);
      continue;
    }
    const imp = /^\s*import\s+(.+)$/.exec(line);
    if (imp) {
      for (const part of imp[1]!.split(",")) {
        const name = part.trim().split(/\s+/)[0]!;
        if (name) mods.push(name.split(".")[0]!);
      }
    }
  }
  return mods;
}

describe("brand generator (AC-492Z4N.3)", () => {
  const pyFiles = (): string[] =>
    existsSync(toolsDir) ? readdirSync(toolsDir).filter((f) => f.endsWith(".py")) : [];

  it("ships render.py and kit_svgs.py in the tools folder", () => {
    expect(pyFiles().sort()).toEqual(expect.arrayContaining(["kit_svgs.py", "render.py"]));
  });

  it("imports only Python's standard library and Pillow", () => {
    const files = pyFiles();
    expect(files.length).toBeGreaterThan(0);
    const local = new Set(files.map((f) => f.replace(/\.py$/, "")));
    for (const f of files) {
      const foreign = pythonImports(text(join(toolsDir, f))).filter(
        (m) => !stdlib.has(m) && m !== "PIL" && !local.has(m),
      );
      expect({ file: f, foreign }).toEqual({ file: f, foreign: [] });
    }
  });

  it("calls rsvg-convert for SVG rendering", () => {
    const all = pyFiles()
      .map((f) => text(join(toolsDir, f)))
      .join("\n");
    expect(all).toContain("rsvg-convert");
  });

  it("names every brand PNG and generated SVG it rewrites", () => {
    const all = pyFiles()
      .map((f) => text(join(toolsDir, f)))
      .join("\n");
    for (const [name] of pngRenders) expect(all, name).toContain(name);
    for (const name of svgSources.filter((n) => n !== "meter-logo.svg")) {
      expect(all, name).toContain(name);
    }
  });

  it("takes the output folder for mocks and previews from the command line", () => {
    expect(existsSync(join(toolsDir, "render.py"))).toBe(true);
    const src = text(join(toolsDir, "render.py"));
    expect(src).toMatch(/argparse|sys\.argv/);
    // A usage message or a required argument: it refuses to run without one.
    expect(src).toMatch(/usage|required=True|add_argument\(\s*["'][a-z]/i);
  });

  it("keeps no on-meter mocks or preview checks in the brand folder", () => {
    expect(existsSync(brandDir)).toBe(true);
    const stray: string[] = [];
    const walk = (dir: string): void => {
      for (const name of readdirSync(dir)) {
        const p = join(dir, name);
        if (statSync(p).isDirectory()) {
          if (name !== "__pycache__") walk(p);
        } else if (/onmeter|preview\.(png|txt)$|-preview\b|contact-sheet|kit-sheet/.test(name)) {
          stray.push(relative(brandDir, p));
        }
      }
    };
    walk(brandDir);
    // social-preview.* is a brand asset, not a preview check.
    expect(stray.filter((p) => !/social-preview\./.test(p))).toEqual([]);
  });
});

/** RGB of pixel (x, y) in a PNG. */
function rgbAt(png: Uint8Array, x: number, y: number): [number, number, number] {
  const { width, rgba } = decodePng(png);
  const i = (y * width + x) * 4;
  return [rgba[i]!, rgba[i + 1]!, rgba[i + 2]!];
}

function near(got: [number, number, number], want: [number, number, number], tol = 24): boolean {
  return got.every((c, i) => Math.abs(c - want[i]!) <= tol);
}

describe("desktop icon sources (AC-492Z4N.4)", () => {
  it("icon.svg is the brand app icon, byte for byte", () => {
    expect(Buffer.from(bytes(join(iconsDir, "icon.svg")))).toEqual(
      Buffer.from(bytes(brand("app-icon.svg"))),
    );
  });

  it("icon-small.svg is the brand favicon, byte for byte", () => {
    expect(Buffer.from(bytes(join(iconsDir, "icon-small.svg")))).toEqual(
      Buffer.from(bytes(brand("favicon.svg"))),
    );
  });

  it("32x32.png is regenerated from the brand favicon (yellow head, blue wave)", () => {
    const png = bytes(join(iconsDir, "32x32.png"));
    const head = rgbAt(png, 16, 7);
    const wave = rgbAt(png, 16, 29);
    expect({ head, yellow: near(head, [0xff, 0xd2, 0x3f]) }).toEqual({ head, yellow: true });
    expect({ wave, blue: near(wave, [0x2f, 0x6b, 0xff]) }).toEqual({ wave, blue: true });
  });

  it("icon.png is regenerated from the brand app icon (yellow at the centre)", () => {
    const png = bytes(join(iconsDir, "icon.png"));
    const ref = bytes(brand("app-icon-1024.png"));
    const centre = rgbAt(png, 512, 512);
    expect({ centre, match: near(centre, rgbAt(ref, 512, 512)) }).toEqual({
      centre,
      match: true,
    });
  });
});

describe("repository README banner (AC-492Z4N.5)", () => {
  it("opens with the brand banner by a relative path, alt text naming open-powerz and the meter", () => {
    const first = text(join(repoRoot, "README.md")).split(/\r?\n/)[0] ?? "";
    const md = /^!\[([^\]]*)\]\(([^)\s]+)\)/.exec(first);
    const html = /^<(?:p[^>]*>\s*)?<img\b[^>]*>/i.exec(first);
    expect(md ?? html, first).not.toBeNull();
    const alt = md ? md[1]! : (/\balt="([^"]*)"/i.exec(first)?.[1] ?? "");
    const src = md ? md[2]! : (/\bsrc="([^"]*)"/i.exec(first)?.[1] ?? "");
    expect(src.replace(/^\.\//, "")).toBe("assets/brand/readme-banner.png");
    expect(alt).toMatch(/open-powerz/i);
    expect(alt).toMatch(/KM003C|POWER-Z|meter/i);
  });
});

describe("social preview and favicon (AC-492Z4N.6)", () => {
  it("keeps the social preview and favicon in the brand folder", () => {
    for (const name of ["social-preview.svg", "social-preview.png", "favicon.svg"]) {
      expect(existsSync(brand(name)), name).toBe(true);
    }
  });

  it("brand README says the social preview is set by hand in the GitHub repository settings", () => {
    const r = text(brand("README.md"));
    expect(r).toMatch(/social preview[^\n]*(by hand|manual)|(by hand|manual)[^\n]*social preview/i);
    expect(r).toMatch(/GitHub[^\n]*settings/i);
  });

  it("brand README says the favicon is kept for the web edition", () => {
    expect(text(brand("README.md"))).toMatch(/favicon[^\n]*web edition|web edition[^\n]*favicon/i);
  });

  it("wires the favicon into no page yet", () => {
    const hits: string[] = [];
    const scan = (p: string): void => {
      if (statSync(p).isDirectory()) {
        for (const name of readdirSync(p)) scan(join(p, name));
      } else if (/favicon(-16|-32)?\.(svg|png)|assets\/brand/i.test(text(p))) {
        hits.push(relative(desktopDir, p));
      }
    };
    scan(join(desktopDir, "src"));
    scan(join(desktopDir, "index.html"));
    expect(hits).toEqual([]);
  });
});
