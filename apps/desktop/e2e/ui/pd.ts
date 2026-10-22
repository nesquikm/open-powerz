/**
 * Shared steps of the PD analyzer specs (FR TGE83C), on the mocked backend.
 *
 * The page contract these specs pin:
 *
 * - A tab "PD analyzer" opens a tabpanel of the same name. It holds the buttons
 *   "Start capture", "Stop capture", "Clear capture" and "Follow" (`aria-pressed`
 *   true while the table follows new rows), a "Show GoodCRC" checkbox and the
 *   "Message type" and "SOP" filter selects (first option "All").
 * - The packet table is a `grid` named "PD packets" that scrolls itself (a virtual
 *   list): `aria-rowcount` is the number of rows the filters let through plus the
 *   header row; each rendered data row has `data-index` (the record index),
 *   `data-kind` ("message" or "event"), `aria-rowindex`, `aria-selected` and an
 *   `aria-label` for screen readers, and one `gridcell` per column.
 * - Selecting a row fills the region "Packet details": a `tree` named "Fields"
 *   (`treeitem`s named by field name first) and the raw bytes, one element per
 *   byte with `data-byte` = its offset; the bytes of the selected field are `mark`
 *   elements.
 * - `?pdsynthetic=<n>` seeds n records when the capture starts (mock builds only).
 */

import { expect, type Locator, type Page } from "@playwright/test";
import { open, streamDemo } from "./helpers";

export const COLUMNS = [
  "#",
  "Time",
  "SOP",
  "Message",
  "ID",
  "Direction",
  "Objects",
  "Revision",
  "VBUS",
  "IBUS",
  "Summary",
] as const;

/** Column position of `name` in a row's cells. */
export function col(name: (typeof COLUMNS)[number]): number {
  return COLUMNS.indexOf(name);
}

export function pdView(page: Page): Locator {
  return page.getByRole("tabpanel", { name: "PD analyzer" });
}

export function grid(page: Page): Locator {
  return pdView(page).getByRole("grid", { name: "PD packets" });
}

/** The rendered data rows (the virtual list renders only those in view). */
export function dataRows(page: Page): Locator {
  return grid(page).locator('[role="row"][data-index]');
}

export function details(page: Page): Locator {
  return pdView(page).getByRole("region", { name: "Packet details" });
}

export function startButton(page: Page): Locator {
  return pdView(page).getByRole("button", { name: "Start capture" });
}

export function stopButton(page: Page): Locator {
  return pdView(page).getByRole("button", { name: "Stop capture" });
}

/** Switch to the PD analyzer tab. */
export async function showPd(page: Page): Promise<void> {
  await page.getByRole("tab", { name: "PD analyzer" }).click();
  await expect(pdView(page)).toBeVisible();
}

/** Open the app (with `query`), connect the demo device and show the PD view. */
export async function openPd(page: Page, query = ""): Promise<void> {
  await open(page, query);
  await streamDemo(page);
  await showPd(page);
}

/** Rows the filters let through (`aria-rowcount` minus the header row). */
export async function rowCount(page: Page): Promise<number> {
  const n = Number(await grid(page).getAttribute("aria-rowcount"));
  if (!Number.isFinite(n)) throw new Error("the grid has no aria-rowcount");
  return n - 1;
}

/** Start the capture and wait until a Source_Capabilities row is in. */
export async function capture(page: Page): Promise<void> {
  await startButton(page).click();
  await expect(stopButton(page)).toBeEnabled();
  await expect
    .poll(async () => (await messageTexts(page)).includes("Source_Capabilities"), {
      timeout: 10_000,
    })
    .toBe(true);
}

/** Stop the capture and wait for the button states to settle. */
export async function stopCapture(page: Page): Promise<void> {
  await stopButton(page).click();
  await expect(startButton(page)).toBeEnabled();
}

/** The Message cell of every rendered row. */
export async function messageTexts(page: Page): Promise<string[]> {
  return dataRows(page).evaluateAll(
    (rows, i) =>
      rows.map(
        (r) =>
          (
            r.querySelectorAll('[role="gridcell"]')[i] as HTMLElement | undefined
          )?.innerText.trim() ?? "",
      ),
    col("Message"),
  );
}

/** The cells of the first rendered row whose Message cell is `name`. */
export function rowNamed(page: Page, name: string): Locator {
  return dataRows(page)
    .filter({ has: page.getByRole("gridcell", { name, exact: true }) })
    .first();
}

/** Scroll the table to the top (so the first records are rendered). */
export async function scrollToTop(page: Page): Promise<void> {
  await grid(page).evaluate((g) => {
    g.scrollTop = 0;
    g.dispatchEvent(new Event("scroll"));
  });
}

/** Whether the table's scroll position is at its bottom. */
export async function atBottom(page: Page): Promise<boolean> {
  return grid(page).evaluate((g) => g.scrollTop + g.clientHeight >= g.scrollHeight - 2);
}

/** The effective background colour behind `el`, as relative luminance 0..1. */
export async function backgroundLuminance(target: Locator): Promise<number> {
  return target.evaluate((el) => {
    let node: Element | null = el;
    let rgb: number[] | null = null;
    while (node) {
      const m = getComputedStyle(node).backgroundColor.match(/[\d.]+/g);
      if (m && (m.length < 4 || Number(m[3]) > 0.5)) {
        rgb = m.slice(0, 3).map(Number);
        break;
      }
      node = node.parentElement;
    }
    rgb ??= [255, 255, 255];
    const lin = rgb.map((c) => {
      const s = c / 255;
      return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
    });
    return 0.2126 * lin[0]! + 0.7152 * lin[1]! + 0.0722 * lin[2]!;
  });
}
