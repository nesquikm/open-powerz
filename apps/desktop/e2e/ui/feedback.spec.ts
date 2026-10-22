/**
 * FR 9CFMD3: export and recording feedback, deep zoom and empty states (QA
 * findings 4, 5 and 9 on build 9787adb).
 *
 * - AC-9CFMD3.1: a finished export shows a notice with the rows written and the
 *   file path, a "Reveal in Finder" (macOS) / "Show in folder" action and "Copy path".
 * - AC-9CFMD3.2: a saved recording's notice gives the rows, the gaps and the path,
 *   with the same two actions.
 * - AC-9CFMD3.3: the reveal goes through the backend seam; the mock build records
 *   each request on `window.__OPEN_POWERZ_MOCK__.revealedPaths()`.
 * - AC-9CFMD3.4: at 50 SPS the zoom slider's bottom end still draws the trace.
 * - AC-9CFMD3.5: with no meter, the chart says "Connect a meter to start", Live is
 *   not pressed and the zoom readout is blank.
 *
 * "Copy path" is observed by replacing `navigator.clipboard.writeText` before the
 * app loads (WebKit grants no clipboard-read permission to the test).
 */

import { expect, test, type Page } from "@playwright/test";
import { chartSeriesLengths, lastDraw, open, streamDemo } from "./helpers";

test.use({ viewport: { width: 1600, height: 900 } });

/** The mock build's record of reveal requests. */
const MOCK_HOOK = "__OPEN_POWERZ_MOCK__";
/** The texts the stubbed clipboard received. */
const COPIED = "__OPEN_POWERZ_COPIED__";

const REVEAL = /^(Reveal in Finder|Show in folder)$/;
/** The mock has no save dialog: the proposed file name is the path. */
const MOCK_PATH = /powerz-\d{8}-\d{6}\.csv/;

async function stubClipboard(page: Page): Promise<void> {
  await page.addInitScript((key) => {
    const copied: string[] = [];
    (window as unknown as Record<string, unknown>)[key] = copied;
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: {
        writeText: async (text: string) => {
          copied.push(text);
        },
        readText: async () => copied.at(-1) ?? "",
      },
    });
  }, COPIED);
}

function copied(page: Page): Promise<string[]> {
  return page.evaluate(
    (key) => [...(((window as unknown as Record<string, unknown>)[key] as string[]) ?? [])],
    COPIED,
  );
}

async function revealedPaths(page: Page): Promise<string[]> {
  return page.evaluate((key) => {
    const hook = (window as unknown as Record<string, { revealedPaths?: () => string[] }>)[key];
    if (typeof hook?.revealedPaths !== "function") {
      throw new Error(`window.${key}.revealedPaths() is not installed in the mock build`);
    }
    return hook.revealedPaths();
  }, MOCK_HOOK);
}

function fileNotice(page: Page, text: RegExp) {
  return page.locator(".notice").filter({ hasText: text });
}

/** The file path the notice names. */
async function noticePath(page: Page, notice: ReturnType<typeof fileNotice>): Promise<string> {
  const m = MOCK_PATH.exec(await notice.innerText());
  if (!m) throw new Error("the notice names no file path");
  return m[0];
}

async function streamWithData(page: Page): Promise<void> {
  await stubClipboard(page);
  await open(page);
  await streamDemo(page);
  await expect.poll(async () => (await chartSeriesLengths(page)).vbus).toBeGreaterThan(20);
}

test("AC-9CFMD3.1: a finished export names its rows and file, with reveal and copy", async ({
  page,
}) => {
  await streamWithData(page);
  await page.getByRole("button", { name: "Export", exact: true }).click();

  const notice = fileNotice(page, /Export/i);
  await expect(notice).toBeVisible();
  await expect(notice).toHaveText(/\d[\d,]* rows?/);
  await expect(notice).toHaveText(MOCK_PATH);
  const path = await noticePath(page, notice);

  const reveal = notice.getByRole("button", { name: REVEAL });
  await expect(reveal).toBeVisible();
  const copy = notice.getByRole("button", { name: "Copy path", exact: true });
  await expect(copy).toBeVisible();

  await reveal.click();
  await expect.poll(() => revealedPaths(page)).toEqual([path]);
  await copy.click();
  await expect.poll(() => copied(page)).toEqual([path]);
});

test("AC-9CFMD3.2: a saved recording names its rows, gaps and file, with reveal and copy", async ({
  page,
}) => {
  await streamWithData(page);
  await page.getByRole("button", { name: "Record", exact: true }).click();
  await expect(page.getByRole("timer")).toBeVisible();
  await expect
    .poll(async () => (await page.getByRole("timer").innerText()).trim())
    .toMatch(/\d+ rows/);
  await page.getByRole("button", { name: "Stop", exact: true }).click();

  const notice = fileNotice(page, /Recording saved/);
  await expect(notice).toBeVisible();
  await expect(notice).toHaveText(/\d[\d,]* rows?/);
  await expect(notice).toHaveText(/no gaps|\d+ gaps?/);
  await expect(notice).toHaveText(MOCK_PATH);
  const path = await noticePath(page, notice);

  await notice.getByRole("button", { name: REVEAL }).click();
  await expect.poll(() => revealedPaths(page)).toEqual([path]);
  await notice.getByRole("button", { name: "Copy path", exact: true }).click();
  await expect.poll(() => copied(page)).toEqual([path]);
});

test("AC-9CFMD3.4: the narrowest zoom at 50 SPS still draws the VBUS line", async ({ page }) => {
  await streamWithData(page);
  await expect(page.getByLabel("Sample rate")).toHaveValue("50");
  // A few seconds of history to zoom into.
  await expect.poll(async () => (await chartSeriesLengths(page)).vbus).toBeGreaterThan(100);

  const slider = page.getByRole("slider", { name: "Zoom" });
  await slider.fill("0");
  await expect.poll(async () => (await lastDraw(page)).mode).toBe("history");
  await expect.poll(async () => (await lastDraw(page)).fromHistory).toBe(true);

  const draw = await lastDraw(page);
  const span = draw.t1Ms - draw.t0Ms;
  // Two sample periods at 50 SPS.
  expect(span).toBeGreaterThanOrEqual(40 - 0.5);
  expect(span).toBeLessThanOrEqual(45);
  await expect.poll(async () => (await chartSeriesLengths(page)).vbus).toBeGreaterThanOrEqual(2);
});

test("AC-9CFMD3.5: with no meter, the chart says how to start", async ({ page }) => {
  await open(page);
  const chart = page.getByRole("region", { name: "Chart" });
  await expect(chart.getByText("Connect a meter to start")).toBeVisible();
  const live = page.locator(".chart-wrap button.live");
  await expect(live).not.toHaveAttribute("aria-pressed", "true");
  await expect(page.locator(".zoom-span")).toHaveText("");

  await streamDemo(page);
  await expect(chart.getByText("Connect a meter to start")).toHaveCount(0);
});

test("AC-9CFMD3.5: after a disconnect, the empty state comes back", async ({ page }) => {
  await open(page);
  await streamDemo(page);
  await expect.poll(async () => (await chartSeriesLengths(page)).vbus).toBeGreaterThan(10);
  await page.getByRole("button", { name: "Disconnect", exact: true }).click();

  const chart = page.getByRole("region", { name: "Chart" });
  await expect(chart.getByText("Connect a meter to start")).toBeVisible();
  await expect(page.locator(".chart-wrap button.live")).not.toHaveAttribute("aria-pressed", "true");
  await expect(page.locator(".zoom-span")).toHaveText("");
});

test("AC-9CFMD3.5: after a disconnect the last session stays on screen under a banner", async ({
  page,
}) => {
  await open(page);
  await streamDemo(page);
  await expect.poll(async () => (await chartSeriesLengths(page)).vbus).toBeGreaterThan(10);
  await page.getByRole("button", { name: "Disconnect", exact: true }).click();
  const chart = page.getByRole("region", { name: "Chart" });
  const message = chart.getByText("Connect a meter to start");
  await expect(message).toBeVisible();
  await expect(message).toContainText("showing the last session");
  // A banner over the top of the plot, not a cover over the trace.
  const plot = (await page.locator(".plot .u-over").boundingBox())!;
  const box = (await message.boundingBox())!;
  expect(box.height).toBeLessThan(plot.height * 0.25);
  expect((await chartSeriesLengths(page)).vbus).toBeGreaterThan(10);
});
