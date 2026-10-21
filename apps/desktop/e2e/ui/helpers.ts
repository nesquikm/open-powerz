/**
 * Shared steps of the Playwright UI suite (FR AH2AEN).
 *
 * Scripted mock scenarios the suite relies on, chosen with `?mock=<name>` in
 * the page URL (implemented in `src/lib/mockBackend.ts`, mock builds only):
 *
 * - `permission-denied`: `connect()` rejects with the Linux permission error
 *   the backend builds (`kind: "permission_denied"`, the message and the udev
 *   hint of `src-tauri/src/ui_error.rs`), as on a Linux host without the rule.
 * - `rate-locked`: authentication fails, so the meter keeps its own rate: the
 *   `connected` status carries `rate_locked: true`, then a `rate` status
 *   reports 50 SPS with `rate_locked: true`; samples stream at 50 SPS.
 * - `slow-export`: an export reports half its rows at once and finishes only
 *   after 30 s, so the top bar's export progress can be checked.
 *
 * `?synthetic=<n>` seeds an n-sample history on connect, in dev builds and in
 * this mock production build alike (`syntheticFromUrl`).
 *
 * `E2E_WIDE_FONT=1` runs the suite with a wide font forced on the page
 * (Verdana, else DejaVu Sans), wider than the defaults of CI Linux (DejaVu
 * Sans) and Windows (Segoe UI), so layout problems those fonts cause show up
 * on a Mac too.
 */

import { expect, type Page } from "@playwright/test";
import { LINUX_PERMISSION_DENIED } from "../../src/lib/mockBackend";
import { HOOK, type ChartDraw, type ChartGapBand, type OpenPowerzE2E } from "../hook";

export const DEMO_LOAD = "demo:load-5v-1a";
export const DEMO_LOAD_LABEL = "Demo device — load-5v-1a";

/** The Linux permission hint the backend sends (`src-tauri/src/ui_error.rs`), as the mock
 * backend sends it; `mockBackend.test.ts` checks the mock's copy against the Rust source. */
export const LINUX_PERMISSION_HINT = LINUX_PERMISSION_DENIED.hint ?? "";

export const PERMISSION_MESSAGE = LINUX_PERMISSION_DENIED.message;

export type Theme = "light" | "dark";

/** Persist `theme` before the app loads, as a returning user would have it. */
export async function useTheme(page: Page, theme: Theme): Promise<void> {
  await page.addInitScript((t) => localStorage.setItem("open-powerz.theme", t), theme);
}

/** Forced on every page when `E2E_WIDE_FONT=1`; font sizes stay as the app sets them. */
const WIDE_FONT_CSS = `html, body, button, select, input, progress {
  font-family: Verdana, "DejaVu Sans", sans-serif !important;
}`;

/** Load the app and wait until the picker lists the demo devices. */
export async function open(page: Page, query = ""): Promise<void> {
  await page.goto(`/${query}`);
  if (process.env.E2E_WIDE_FONT === "1") await page.addStyleTag({ content: WIDE_FONT_CSS });
  await expect(page.getByLabel("Device", { exact: true })).toBeEnabled();
  await expect(
    page.getByLabel("Device", { exact: true }).locator(`option[value="${DEMO_LOAD}"]`),
  ).toHaveCount(1);
}

export function badge(page: Page) {
  return page.locator(".badge");
}

/** Pick the 5 V / 1 A demo device and connect. */
export async function connectDemo(page: Page, id = DEMO_LOAD): Promise<void> {
  await page.getByLabel("Device", { exact: true }).selectOption(id);
  await page.getByRole("button", { name: "Connect" }).click();
}

/** Connect and wait until the badge says Streaming. */
export async function streamDemo(page: Page, id = DEMO_LOAD): Promise<void> {
  await connectDemo(page, id);
  await expect(badge(page)).toHaveText("Streaming");
}

/** The number a readout value shows (`VBUS`, `IBUS`, …), or NaN while it is a dash. */
export async function readoutValue(page: Page, name: string): Promise<number> {
  const text = await page
    .getByRole("region", { name: "Live readout" })
    .getByLabel(name, { exact: true })
    .locator(".digits")
    .innerText();
  // The UI shows negatives with U+2212; read it back as a plain minus.
  return Number.parseFloat(text.replace(/\u2212/g, "-").replace(/[^\d.+-]/g, ""));
}

/** Wait until readout `name` is within `tolerance` (relative) of `target`. */
export async function expectReadoutNear(
  page: Page,
  name: string,
  target: number,
  tolerance: number,
): Promise<void> {
  await expect
    .poll(async () => Math.abs((await readoutValue(page, name)) - target) / target, {
      message: `${name} within ${tolerance * 100}% of ${target}`,
      timeout: 5_000,
    })
    .toBeLessThanOrEqual(tolerance);
}

/** The Elapsed figure of the stats panel, in seconds. */
export async function statsElapsedSeconds(page: Page): Promise<number> {
  const text = await page
    .getByRole("complementary", { name: "Statistics" })
    .locator("dt", { hasText: "Elapsed" })
    .locator("xpath=following-sibling::dd[1]")
    .innerText();
  const [h, m, s] = text.trim().split(":").map(Number);
  return (h ?? NaN) * 3600 + (m ?? NaN) * 60 + (s ?? NaN);
}

/** Move focus off any control, so a key press reaches the window's shortcuts. */
export async function blur(page: Page): Promise<void> {
  await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
}

/** Fail unless the build exposes the e2e test hook. */
export async function expectHook(page: Page): Promise<void> {
  const installed = await page.evaluate((name) => name in window, HOOK);
  expect(installed, `window.${HOOK} is installed in the test build`).toBe(true);
}

/** Points the chart has drawn per series, read through the test hook. */
export async function chartSeriesLengths(page: Page): Promise<{ vbus: number; ibus: number }> {
  await expectHook(page);
  return page.evaluate(
    (name) => (window as unknown as Record<string, OpenPowerzE2E>)[name]!.chartSeriesLengths(),
    HOOK,
  );
}

/** Every chart draw so far, read through the test hook. */
export async function chartDraws(page: Page): Promise<ChartDraw[]> {
  await expectHook(page);
  return page.evaluate(
    (name) => (window as unknown as Record<string, OpenPowerzE2E>)[name]!.chartDraws(),
    HOOK,
  );
}

/** The time-axis labels of the chart's last draw, read through the test hook. */
export async function chartXTicks(page: Page): Promise<string[]> {
  await expectHook(page);
  return page.evaluate(
    (name) => (window as unknown as Record<string, OpenPowerzE2E>)[name]!.chartXTicks(),
    HOOK,
  );
}

/** The gaps the chart's last draw shaded, read through the test hook. */
export async function chartGapBands(page: Page): Promise<ChartGapBand[]> {
  await expectHook(page);
  return page.evaluate(
    (name) => (window as unknown as Record<string, OpenPowerzE2E>)[name]!.chartGapBands(),
    HOOK,
  );
}
