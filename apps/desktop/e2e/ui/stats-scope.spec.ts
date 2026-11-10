/**
 * FR PBDEQE on the mocked backend: what the File tab's stats cover.
 *
 * AC-PBDEQE.2: a scope label, "Whole file" or "Shown range, <start> to <end>".
 * AC-PBDEQE.3: a Whole file / Shown range toggle (the group "Stats scope"); Shown range
 * is offered only while the chart is zoomed, follows each new zoom, and Whole file
 * brings the whole-file stats back.
 * AC-PBDEQE.4: regression, a zoom that leaves out a VBUS dip has the zoom's minimum.
 * AC-PBDEQE.5: the live Stats panel says "Since connect or Reset".
 *
 * Mock contract pinned here: `?recordingfile=dip` opens the usual demo file
 * (`MOCK_RECORDING_FILE`, the `load-5v-1a` reading at each ms) with VBUS held at 3 V for
 * its first 1000 ms, so the whole file's VBUS minimum is 3.000 V and any later part's is
 * the load's own (4.990 V over more than a second).
 */

import { expect, test, type Locator, type Page } from "@playwright/test";
import { open, streamDemo } from "./helpers";

test.use({ viewport: { width: 1600, height: 900 } });

function fileView(page: Page): Locator {
  return page.getByRole("region", { name: "Recording file" });
}

function scopeGroup(view: Locator): Locator {
  return view.getByRole("group", { name: "Stats scope" });
}

function scopeButton(view: Locator, name: "Whole file" | "Shown range"): Locator {
  return scopeGroup(view).getByRole("button", { name, exact: true });
}

function label(view: Locator): Locator {
  return view.locator(".stats-scope");
}

/** The File tab's VBUS row cell under `column` (Min, Max or Avg). */
function vbusCell(view: Locator, column: "Min" | "Max" | "Avg"): Locator {
  const index = { Min: 0, Max: 1, Avg: 2 }[column];
  return view
    .locator("tbody tr", { has: view.page().getByRole("rowheader", { name: "VBUS" }) })
    .locator("td")
    .nth(index);
}

function elapsed(view: Locator): Locator {
  return view
    .locator(".totals dt", { hasText: "Elapsed" })
    .locator("xpath=following-sibling::dd[1]");
}

async function openDipFile(page: Page): Promise<Locator> {
  await open(page, "?recordingfile=dip");
  await page.getByRole("button", { name: "Open recording", exact: true }).click();
  const view = fileView(page);
  await expect(view).toBeVisible();
  return view;
}

/** Drags across the file's plot from `from` to `to` (shares of its width). */
async function zoom(page: Page, view: Locator, from: number, to: number): Promise<void> {
  const over = view.locator(".chart .u-over").first();
  const box = await over.boundingBox();
  if (box === null) throw new Error("no plot to drag on");
  const y = box.y + box.height / 2;
  await page.mouse.move(box.x + box.width * from, y);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width * to, y, { steps: 5 });
  await page.mouse.up();
}

/** Shown range is not offered: gone, or there and disabled. */
async function expectShownRangeNotOffered(view: Locator): Promise<void> {
  const shown = scopeButton(view, "Shown range");
  await expect
    .poll(async () => (await shown.count()) === 0 || (await shown.isDisabled()))
    .toBe(true);
}

test("the File tab stats say Whole file, and Shown range waits for a zoom (AC-PBDEQE.2, .3)", async ({
  page,
}) => {
  const view = await openDipFile(page);
  await expect(label(view)).toHaveText("Whole file");
  await expect(scopeButton(view, "Whole file")).toHaveAttribute("aria-pressed", "true");
  await expectShownRangeNotOffered(view);
  await expect(vbusCell(view, "Min")).toHaveText("3.000 V");
  await expect(elapsed(view)).toHaveText("0:00:12");
});

test("Shown range follows each zoom and Whole file brings the file back (AC-PBDEQE.2, .3, .4)", async ({
  page,
}) => {
  const view = await openDipFile(page);
  const range = view.locator(".range");
  const wholeMin = "3.000 V";
  await expect(vbusCell(view, "Min")).toHaveText(wholeMin);

  // A zoom that leaves out the dip; the stats stay on the whole file until asked.
  await zoom(page, view, 0.4, 0.6);
  await expect(range).toHaveText(/ to /);
  await expect(label(view)).toHaveText("Whole file");
  await expect(vbusCell(view, "Min")).toHaveText(wholeMin);

  await expect(scopeButton(view, "Shown range")).toBeEnabled();
  await scopeButton(view, "Shown range").click();
  await expect(scopeButton(view, "Shown range")).toHaveAttribute("aria-pressed", "true");
  await expect(scopeButton(view, "Whole file")).toHaveAttribute("aria-pressed", "false");

  // The label names the range on screen, in the range text's own format.
  const first = (await range.textContent()) ?? "";
  await expect(label(view)).toHaveText(`Shown range, ${first}`);
  // AC-PBDEQE.4: the lowest VBUS inside the zoom, never the dip outside it.
  await expect(vbusCell(view, "Min")).toHaveText("4.990 V");
  await expect(vbusCell(view, "Max")).toHaveText("5.010 V");
  // Elapsed is the zoom's span (about 2.5 s of the 12.3 s file).
  await expect(elapsed(view)).toHaveText("0:00:02");

  // A new zoom inside the first: the stats follow it.
  await zoom(page, view, 0.1, 0.3);
  await expect(range).not.toHaveText(first);
  const second = (await range.textContent()) ?? "";
  await expect(label(view)).toHaveText(`Shown range, ${second}`);
  await expect(elapsed(view)).toHaveText("0:00:00");

  // Back to the whole file's stats, the zoom kept.
  await scopeButton(view, "Whole file").click();
  await expect(label(view)).toHaveText("Whole file");
  await expect(vbusCell(view, "Min")).toHaveText(wholeMin);
  await expect(elapsed(view)).toHaveText("0:00:12");
  await expect(range).toHaveText(second);
});

test("leaving the zoom puts the stats back on the whole file (AC-PBDEQE.3)", async ({ page }) => {
  const view = await openDipFile(page);
  await zoom(page, view, 0.4, 0.6);
  await scopeButton(view, "Shown range").click();
  await expect(vbusCell(view, "Min")).toHaveText("4.990 V");

  await view.locator(".zoom").getByRole("button", { name: "Whole file", exact: true }).click();

  await expect(label(view)).toHaveText("Whole file");
  await expect(vbusCell(view, "Min")).toHaveText("3.000 V");
  await expect(elapsed(view)).toHaveText("0:00:12");
  await expectShownRangeNotOffered(view);
});

test("the live Stats panel says it counts since connect or Reset (AC-PBDEQE.5)", async ({
  page,
}) => {
  await open(page);
  await streamDemo(page);
  const panel = page.getByRole("complementary", { name: "Statistics" });
  await expect(panel).toContainText("Since connect or Reset");
  // Its values are the live ones still: the 5 V demo load.
  await expect(
    panel
      .locator("tbody tr", { has: page.getByRole("rowheader", { name: "VBUS" }) })
      .locator("td")
      .nth(2),
  ).toHaveText(/^5\.0\d\d\s*V$/);
});
