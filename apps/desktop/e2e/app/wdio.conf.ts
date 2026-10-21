/**
 * Real-app suite (FR AH2AEN, AC .1–.3): WebdriverIO drives the built desktop
 * app through `tauri-driver` (Tauri 2 WebDriver guide), against the demo
 * device.
 *
 * Linux (WebKitGTK WebDriver, under xvfb in CI) and Windows (Edge WebDriver
 * matching WebView2) only: `tauri-driver` has no macOS support, because
 * WKWebView offers no WebDriver.
 *
 * Run: `bun run e2e:app` (needs `cargo install tauri-driver --locked`).
 *
 * The app is built with the `e2e` Cargo feature and `VITE_E2E=1`: the web
 * build installs the `window.__OPEN_POWERZ_E2E__` test hook, and the backend
 * writes recordings and exports into `$OPEN_POWERZ_E2E_SAVE_DIR` under the
 * proposed file name instead of opening the save dialog. The web build goes to
 * `dist-e2e-app/` (`tauri.e2e.conf.json`), never `dist/`, so a later plain
 * `cargo build` can never embed the test hook.
 *
 * Logs (WebdriverIO's and tauri-driver's) go to `e2e-app-logs/`; CI uploads
 * them when the job fails.
 */

import { type ChildProcess, spawn, spawnSync } from "node:child_process";
import { createWriteStream, mkdirSync, mkdtempSync, rmSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const desktopDir = path.resolve(here, "../..");
const workspaceDir = path.resolve(desktopDir, "../..");
const exe = process.platform === "win32" ? ".exe" : "";
const application = path.join(workspaceDir, "target", "debug", `open-powerz-desktop${exe}`);

/** Merged over `src-tauri/tauri.conf.json`: the web build goes to `dist-e2e-app/`. */
const e2eTauriConfig = path.join(here, "tauri.e2e.conf.json");
/** WebdriverIO's and tauri-driver's logs (uploaded by CI on failure). */
const logDir = path.join(desktopDir, "e2e-app-logs");

/** Where the e2e build saves recordings and exports; the specs read it back. */
export const SAVE_DIR_ENV = "OPEN_POWERZ_E2E_SAVE_DIR";

let tauriDriver: ChildProcess | undefined;
let exiting = false;

/** Stop tauri-driver and remove the save directory; safe to call more than once. */
function cleanUp(): void {
  exiting = true;
  if (tauriDriver && tauriDriver.exitCode === null && !tauriDriver.killed) tauriDriver.kill();
  const saveDir = process.env[SAVE_DIR_ENV];
  if (saveDir) {
    try {
      rmSync(saveDir, { recursive: true, force: true });
    } catch {
      // Best effort: the OS temp dir is cleaned eventually.
    }
  }
}

// Best effort when the worker dies without `afterSession` (Ctrl-C, a kill, a crash).
process.once("exit", cleanUp);
for (const signal of ["SIGINT", "SIGTERM"] as const) {
  process.once(signal, () => {
    cleanUp();
    // Leave the exit to WebdriverIO's own handler when it has one.
    if (process.listenerCount(signal) === 0) process.exit(signal === "SIGINT" ? 130 : 143);
  });
}

function tauriDriverPath(): string {
  return path.resolve(os.homedir(), ".cargo", "bin", `tauri-driver${exe}`);
}

export const config: WebdriverIO.Config = {
  hostname: "127.0.0.1",
  port: 4444,
  specs: ["./specs/**/*.e2e.ts"],
  maxInstances: 1,
  capabilities: [
    // `tauri:options` is tauri-driver's own capability.
    { maxInstances: 1, "tauri:options": { application } } as WebdriverIO.Capabilities,
  ],
  logLevel: "warn",
  outputDir: logDir,
  reporters: ["spec"],
  framework: "mocha",
  mochaOpts: { ui: "bdd", timeout: 120_000 },

  /** Build the web UI with the hook into `dist-e2e-app/`, then a debug app with the `e2e`
   * feature embedding it. */
  onPrepare: () => {
    mkdirSync(logDir, { recursive: true });
    const build = spawnSync(
      "bun",
      [
        "run",
        "tauri",
        "build",
        "--debug",
        "--no-bundle",
        "--features",
        "e2e",
        "--config",
        e2eTauriConfig,
      ],
      {
        cwd: desktopDir,
        stdio: "inherit",
        shell: process.platform === "win32",
        env: { ...process.env, VITE_E2E: "1" },
      },
    );
    if (build.status !== 0) throw new Error("building the e2e app failed");
  },

  /** A fresh save directory per session, handed to the app through tauri-driver's environment. */
  beforeSession: () => {
    const saveDir = mkdtempSync(path.join(os.tmpdir(), "open-powerz-e2e-"));
    process.env[SAVE_DIR_ENV] = saveDir;
    mkdirSync(logDir, { recursive: true });
    const log = createWriteStream(path.join(logDir, `tauri-driver-${process.pid}.log`));
    tauriDriver = spawn(tauriDriverPath(), [], {
      stdio: ["ignore", "pipe", "pipe"],
      env: { ...process.env, [SAVE_DIR_ENV]: saveDir },
    });
    // Tee tauri-driver's output to the console and to its log file.
    tauriDriver.stdout?.on("data", (chunk: Buffer) => {
      process.stdout.write(chunk);
      log.write(chunk);
    });
    tauriDriver.stderr?.on("data", (chunk: Buffer) => {
      process.stderr.write(chunk);
      log.write(chunk);
    });
    tauriDriver.on("close", () => log.end());
    tauriDriver.on("error", (e) => {
      console.error("tauri-driver failed to start:", e);
      process.exit(1);
    });
    tauriDriver.on("exit", (code) => {
      if (!exiting) {
        console.error("tauri-driver exited early with code", code);
        process.exit(1);
      }
    });
  },

  afterSession: () => cleanUp(),
};
