/**
 * FR VEDDFD: a reload of the window picks the running session up again.
 *
 * The mock backend lives in the page, so a real `page.reload()` would take it
 * along; `reloadApp` (helpers.ts) uses the mock build's reload hook instead,
 * which mounts a fresh app on the same page while the mock and its running
 * session carry on, as the real backend does across a webview reload.
 *
 * - AC-VEDDFD.1: with nothing running, the new app starts disconnected;
 * - AC-VEDDFD.2: with a session running, it re-attaches without reconnecting;
 * - AC-VEDDFD.3: an open recording shows as recording, with its elapsed time;
 * - AC-VEDDFD.4: "Whole session" and the statistics reach back before the reload;
 * - AC-VEDDFD.5: Disconnect, Stop, rate changes and Export work afterwards;
 * - AC-VEDDFD.6: the live 10 s, 1 min and 10 min windows reach back before the reload
 *   straight away, and the zoom readout names the span drawn.
 */

import { expect, test, type Page } from "@playwright/test";
import { formatSpan } from "../../src/lib/chartSeries";
import { HOOK, type ChartDraw, type OpenPowerzE2E } from "../hook";
import {
  badge,
  chartDraws,
  chartSeriesLengths,
  expectReadoutNear,
  open,
  recordingElapsedSeconds,
  reloadApp,
  statsElapsedSeconds,
  streamDemo,
} from "./helpers";

test.use({ viewport: { width: 1600, height: 900 } });

/** Not the picker's first entry, so a picker that forgot the session shows another one. */
const DEMO_PD = "demo:pd-9v-2a";

test("with no session running the reloaded app starts disconnected (AC-VEDDFD.1)", async ({
  page,
}) => {
  await open(page);
  await reloadApp(page);
  await expect(badge(page)).toHaveText("Disconnected");
  await expect(page.getByRole("button", { name: "Connect", exact: true })).toBeEnabled();
  await expect(page.getByRole("alert")).toHaveCount(0);
  // Connect works as on a first load.
  await streamDemo(page);
});

test("a reload while streaming re-attaches to the session (AC-VEDDFD.2, AC-VEDDFD.4, AC-VEDDFD.5)", async ({
  page,
}) => {
  await open(page);
  await streamDemo(page, DEMO_PD);
  await expect.poll(() => statsElapsedSeconds(page), { timeout: 8_000 }).toBeGreaterThanOrEqual(2);
  const elapsedBefore = await statsElapsedSeconds(page);

  await reloadApp(page);

  // The session is still up: no reconnect, no "already connected" error.
  await expect(badge(page)).toHaveText("Streaming");
  await expect(page.getByRole("alert")).toHaveCount(0);
  await expect(page.getByLabel("Device", { exact: true })).toHaveValue(DEMO_PD);
  await expect(page.getByLabel("Sample rate")).toHaveValue("50");
  await expect(page.getByRole("button", { name: "Disconnect", exact: true })).toBeEnabled();

  // New batches reach the new page: the readout and the chart move.
  await expectReadoutNear(page, "VBUS", 9, 0.05);
  const drawn = (await chartSeriesLengths(page)).vbus;
  await expect.poll(async () => (await chartSeriesLengths(page)).vbus).toBeGreaterThan(drawn);

  // The statistics and "Whole session" reach back before the reload.
  await expect.poll(() => statsElapsedSeconds(page)).toBeGreaterThanOrEqual(elapsedBefore);
  await page.getByRole("button", { name: "Whole session" }).click();
  await expect.poll(async () => (await chartDraws(page)).at(-1)?.t0Ms, { timeout: 5_000 }).toBe(0);
  expect((await chartDraws(page)).at(-1)?.t1Ms ?? 0).toBeGreaterThanOrEqual(elapsedBefore * 1000);

  // A rate change and an export work on the re-attached session.
  await page.getByLabel("Sample rate").selectOption("10");
  await expect(page.getByLabel("Sample rate")).toHaveValue("10");
  await expect(badge(page)).toHaveText("Streaming");
  const exportButton = page.getByRole("button", { name: "Export", exact: true });
  await expect(exportButton).toBeEnabled();
  await exportButton.click();
  await expect(page.getByRole("alert")).toHaveCount(0);

  // Disconnect ends it, and a new connect is accepted afterwards.
  await page.getByRole("button", { name: "Disconnect", exact: true }).click();
  await expect(badge(page)).toHaveText("Disconnected");
  await expect(page.getByRole("alert")).toHaveCount(0);
  await streamDemo(page);
});

test("a reload while recording keeps the recording running (AC-VEDDFD.3, AC-VEDDFD.5)", async ({
  page,
}) => {
  await open(page);
  await streamDemo(page);
  await page.getByRole("button", { name: "Record", exact: true }).click();
  await expect(page.getByRole("button", { name: "Stop", exact: true })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await expect
    .poll(() => recordingElapsedSeconds(page), { timeout: 8_000 })
    .toBeGreaterThanOrEqual(2);
  const before = await recordingElapsedSeconds(page);

  await reloadApp(page);

  await expect(badge(page)).toHaveText("Streaming");
  const stop = page.getByRole("button", { name: "Stop", exact: true });
  await expect(stop).toHaveAttribute("aria-pressed", "true");
  await expect(page.getByRole("timer")).toBeVisible();
  // The timer picks up the backend's elapsed time, not zero.
  expect(await recordingElapsedSeconds(page)).toBeGreaterThanOrEqual(before);
  await expect(page.getByRole("timer")).toContainText(/[1-9][\d,]* rows/);
  await expect
    .poll(() => recordingElapsedSeconds(page), { timeout: 5_000 })
    .toBeGreaterThan(before);

  // Stop ends the re-attached recording.
  await stop.click();
  await expect(page.getByRole("button", { name: "Record", exact: true })).toHaveAttribute(
    "aria-pressed",
    "false",
  );
  await expect(page.getByRole("timer")).toHaveCount(0);
  await expect(page.getByRole("alert")).toHaveCount(0);
});

/** The chart's last live draw and the zoom readout, read at the same moment. */
async function liveDrawAndZoom(page: Page): Promise<{ draw: ChartDraw | null; zoom: string }> {
  return page.evaluate((name) => {
    const hook = (window as unknown as Record<string, OpenPowerzE2E>)[name];
    const draws = hook ? hook.chartDraws() : [];
    const draw = [...draws].reverse().find((d) => d.mode === "live") ?? null;
    const zoom = document.querySelector(".zoom-span")?.textContent?.trim() ?? "";
    return { draw, zoom };
  }, HOOK);
}

/** Within `timeoutMs`, the latest live draw starts at the session start (within one
 * backfill step) and spans at least `spanMs`, and the zoom readout names that span. */
async function expectLiveBackfilled(page: Page, spanMs: number, timeoutMs: number): Promise<void> {
  await expect
    .poll(
      async () => {
        const { draw, zoom } = await liveDrawAndZoom(page);
        if (!draw) return "no live draw yet";
        if (draw.t0Ms > 250) return `starts at ${draw.t0Ms} ms, not the session start`;
        if (draw.t1Ms - draw.t0Ms < spanMs)
          return `spans ${draw.t1Ms - draw.t0Ms} ms, less than ${spanMs} ms`;
        if (zoom !== formatSpan(draw.t1Ms - draw.t0Ms))
          return `zoom readout ${zoom} for a ${draw.t1Ms - draw.t0Ms} ms draw`;
        return "ok";
      },
      { timeout: timeoutMs, intervals: [50] },
    )
    .toBe("ok");
}

test("straight after a reload the live windows show the samples from before it (AC-VEDDFD.6)", async ({
  page,
}) => {
  await open(page);
  await streamDemo(page, DEMO_PD);
  await expect.poll(() => statsElapsedSeconds(page), { timeout: 10_000 }).toBeGreaterThanOrEqual(6);
  const spanBefore = (await statsElapsedSeconds(page)) * 1000;

  await reloadApp(page);
  await expect(badge(page)).toHaveText("Streaming");

  // The 10 s window (the default) fills from the history at once, not over 10 s.
  await expectLiveBackfilled(page, spanBefore, 1_500);
  expect((await liveDrawAndZoom(page)).zoom).not.toBe("");

  // So do the 1 min and 10 min windows.
  for (const name of ["1 min", "10 min"]) {
    await page.getByRole("button", { name, exact: true }).click();
    await expectLiveBackfilled(page, spanBefore, 1_500);
  }
  await page.getByRole("button", { name: "10 s", exact: true }).click();
  await expectLiveBackfilled(page, spanBefore, 1_500);
});
