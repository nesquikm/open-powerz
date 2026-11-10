/**
 * FR AXHK2S on the mocked backend: "Open recording" with no meter, the read-only view and
 * its mark, a running stream and recording going on behind it, Cancel, a refusal, Close,
 * and Export .db. `?recordingfile=slow` holds the open until Cancel, `refused` refuses
 * the file, `truncated` opens one past the row limit, `dialog` stays in the open dialog,
 * `unknown-total` holds an open that knows no total.
 */

import { expect, test, type Locator, type Page } from "@playwright/test";
import { MOCK_RECORDING_FILE } from "../../src/lib/mockBackend";
import { badge, open, statsElapsedSeconds, streamDemo } from "./helpers";

test.use({ viewport: { width: 1600, height: 900 } });

function button(scope: Page | Locator, name: string): Locator {
  return scope.getByRole("button", { name, exact: true });
}

function fileTab(page: Page): Locator {
  return page.getByRole("tab", { name: "File", exact: true });
}

function fileView(page: Page): Locator {
  return page.getByRole("region", { name: "Recording file" });
}

async function openRecording(page: Page): Promise<void> {
  await button(page, "Open recording").click();
  await expect(fileView(page)).toBeVisible();
}

test("a file opens with no meter connected, marked read-only (AC-AXHK2S.1, .2, .7)", async ({
  page,
}) => {
  await open(page);
  await expect(fileTab(page)).toHaveCount(0);

  await openRecording(page);

  await expect(fileTab(page)).toHaveAttribute("aria-selected", "true");
  const view = fileView(page);
  await expect(view).toContainText(MOCK_RECORDING_FILE.name);
  await expect(view).toContainText("read-only");
  await expect(view).toContainText(MOCK_RECORDING_FILE.rows.toLocaleString("en-US"));
  await expect(view).toContainText(new RegExp(`${MOCK_RECORDING_FILE.skipped}\\s+damaged rows?`));
  await expect(view.locator("canvas").first()).toBeVisible();
  await expect(view.getByText("Charge", { exact: false }).first()).toBeVisible();
});

test("a running stream and recording go on behind the file (AC-AXHK2S.7)", async ({ page }) => {
  await open(page);
  await streamDemo(page);
  await button(page, "Record").click();
  await expect(page.getByRole("timer")).toContainText(/[1-9][\d,]* rows?/);

  await openRecording(page);
  const rows = async () =>
    Number(
      (/([\d,]+) rows?/.exec(await page.getByRole("timer").innerText())?.[1] ?? "0").replace(
        /,/g,
        "",
      ),
    );
  const before = await rows();

  await expect(badge(page)).toHaveText("Streaming");
  await expect.poll(rows).toBeGreaterThan(before);

  await page.getByRole("tab", { name: "Meter", exact: true }).click();
  const elapsed = await statsElapsedSeconds(page);
  await expect.poll(() => statsElapsedSeconds(page)).toBeGreaterThan(elapsed);
  // The file stays open while another tab shows.
  await expect(fileTab(page)).toHaveCount(1);
});

test("Cancel ends a slow open and leaves no tab (AC-AXHK2S.4)", async ({ page }) => {
  await open(page, "?recordingfile=slow");
  await button(page, "Open recording").click();

  const cancel = button(page, "Cancel");
  await expect(cancel).toBeVisible();
  await expect(page.getByRole("progressbar")).toBeVisible();
  await cancel.click();

  await expect(cancel).toBeHidden();
  await expect(fileTab(page)).toHaveCount(0);
  await expect(page.getByRole("alert")).toHaveCount(0);
});

test("a refused file is said in plain words and opens no tab (AC-AXHK2S.3)", async ({ page }) => {
  await open(page, "?recordingfile=refused");
  await button(page, "Open recording").click();

  await expect(page.getByRole("alert")).toContainText(/header/i);
  await expect(fileTab(page)).toHaveCount(0);
});

test("a file past the limit says how many rows were left out (AC-AXHK2S.4)", async ({ page }) => {
  await open(page, "?recordingfile=truncated");
  await openRecording(page);
  await expect(fileView(page)).toContainText(/left out/i);
});

test("Close file removes the tab and returns to the meter view (AC-AXHK2S.7)", async ({ page }) => {
  await open(page);
  await openRecording(page);

  await button(fileView(page), "Close file").click();

  await expect(fileTab(page)).toHaveCount(0);
  await expect(fileView(page)).toHaveCount(0);
  await expect(page.getByRole("tab", { name: "Meter", exact: true })).toHaveAttribute(
    "aria-selected",
    "true",
  );
});

test("Export .db writes the session and reports its rows (AC-AXHK2S.6)", async ({ page }) => {
  await open(page);
  await streamDemo(page);
  await button(page, "Export .db").click();
  // The button says .db too: only the notice has the row count and the file name.
  await expect(
    page.getByText(/^Exported [\d,]+ rows? to powerz-\d{8}-\d{6}\.db$/).first(),
  ).toBeVisible();
});

test("no Cancel or progress while the open dialog is up (AC-AXHK2S.4)", async ({ page }) => {
  await open(page, "?recordingfile=dialog");
  await button(page, "Open recording").click();
  // Long enough for the page to have drawn what opening shows.
  await page.evaluate(
    () => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))),
  );
  await expect(button(page, "Cancel")).toHaveCount(0);
  await expect(page.getByRole("progressbar")).toHaveCount(0);
});

test("the opening strip is a status, with a plain bar for an unknown total (AC-AXHK2S.4)", async ({
  page,
}) => {
  await open(page, "?recordingfile=unknown-total");
  await button(page, "Open recording").click();

  const strip = page.getByRole("status").filter({ hasText: "Opening recording" });
  await expect(strip).toBeVisible();
  const bar = strip.getByRole("progressbar");
  await expect(bar).toBeVisible();
  // An indeterminate bar has no position.
  expect(await bar.evaluate((el) => (el as HTMLProgressElement).position)).toBe(-1);

  await button(strip, "Cancel").click();
  await expect(strip).toBeHidden();
});

test("a known total fills the bar (AC-AXHK2S.4)", async ({ page }) => {
  await open(page, "?recordingfile=slow");
  await button(page, "Open recording").click();
  const bar = page.getByRole("progressbar");
  await expect(bar).toBeVisible();
  expect(await bar.evaluate((el) => (el as HTMLProgressElement).position)).toBeCloseTo(0.5, 1);
  await button(page, "Cancel").click();
});

test("Close file keeps to one line on a phone", async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 640 });
  await open(page);
  await openRecording(page);
  const close = button(fileView(page), "Close file");
  await expect(close).toHaveCSS("white-space", "nowrap");
  // One box per line of the label.
  const lines = await close.evaluate((el) => {
    const range = document.createRange();
    range.selectNodeContents(el);
    return new Set([...range.getClientRects()].map((r) => Math.round(r.top))).size;
  });
  expect(lines).toBe(1);
  const box = await close.boundingBox();
  if (box === null) throw new Error("no button");
  expect(box.x + box.width).toBeLessThanOrEqual(320);
});

test("a drag zooms the file's chart and Whole file comes back (AC-AXHK2S.4)", async ({ page }) => {
  await open(page);
  await openRecording(page);
  const view = fileView(page);
  const range = view.locator(".range");
  const whole = await range.textContent();
  await expect(button(view.locator(".zoom"), "Whole file")).toBeDisabled();

  const over = view.locator(".u-over");
  const box = await over.boundingBox();
  if (box === null) throw new Error("no plot to drag on");
  const y = box.y + box.height / 2;
  await page.mouse.move(box.x + box.width * 0.4, y);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width * 0.6, y, { steps: 5 });
  await page.mouse.up();

  await expect(range).not.toHaveText(whole ?? "");
  await expect(button(view.locator(".zoom"), "Whole file")).toBeEnabled();

  await button(view.locator(".zoom"), "Whole file").click();
  await expect(range).toHaveText(whole ?? "");
});

test("opening a file leaves the Trigger tab on screen, as leaving it sends (AC-AXHK2S.7)", async ({
  page,
}) => {
  await open(page);
  await streamDemo(page);
  const trigger = page.getByRole("tab", { name: "Trigger", exact: true });
  await trigger.click();
  await expect(trigger).toHaveAttribute("aria-selected", "true");

  await button(page, "Open recording").click();

  await expect(fileTab(page)).toBeVisible();
  await expect(trigger).toHaveAttribute("aria-selected", "true");
});

// FR 09H204: the overview strip under the File tab's chart.

function fileStrip(view: Locator): Locator {
  return view.locator(".overview");
}

function fileStripWindow(view: Locator): Locator {
  return view.getByRole("slider", { name: "Visible range" });
}

/** Drags across the file's plot from `from` to `to` (shares of its width). */
async function zoomFile(page: Page, view: Locator, from: number, to: number): Promise<void> {
  const over = view.locator(".u-over").first();
  const box = await over.boundingBox();
  if (box === null) throw new Error("no plot to drag on");
  const y = box.y + box.height / 2;
  await page.mouse.move(box.x + box.width * from, y);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width * to, y, { steps: 5 });
  await page.mouse.up();
}

/** The strip window's width as a share of the strip's. */
async function windowShare(view: Locator): Promise<number> {
  const strip = await fileStrip(view).boundingBox();
  const win = await fileStripWindow(view).boundingBox();
  if (strip === null || win === null || strip.width === 0) return Number.NaN;
  return win.width / strip.width;
}

/** Painted (non-transparent) pixels on the strip's canvas. */
async function paintedPixels(view: Locator): Promise<number> {
  return fileStrip(view)
    .locator("canvas")
    .first()
    .evaluate((el) => {
      const canvas = el as HTMLCanvasElement;
      const ctx = canvas.getContext("2d");
      if (ctx === null || canvas.width === 0 || canvas.height === 0) return 0;
      const data = ctx.getImageData(0, 0, canvas.width, canvas.height).data;
      let painted = 0;
      for (let i = 3; i < data.length; i += 4) if (data[i]! > 0) painted++;
      return painted;
    });
}

test("the File tab shows the overview strip, painted from the file (AC-09H204.1)", async ({
  page,
}) => {
  await open(page);
  await openRecording(page);
  const view = fileView(page);
  await expect(fileStrip(view)).toBeVisible();
  await expect(fileStripWindow(view)).toBeVisible();
  // Under the chart.
  const plot = await view.locator(".u-over").first().boundingBox();
  const strip = await fileStrip(view).boundingBox();
  if (plot === null || strip === null) throw new Error("no plot or strip");
  expect(strip.y).toBeGreaterThanOrEqual(plot.y + plot.height - 1);
  await expect.poll(() => paintedPixels(view)).toBeGreaterThan(0);
});

test("the strip's window covers the whole file, then only the zoomed part (AC-09H204.2)", async ({
  page,
}) => {
  await open(page);
  await openRecording(page);
  const view = fileView(page);
  await expect(fileStripWindow(view)).toBeVisible();
  await expect.poll(() => windowShare(view)).toBeGreaterThan(0.95);

  const range = view.locator(".range");
  const whole = await range.textContent();
  await zoomFile(page, view, 0.4, 0.6);
  await expect(range).not.toHaveText(whole ?? "");
  await expect.poll(() => windowShare(view)).toBeLessThan(0.5);
  await expect.poll(() => windowShare(view)).toBeGreaterThan(0.05);

  await button(view.locator(".zoom"), "Whole file").click();
  await expect.poll(() => windowShare(view)).toBeGreaterThan(0.95);
});

test("the strip moves the File chart's range, and Shown range stats follow (AC-09H204.3)", async ({
  page,
}) => {
  await open(page);
  await openRecording(page);
  const view = fileView(page);
  const range = view.locator(".range");
  const scopeLine = view.locator(".stats-scope");
  await expect(fileStripWindow(view)).toBeVisible();
  await zoomFile(page, view, 0.3, 0.45);
  await expect(button(view.locator(".zoom"), "Whole file")).toBeEnabled();
  await view
    .getByRole("group", { name: "Stats scope" })
    .getByRole("button", { name: "Shown range", exact: true })
    .click();
  await expect(scopeLine).toContainText(/Shown range/);

  // Keyboard: → on the focused window moves it.
  let before = await range.textContent();
  let scopeBefore = await scopeLine.textContent();
  await fileStripWindow(view).focus();
  await page.keyboard.press("ArrowRight");
  await expect(range).not.toHaveText(before ?? "");
  await expect(scopeLine).not.toHaveText(scopeBefore ?? "");

  // Drag: the window moves right by a fifth of the strip.
  before = await range.textContent();
  scopeBefore = await scopeLine.textContent();
  const strip = await fileStrip(view).boundingBox();
  const win = await fileStripWindow(view).boundingBox();
  if (strip === null || win === null) throw new Error("no strip");
  const y = win.y + win.height / 2;
  await page.mouse.move(win.x + win.width / 2, y);
  await page.mouse.down();
  await page.mouse.move(win.x + win.width / 2 + strip.width * 0.2, y, { steps: 5 });
  await page.mouse.up();
  await expect(range).not.toHaveText(before ?? "");
  await expect(scopeLine).not.toHaveText(scopeBefore ?? "");

  // Click: off the window, near the strip's left end, centres the view there.
  before = await range.textContent();
  scopeBefore = await scopeLine.textContent();
  await page.mouse.click(strip.x + strip.width * 0.05, strip.y + strip.height / 2);
  await expect(range).not.toHaveText(before ?? "");
  await expect(scopeLine).toContainText(/Shown range/);
  await expect(scopeLine).not.toHaveText(scopeBefore ?? "");
  const after = await windowShare(view);
  expect(after).toBeLessThan(0.5);
  const moved = await fileStripWindow(view).boundingBox();
  if (moved === null) throw new Error("no window");
  expect(moved.x - strip.x).toBeLessThan(strip.width * 0.2);
});

test("closing or reopening the file clears the strip (AC-09H204.4)", async ({ page }) => {
  await open(page);
  await openRecording(page);
  let view = fileView(page);
  await expect(fileStripWindow(view)).toBeVisible();
  await zoomFile(page, view, 0.4, 0.6);
  await expect.poll(() => windowShare(view)).toBeLessThan(0.5);

  await button(view, "Close file").click();
  await expect(fileView(page)).toHaveCount(0);
  await expect(
    page.getByRole("slider", { name: "Visible range" }).filter({ visible: true }),
  ).toHaveCount(0);

  await openRecording(page);
  view = fileView(page);
  await expect(fileStripWindow(view)).toBeVisible();
  // A fresh file: the window spans it whole, nothing of the old zoom stays.
  await expect.poll(() => windowShare(view)).toBeGreaterThan(0.95);
  await expect.poll(() => paintedPixels(view)).toBeGreaterThan(0);
});
