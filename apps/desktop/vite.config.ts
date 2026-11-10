import { defineConfig } from "vitest/config";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import type { Plugin } from "vite";

// Tauri sets TAURI_DEV_HOST when running `tauri dev` against a device.
const host = process.env.TAURI_DEV_HOST;

const BRAND = fileURLToPath(new URL("../../assets/brand/", import.meta.url));

/**
 * The web edition's page head (FR QFTCX1, AC-QFTCX1.2): emits the brand favicons from
 * assets/brand at build time (no copies in the app) and swaps the desktop placeholder icon,
 * title and description for the web edition's.
 */
function pagesHead(): Plugin {
  let base = "/";
  const icons = [
    { file: "favicon.svg", type: "image/svg+xml", sizes: null },
    { file: "favicon-16.png", type: "image/png", sizes: "16x16" },
    { file: "favicon-32.png", type: "image/png", sizes: "32x32" },
  ];
  return {
    name: "open-powerz-pages-head",
    apply: "build",
    configResolved(cfg) {
      base = cfg.base;
    },
    generateBundle() {
      for (const { file } of icons)
        this.emitFile({ type: "asset", fileName: file, source: readFileSync(BRAND + file) });
    },
    transformIndexHtml(html) {
      const links = icons
        .map(
          ({ file, type, sizes }) =>
            `<link rel="icon" type="${type}"${sizes ? ` sizes="${sizes}"` : ""} href="${base}${file}" />`,
        )
        .join("\n    ");
      return html
        .replace(/<!--[^>]*favicon[^>]*-->\s*/, "")
        .replace(/<link rel="icon" href="data:," \/>/, links)
        .replace(
          /<title>[^<]*<\/title>/,
          "<title>open-powerz web: POWER-Z meter in the browser</title>\n    " +
            '<meta name="description" content="Read a ChargerLAB POWER-Z KM003C / KM002C ' +
            'USB-C power meter right in the browser (Chrome or Edge, WebUSB / WebHID), no install." />',
        );
    },
  };
}

// Mode `pages` (FR QFTCX1) builds the web edition for GitHub Pages: served from
// /open-powerz/, written to dist-pages; `.env.pages` sets VITE_BACKEND=web.
export default defineConfig(({ mode }) => {
  const pages = mode === "pages";
  // Cross-origin isolation for the web edition's trigger worker (FR W8SHXC); on GitHub
  // Pages public/coi-sw.js adds the same headers.
  const isolation = pages
    ? {
        "Cross-Origin-Opener-Policy": "same-origin",
        "Cross-Origin-Embedder-Policy": "require-corp",
      }
    : undefined;
  return {
    plugins: [svelte(), ...(pages ? [pagesHead()] : [])],
    ...(pages ? { base: "/open-powerz/", build: { outDir: "dist-pages" } } : {}),
    // public/ holds only the web edition's coi-sw.js (FR W8SHXC): the desktop bundle
    // and the mock build never ship it.
    publicDir: pages ? "public" : false,
    // The trigger worker imports the wasm glue lazily: ES module workers split chunks.
    worker: { format: "es" },
    preview: { headers: isolation },
    // Keep Rust errors visible in the terminal.
    clearScreen: false,
    server: {
      port: 1420,
      strictPort: true,
      host: host || false,
      hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
      watch: { ignored: ["**/src-tauri/**"] },
      headers: isolation,
    },
    test: {
      include: ["src/**/*.test.ts", "scripts/**/*.test.ts", "e2e/**/*.test.ts"],
      passWithNoTests: true,
    },
  };
});
