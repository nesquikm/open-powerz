/**
 * Playwright UI suite (FR AH2AEN, AC .5 / .6, and the deferred AC-AH2AEM.4
 * pan timing): the production web build made with `VITE_BACKEND=mock` (the
 * scripted mock backend, test hook on `window`) served by `vite preview`, in
 * Chromium and WebKit.
 *
 * Run: `bun run e2e:ui` (or `bunx playwright test -c e2e/ui/playwright.config.ts`
 * from apps/desktop). CI uses Playwright's bundled browsers
 * (`bunx playwright install --with-deps chromium webkit`); locally,
 * `PLAYWRIGHT_CHROMIUM_CHANNEL=chrome` runs the Chromium project on the
 * installed Google Chrome instead of a downloaded browser.
 *
 * The build goes to `dist-e2e-ui/`, never `dist/`, so the mock build can
 * never be embedded into the Tauri app by a later cargo build.
 */

import { defineConfig, devices } from "@playwright/test";

const PORT = 4173;
const BASE_URL = `http://127.0.0.1:${PORT}/`;
const OUT_DIR = "dist-e2e-ui";
const chromiumChannel = process.env.PLAYWRIGHT_CHROMIUM_CHANNEL;

export default defineConfig({
  testDir: ".",
  testMatch: "**/*.spec.ts",
  fullyParallel: true,
  // One worker locally: parallel WebKit pages streaming for minutes exhausted a dev
  // machine's memory. CI keeps Playwright's default.
  workers: process.env.CI ? undefined : 1,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  timeout: 60_000,
  reporter: process.env.CI ? [["list"], ["html", { open: "never" }]] : "list",
  use: {
    baseURL: BASE_URL,
    trace: "retain-on-failure",
  },
  projects: [
    {
      name: "chromium",
      use: {
        ...devices["Desktop Chrome"],
        ...(chromiumChannel ? { channel: chromiumChannel } : {}),
      },
    },
    {
      name: "webkit",
      use: { ...devices["Desktop Safari"] },
    },
  ],
  webServer: {
    // Run from apps/desktop; VITE_BACKEND comes from `env`, so this works on every OS.
    command: `bunx vite build --outDir ${OUT_DIR} --emptyOutDir && bunx vite preview --outDir ${OUT_DIR} --port ${PORT} --strictPort --host 127.0.0.1`,
    cwd: "../..",
    env: { VITE_BACKEND: "mock" },
    url: BASE_URL,
    reuseExistingServer: !process.env.CI,
    timeout: 180_000,
  },
});
