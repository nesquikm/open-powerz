/**
 * Playwright suite of the built web edition (FR QFTCX1, AC-QFTCX1.4): `bun run
 * build:pages` output (`dist-pages/`, base path `/open-powerz/`, the browser backend
 * with the demo meter) served under `/open-powerz/` by `serve.ts`, as GitHub Pages
 * serves it.
 *
 * Chromium (WebUSB and WebHID present) drives the demo meter end to end; WebKit (no
 * WebUSB, no WebHID, as Safari) must see the "not supported" view.
 *
 * Run: `bun run e2e:pages` from apps/desktop (needs `bun run build:wasm` first). One
 * worker: the specs stream the demo meter for a while.
 */

import { defineConfig, devices } from "@playwright/test";

const PORT = 4174;
const BASE_URL = `http://127.0.0.1:${PORT}/open-powerz/`;
const chromiumChannel = process.env.PLAYWRIGHT_CHROMIUM_CHANNEL;

export default defineConfig({
  testDir: ".",
  testMatch: "**/*.spec.ts",
  fullyParallel: false,
  workers: 1,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  timeout: 60_000,
  reporter: process.env.CI ? [["list"], ["html", { open: "never" }]] : "list",
  use: {
    baseURL: BASE_URL,
    trace: "retain-on-failure",
    acceptDownloads: true,
  },
  projects: [
    {
      name: "chromium",
      testIgnore: "**/unsupported.spec.ts",
      use: {
        ...devices["Desktop Chrome"],
        ...(chromiumChannel ? { channel: chromiumChannel } : {}),
      },
    },
    {
      name: "webkit",
      testMatch: "**/unsupported.spec.ts",
      use: { ...devices["Desktop Safari"] },
    },
  ],
  webServer: {
    // Run from apps/desktop. The pages mode sets the base path, the backend and the folder.
    command: `bun run build:pages && bun e2e/pages/serve.ts dist-pages ${PORT}`,
    cwd: "../..",
    url: BASE_URL,
    reuseExistingServer: !process.env.CI,
    timeout: 240_000,
  },
});
