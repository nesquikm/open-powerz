/**
 * AC-4NSVC9.1 / AC-4NSVC9.2: in the Playwright WebKit browser, after 60 s of
 * warm-up at a 1000 SPS live stream of the demo device, the page's web content
 * process grows by less than 120 MB over the next 2 minutes. Before the fix
 * the chart's per-frame Path2D objects leaked native memory in WebKit at
 * ~35 MB/s at 1000 SPS. The bound is not zero: WebKit collects garbage only
 * every ~37 s and the mock keeps the whole session history in the page, so
 * even a healthy page creeps up by ~0.5 MB/s here.
 *
 * WebKit only, and macOS / Linux only (the page process RSS is read from Node
 * with `ps`). It streams for 3 minutes, so it runs only when asked for: in CI
 * as a step of its own after the UI suite, locally by hand:
 *
 *   MEMORY_TEST=1 bun run e2e:ui memory.spec.ts --project=webkit --workers=1
 *
 * Run it alone, with one worker: the RSS is read per process, and another
 * page streaming at the same time would only add noise. The test opens its
 * own browser context and tracks the WebKit content processes that appear
 * when its page loads (on macOS `com.apple.WebKit.WebContent`, on Linux
 * `WebKitWebProcess`), taking the largest of them at each sample.
 */

import { expect, test } from "@playwright/test";
import { blur, open, streamDemo } from "./helpers";
import { listProcesses, noPageProcessReason, pageProcessMarker } from "./processes";

const WARM_UP_MS = 60_000;
const MEASURE_MS = 120_000;
const SAMPLE_EVERY_MS = 10_000;
const MAX_GROWTH_MB = 120;

/** Command-line marker of a WebKit page process on this OS. */
const PAGE_PROCESS = pageProcessMarker(process.platform);

/** RSS (KiB) of every running WebKit page process, by pid (AC-KQF5GM.4: full width). */
function pageProcesses(): Map<number, number> {
  return listProcesses(PAGE_PROCESS);
}

/** Largest RSS, in MB, among `pids` (all page processes when `pids` is empty). */
function rssMb(pids: ReadonlySet<number>): number {
  let max = 0;
  for (const [pid, rss] of pageProcesses()) {
    if (pids.size === 0 || pids.has(pid)) max = Math.max(max, rss);
  }
  return max / 1024;
}

test("a live session keeps the page's memory flat", async ({ browser, browserName }) => {
  test.skip(browserName !== "webkit", "the leak is WebKit's; measured on WebKit only");
  test.skip(
    process.env.MEMORY_TEST !== "1",
    "streams for 3 minutes: set MEMORY_TEST=1 (and --workers=1) to run it",
  );
  test.skip(process.platform === "win32", "reads the page process RSS with ps (macOS / Linux)");
  test.setTimeout(300_000);

  const before = new Set(pageProcesses().keys());
  const context = await browser.newContext({ viewport: { width: 1600, height: 900 } });
  try {
    const page = await context.newPage();
    await open(page);
    await streamDemo(page);
    await blur(page);
    await page.keyboard.press("4");
    await expect(page.getByLabel("Sample rate")).toHaveValue("1000");

    // This page's content process: the ones that appeared since the context opened.
    const ours = new Set([...pageProcesses().keys()].filter((pid) => !before.has(pid)));
    // AC-KQF5GM.4: no page process to measure is a skip with its reason, not a failure.
    const missing = noPageProcessReason(ours, PAGE_PROCESS);
    test.skip(missing !== undefined, missing);
    expect(rssMb(ours), `a ${PAGE_PROCESS} process is running`).toBeGreaterThan(0);

    await page.waitForTimeout(WARM_UP_MS);
    const samples: number[] = [rssMb(ours)];
    for (let t = SAMPLE_EVERY_MS; t <= MEASURE_MS; t += SAMPLE_EVERY_MS) {
      await page.waitForTimeout(SAMPLE_EVERY_MS);
      samples.push(rssMb(ours));
      // A leak this size fails now rather than growing the page until the OS kills it.
      if (samples[samples.length - 1]! - samples[0]! >= MAX_GROWTH_MB) break;
    }
    await expect(page.getByLabel("Sample rate")).toHaveValue("1000");

    const growth = samples[samples.length - 1]! - samples[0]!;
    const log = samples.map((mb, i) => `${(i * SAMPLE_EVERY_MS) / 1000}s ${mb.toFixed(0)} MB`);
    console.log(`page RSS after warm-up (${PAGE_PROCESS}): ${log.join(", ")}`);
    expect(
      growth,
      `page grew ${growth.toFixed(0)} MB in ${((samples.length - 1) * SAMPLE_EVERY_MS) / 1000} s at 1000 SPS: ${log.join(", ")}`,
    ).toBeLessThan(MAX_GROWTH_MB);
  } finally {
    await context.close();
  }
});
