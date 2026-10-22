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
import {
  HOOK,
  type ChartBreak,
  type ChartDraw,
  type ChartGapBand,
  type OpenPowerzE2E,
  type OverviewWindowBox,
  RELOAD_HOOK,
  type ReloadHook,
} from "../hook";

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

/** Calls one accessor of the test hook in the page. */
async function hookCall<K extends keyof OpenPowerzE2E>(
  page: Page,
  method: K,
): Promise<ReturnType<OpenPowerzE2E[K]>> {
  await expectHook(page);
  return page.evaluate(
    ([name, m]) => {
      const h = (window as unknown as Record<string, OpenPowerzE2E>)[name]!;
      const fn = h[m as keyof OpenPowerzE2E] as unknown;
      if (typeof fn !== "function") throw new Error(`the test hook has no ${m}()`);
      return (fn as () => unknown).call(h);
    },
    [HOOK, method] as const,
  ) as Promise<ReturnType<OpenPowerzE2E[K]>>;
}

/** The chart's playhead (session ms) or null, read through the test hook (FR FENCD9). */
export function chartPlayhead(page: Page): Promise<number | null> {
  return hookCall(page, "chartPlayhead");
}

/** The J/K/L shuttle speed, read through the test hook (FR FENCD9). */
export function chartShuttle(page: Page): Promise<number> {
  return hookCall(page, "chartShuttle");
}

/** The collapsed gaps of the chart's last draw, read through the test hook (FR FENCD9). */
export function chartBreaks(page: Page): Promise<ChartBreak[]> {
  return hookCall(page, "chartBreaks");
}

/** The overview strip's viewport window, read through the test hook (FR FENCD9). */
export function overviewWindow(page: Page): Promise<OverviewWindowBox | null> {
  return hookCall(page, "overviewWindow");
}

/**
 * The chart's last draw, read through the test hook.
 *
 * AC-KQF5GM.11: a paused (history) view keeps its range until input changes it, so a
 * history draw is returned once its range has held for a few animation frames.
 * Without that, a read right after a drag can catch a draw for one of the drag's
 * intermediate steps while the later input and its `history_range` answer are still on
 * their way. A live view redraws every frame and is returned as read.
 */
export async function lastDraw(page: Page): Promise<ChartDraw> {
  let d = (await chartDraws(page)).at(-1);
  if (!d) throw new Error("the chart has not drawn yet");
  for (let i = 0; i < 20 && d.mode === "history"; i++) {
    await animationFrames(page, 3);
    const next: ChartDraw | undefined = (await chartDraws(page)).at(-1);
    if (!next) throw new Error("the chart has not drawn yet");
    const same =
      next.mode === d.mode &&
      next.t0Ms === d.t0Ms &&
      next.t1Ms === d.t1Ms &&
      next.fromHistory === d.fromHistory;
    d = next;
    if (same) break;
  }
  return d;
}

/** The collapsed gaps the overview strip drew, read through the test hook. */
export async function overviewBreaks(
  page: Page,
): Promise<{ leftPx: number; widthPx: number; label: string }[]> {
  await expectHook(page);
  return page.evaluate(
    (name) => (window as unknown as Record<string, OpenPowerzE2E>)[name]!.overviewBreaks(),
    HOOK,
  );
}

/** Reload the app the way the window reload does (FR VEDDFD): a fresh app on the same
 * page while the backend, here the in-page mock, keeps running (see `RELOAD_HOOK`).
 * Waits until the new app is on screen and its picker lists the demo devices. */
export async function reloadApp(page: Page): Promise<void> {
  const installed = await page.evaluate(
    (name) => typeof (window as unknown as Record<string, unknown>)[name] === "function",
    RELOAD_HOOK,
  );
  expect(installed, `window.${RELOAD_HOOK} is installed in the mock build`).toBe(true);
  // Remember the old app's top bar, to tell the fresh one apart from it.
  await page.evaluate(() => {
    (window as unknown as Record<string, unknown>).__preReloadBadge =
      document.querySelector(".badge");
  });
  await page.evaluate(
    (name) => (window as unknown as Record<string, ReloadHook>)[name]!(),
    RELOAD_HOOK,
  );
  await expect
    .poll(() =>
      page.evaluate(() => {
        const now = document.querySelector(".badge");
        return (
          now !== null && now !== (window as unknown as Record<string, unknown>).__preReloadBadge
        );
      }),
    )
    .toBe(true);
  await expect(
    page.getByLabel("Device", { exact: true }).locator(`option[value="${DEMO_LOAD}"]`),
  ).toHaveCount(1);
}

/** The recording timer's elapsed time in seconds (`REC h:mm:ss`), or NaN when hidden. */
export async function recordingElapsedSeconds(page: Page): Promise<number> {
  const timer = page.getByRole("timer");
  if ((await timer.count()) === 0) return NaN;
  const m = /REC (\d+):(\d{2}):(\d{2})/.exec(await timer.innerText());
  if (!m) return NaN;
  return Number(m[1]) * 3600 + Number(m[2]) * 60 + Number(m[3]);
}

/**
 * AC-KQF5GM.11: waits for the page to have painted `count` more animation frames. A
 * test that checks that something did *not* happen (a key that must do nothing, a
 * stopped playhead) gives the app that many frames to react instead of a fixed sleep;
 * the frames tick at the page's own pace, however slow the runner is.
 */
export async function animationFrames(page: Page, count = 5): Promise<void> {
  await page.evaluate(
    (n) =>
      new Promise<void>((resolve) => {
        let left = n;
        const tick = (): void => {
          left -= 1;
          if (left <= 0) resolve();
          else requestAnimationFrame(tick);
        };
        requestAnimationFrame(tick);
      }),
    count,
  );
}

/**
 * AC-KQF5GM.11: reads `read` until two reads in a row, a couple of animation frames
 * apart, agree, and returns that value: a wait for layout to settle instead of a fixed
 * "let it settle" sleep.
 */
export async function settled<T>(page: Page, read: () => Promise<T>, timeout = 10_000): Promise<T> {
  let value = await read();
  let previous = JSON.stringify(value);
  await expect
    .poll(
      async () => {
        await animationFrames(page, 2);
        value = await read();
        const now = JSON.stringify(value);
        const same = now === previous;
        previous = now;
        return same;
      },
      { timeout, intervals: [0], message: "the layout settles" },
    )
    .toBe(true);
  return value;
}

/** AC-KQF5GM.11: waits until the chart has drawn `count` times after this call, so the
 * app has redrawn with whatever just changed. */
export async function chartDrawsAfterNow(page: Page, count = 2): Promise<void> {
  const since = await page.evaluate(() => performance.now());
  await expect
    .poll(async () => (await chartDraws(page)).filter((d) => d.at > since).length, {
      message: `${count} chart draws`,
    })
    .toBeGreaterThanOrEqual(count);
}

/** The newest session ms of the chart's last live draw, or -1 before any. */
export async function newestLiveMs(page: Page): Promise<number> {
  const draws = await chartDraws(page);
  return [...draws].reverse().find((d) => d.mode === "live")?.t1Ms ?? -1;
}

/** AC-KQF5GM.11: waits until the live stream has drawn `ms` more session ms of data
 * than it had when this was called: a wait for data, not for wall time. */
export async function streamOn(page: Page, ms: number, timeout = 30_000): Promise<void> {
  const from = await newestLiveMs(page);
  await expect
    .poll(() => newestLiveMs(page), { timeout, message: `${ms} ms more of the stream` })
    .toBeGreaterThanOrEqual(from + ms);
}

/**
 * Pauses the running page clock a moment ahead of its current time. The clock keeps
 * running while the pause request travels to the page, so a target that close can be
 * in the past by the time it lands; then retry with a wider lead.
 */
export async function holdClock(page: Page): Promise<void> {
  for (let leadMs = 50; ; leadMs *= 2) {
    const target = (await page.evaluate(() => Date.now())) + leadMs;
    try {
      await page.clock.pauseAt(target);
      return;
    } catch (e) {
      if (leadMs >= 3_200 || !String(e).includes("Cannot fast-forward to the past")) throw e;
    }
  }
}
