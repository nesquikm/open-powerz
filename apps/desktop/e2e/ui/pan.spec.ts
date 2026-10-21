/**
 * AC-AH2AEM.4 (deferred to this suite by FR AH2AEN): with a 10-million-sample
 * history served by the mock backend (`?synthetic=10000000`, which the mock
 * production build must honour), each pan step renders in at most 100 ms:
 * from the arrow key press to the chart's draw of the new range from its
 * history response, read through the e2e test hook.
 *
 * Chromium only: the seed is ~320 MB of columns per page, and the bound is a
 * property of the app, not of the browser engine.
 */

import { expect, test } from "@playwright/test";
import { HOOK, type ChartDraw, type OpenPowerzE2E } from "../hook";
import { chartDraws, chartSeriesLengths, expectHook, open, streamDemo } from "./helpers";

const SYNTHETIC = 10_000_000;
const PAN_STEPS = 6;
const BOUND_MS = 100;

test.use({ viewport: { width: 1600, height: 900 } });

test("a pan step over a 10M-sample history renders in ≤ 100 ms (AC-AH2AEM.4)", async ({
  page,
  browserName,
}) => {
  test.skip(browserName !== "chromium", "timed on Chromium only");
  test.setTimeout(120_000);
  await open(page, `?synthetic=${SYNTHETIC}`);
  await streamDemo(page);
  await expectHook(page);
  await expect.poll(async () => (await chartSeriesLengths(page)).vbus).toBeGreaterThan(0);

  const plot = page.getByRole("application");
  await plot.focus();
  const timings: number[] = [];
  let previous: ChartDraw | null = null;
  for (let step = 0; step < PAN_STEPS; step++) {
    const before = (await chartDraws(page)).length;
    // Press inside the page so the start time and the draw time share a clock.
    const pressedAt = await page.evaluate(() => {
      const t = performance.now();
      document.activeElement?.dispatchEvent(
        new KeyboardEvent("keydown", { key: "ArrowLeft", bubbles: true, cancelable: true }),
      );
      return t;
    });
    const done = await page.waitForFunction(
      ([name, from, prev]) => {
        const h = (window as unknown as Record<string, OpenPowerzE2E>)[name]!;
        return (
          h
            .chartDraws()
            .slice(from)
            .find(
              (d) =>
                d.mode === "history" &&
                d.fromHistory &&
                (prev === null || d.t0Ms < prev.t0Ms || d.t1Ms < prev.t1Ms),
            ) ?? false
        );
      },
      [HOOK, before, previous] as const,
      { timeout: 10_000, polling: "raf" },
    );
    const draw = (await done.jsonValue()) as ChartDraw;
    timings.push(draw.at - pressedAt);
    previous = draw;
  }
  expect(previous?.t0Ms ?? Infinity, "the pans moved back into the seeded history").toBeLessThan(
    SYNTHETIC,
  );
  expect(
    Math.max(...timings),
    `pan step render times (ms): ${timings.map((t) => t.toFixed(1)).join(", ")}`,
  ).toBeLessThanOrEqual(BOUND_MS);
});
