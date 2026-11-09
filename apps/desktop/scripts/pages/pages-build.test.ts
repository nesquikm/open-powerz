/**
 * FR QFTCX1: the web edition builds into a static site for GitHub Pages.
 *
 * AC-QFTCX1.1: `bun run build:pages` (Vite mode `pages`: base `/open-powerz/`,
 * `VITE_BACKEND=web`, output folder `dist-pages`) produces a folder whose every asset URL
 * resolves when it is served at `/open-powerz/`, with the wasm module in it.
 * AC-QFTCX1.2: the page carries the brand favicon (SVG plus 16 / 32 px PNG, copied from
 * assets/brand at build time, not duplicated in the app) and a title and description for
 * the web edition.
 * AC-QFTCX1.4 (harness): an `e2e:pages` script runs the Playwright suite of the built site.
 *
 * The built-output checks run one real `vite build --mode pages` into a temporary folder;
 * the wasm module comes from `bun run build:wasm`, which the gate runs before Vitest.
 */

import { execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { loadEnv, type UserConfig } from "vite";
import { afterAll, beforeAll, describe, expect, it } from "vitest";

const DESKTOP = fileURLToPath(new URL("../../", import.meta.url));
const REPO_ROOT = join(DESKTOP, "../..");
const BRAND = join(REPO_ROOT, "assets/brand");
const BASE = "/open-powerz/";

const scripts = (): Record<string, string> =>
  (
    JSON.parse(readFileSync(join(DESKTOP, "package.json"), "utf8")) as {
      scripts?: Record<string, string>;
    }
  ).scripts ?? {};

/** The Vite config as resolved for `mode` (the config may be an object or a function). */
async function configFor(mode: string): Promise<UserConfig> {
  const mod = (await import("../../vite.config")) as { default: unknown };
  const c = mod.default;
  if (typeof c === "function") {
    return (await (c as (env: object) => UserConfig | Promise<UserConfig>)({
      mode,
      command: "build",
      isSsrBuild: false,
      isPreview: false,
    })) as UserConfig;
  }
  return c as UserConfig;
}

/** `VITE_BACKEND` as the `pages` mode sets it: a `.env.pages` file or a config `define`. */
async function pagesBackend(): Promise<string | undefined> {
  const saved = process.env.VITE_BACKEND;
  delete process.env.VITE_BACKEND;
  try {
    const fromEnv = loadEnv("pages", DESKTOP, "VITE_").VITE_BACKEND;
    if (fromEnv !== undefined) return fromEnv;
    const define = (await configFor("pages")).define ?? {};
    const raw = define["import.meta.env.VITE_BACKEND"];
    return typeof raw === "string" ? (JSON.parse(raw) as string) : undefined;
  } finally {
    if (saved !== undefined) process.env.VITE_BACKEND = saved;
  }
}

describe("the pages build mode (AC-QFTCX1.1)", () => {
  it("has a build:pages script that builds in Vite mode pages", () => {
    const cmd = scripts()["build:pages"] ?? "";
    expect(cmd, "package.json has no build:pages script").not.toBe("");
    expect(cmd).toMatch(/vite build\b[^&|;]*--mode[ =]pages\b/);
  });

  it("serves from /open-powerz/ and writes dist-pages in mode pages", async () => {
    const cfg = await configFor("pages");
    expect(cfg.base).toBe(BASE);
    expect(cfg.build?.outDir).toBe("dist-pages");
  });

  it("routes the backend to the browser backend in mode pages", async () => {
    expect(await pagesBackend()).toBe("web");
  });

  it("leaves the desktop build at the root and in dist", async () => {
    const cfg = await configFor("production");
    expect(cfg.base ?? "/").toBe("/");
    expect(cfg.build?.outDir ?? "dist").toBe("dist");
  });

  it("git-ignores the dist-pages folder", () => {
    const ignore = readFileSync(join(REPO_ROOT, ".gitignore"), "utf8");
    expect(ignore).toMatch(/^\/?apps\/desktop\/dist-pages\/?\s*$/m);
  });

  it("has an e2e:pages script that runs the pages Playwright suite (AC-QFTCX1.4)", () => {
    expect(scripts()["e2e:pages"] ?? "").toMatch(
      /playwright test\b.*-c\s+e2e\/pages\/playwright\.config\.ts/,
    );
  });
});

describe("the brand icons are not duplicated in the app (AC-QFTCX1.2)", () => {
  it("keeps no copy of the favicons under apps/desktop", () => {
    const brand = ["favicon.svg", "favicon-16.png", "favicon-32.png"].map((f) =>
      readFileSync(join(BRAND, f)),
    );
    const skip = new Set(["node_modules", "dist", "dist-e2e-ui", "dist-e2e-app", "dist-pages"]);
    const copies: string[] = [];
    const walk = (dir: string): void => {
      for (const name of readdirSync(dir)) {
        if (skip.has(name) || name.startsWith(".")) continue;
        const p = join(dir, name);
        if (relative(DESKTOP, p).startsWith("src-tauri")) continue;
        if (relative(DESKTOP, p).startsWith("test-results")) continue;
        const st = statSync(p);
        if (st.isDirectory()) walk(p);
        else if (/\.(svg|png)$/.test(name) && brand.some((b) => b.equals(readFileSync(p))))
          copies.push(relative(DESKTOP, p));
      }
    };
    walk(DESKTOP);
    expect(copies).toEqual([]);
  });
});

describe("the built site (AC-QFTCX1.1, AC-QFTCX1.2)", () => {
  let out = "";
  let html = "";
  let buildError = "";

  beforeAll(() => {
    out = mkdtempSync(join(tmpdir(), "open-powerz-pages-"));
    const env = { ...process.env };
    delete env.VITE_BACKEND;
    try {
      execFileSync("bunx", ["vite", "build", "--mode", "pages", "--outDir", out, "--emptyOutDir"], {
        cwd: DESKTOP,
        env,
        stdio: "pipe",
      });
      html = readFileSync(join(out, "index.html"), "utf8");
    } catch (e) {
      buildError = String((e as { stderr?: Buffer }).stderr ?? e);
    }
  }, 240_000);

  afterAll(() => {
    if (out) rmSync(out, { recursive: true, force: true });
  });

  /** Every `src` / `href` URL in index.html that points into the site (not http/data/#). */
  const localUrls = (): string[] =>
    [...html.matchAll(/\s(?:src|href)="([^"]+)"/g)]
      .map((m) => m[1] ?? "")
      .filter((u) => !/^(?:[a-z]+:|#|\/\/)/i.test(u));

  /** The file `url` names when the folder is served at /open-powerz/, or null outside it. */
  const fileFor = (url: string): string | null => {
    const path = url.split(/[?#]/)[0] ?? "";
    if (path.startsWith("/"))
      return path.startsWith(BASE) ? join(out, path.slice(BASE.length)) : null;
    return join(out, path);
  };

  it("builds", () => {
    expect(buildError).toBe("");
    expect(html).toContain('<div id="app"');
  });

  it("points every asset URL under /open-powerz/ and each one exists", () => {
    const urls = localUrls();
    expect(urls.length).toBeGreaterThan(2);
    const unresolved = urls.filter((u) => {
      const f = fileFor(u);
      return f === null || !existsSync(f);
    });
    expect(unresolved, "asset URLs that do not resolve under /open-powerz/").toEqual([]);
    expect(urls.filter((u) => u.startsWith("/") && !u.startsWith(BASE))).toEqual([]);
  });

  it("bundles the wasm module (the web backend is in)", () => {
    const files: string[] = [];
    const walk = (dir: string): void => {
      for (const n of readdirSync(dir)) {
        const p = join(dir, n);
        if (statSync(p).isDirectory()) walk(p);
        else files.push(n);
      }
    };
    walk(out);
    expect(files.some((f) => f.endsWith(".wasm"))).toBe(true);
  });

  it("links the SVG, 16 px and 32 px brand favicons, copied from assets/brand", () => {
    const icons = [...html.matchAll(/<link\b[^>]*\brel="icon"[^>]*>/g)].map((m) => m[0]);
    const attr = (tag: string, name: string): string =>
      new RegExp(`\\s${name}="([^"]*)"`).exec(tag)?.[1] ?? "";
    const want: [type: string, sizes: string | null, brand: string][] = [
      ["image/svg+xml", null, "favicon.svg"],
      ["image/png", "16x16", "favicon-16.png"],
      ["image/png", "32x32", "favicon-32.png"],
    ];
    for (const [type, sizes, brand] of want) {
      const tag = icons.find(
        (t) => attr(t, "type") === type && (sizes === null || attr(t, "sizes") === sizes),
      );
      expect(
        tag,
        `a <link rel="icon" type="${type}"${sizes ? ` sizes="${sizes}"` : ""}>`,
      ).toBeDefined();
      const file = fileFor(attr(tag ?? "", "href"));
      expect(file, `${brand} link resolves under ${BASE}`).not.toBeNull();
      expect(existsSync(file ?? ""), `${brand} is in the built folder`).toBe(true);
      expect(readFileSync(file ?? "").equals(readFileSync(join(BRAND, brand)))).toBe(true);
    }
    // The desktop placeholder icon is gone from the web edition.
    expect(html).not.toContain('href="data:,"');
  });

  it("has a title and a description for the web edition", () => {
    const title = /<title>([^<]*)<\/title>/.exec(html)?.[1] ?? "";
    expect(title).toMatch(/open-powerz/i);
    expect(title).toMatch(/web|browser/i);
    const description =
      /<meta\b[^>]*\bname="description"[^>]*\bcontent="([^"]+)"/.exec(html)?.[1] ??
      /<meta\b[^>]*\bcontent="([^"]+)"[^>]*\bname="description"/.exec(html)?.[1] ??
      "";
    expect(description).toMatch(/KM003C/);
    expect(description).toMatch(/browser/i);
  });
});
