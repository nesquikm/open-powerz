/**
 * FR TGE83C: the desktop PD analyzer view on the mocked backend (AC .1–.5, .7, .8):
 * capture, the packet table and its filters, the details pane, following new rows,
 * Clear with an in-app confirmation, keyboard use, screen reader labels, axe, and the
 * light, dark and system themes. The page contract is described in `pd.ts`.
 */

import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import { open, useTheme, type Theme } from "./helpers";
import {
  atBottom,
  backgroundLuminance,
  capture,
  col,
  COLUMNS,
  dataRows,
  details,
  grid,
  messageTexts,
  openPd,
  pdView,
  rowCount,
  rowNamed,
  scrollToTop,
  showPd,
  startButton,
  stopButton,
  stopCapture,
} from "./pd";

test.use({ viewport: { width: 1600, height: 900 } });

/** Fail the test on any native browser dialog (Clear must confirm in the app). */
function noNativeDialogs(page: Page): void {
  page.on("dialog", async (d) => {
    await d.dismiss();
    throw new Error(`a native ${d.type()} dialog opened: ${d.message()}`);
  });
}

// ---- AC-TGE83C.1: Start and Stop ----

test("Start and Stop are disabled with a reason while no meter is connected (AC-TGE83C.1)", async ({
  page,
}) => {
  await open(page);
  await showPd(page);
  await expect(startButton(page)).toBeDisabled();
  await expect(stopButton(page)).toBeDisabled();
  await expect(startButton(page)).toHaveAccessibleDescription(/connect/i);
  await expect(stopButton(page)).toHaveAccessibleDescription(/connect/i);
  // The reason is on screen too, not only for screen readers.
  await expect(
    pdView(page)
      .getByText(/connect a meter/i)
      .first(),
  ).toBeVisible();
});

test("the demo device captures the PD scenario with Start, and Stop ends it (AC-TGE83C.1)", async ({
  page,
}) => {
  await openPd(page);
  await expect(startButton(page)).toBeEnabled();
  await expect(stopButton(page)).toBeDisabled();
  await capture(page);
  await expect(startButton(page)).toBeDisabled();
  await stopCapture(page);
  await expect(stopButton(page)).toBeDisabled();
  const n = await rowCount(page);
  expect(n).toBeGreaterThan(0);
  // Nothing more arrives once stopped (the 1 s wait is the observation window).
  await page.waitForTimeout(1_000);
  expect(await rowCount(page)).toBe(n);
});

// ---- AC-TGE83C.2: the packet table ----

test("the table has every column, and a message row fills them in (AC-TGE83C.2)", async ({
  page,
}) => {
  await openPd(page);
  await capture(page);
  await stopCapture(page);
  await scrollToTop(page);
  const headers = await grid(page).getByRole("columnheader").allInnerTexts();
  expect(headers.map((h) => h.trim())).toEqual([...COLUMNS]);

  const caps = rowNamed(page, "Source_Capabilities");
  await expect(caps).toHaveAttribute("data-kind", "message");
  const cells = (await caps.getByRole("gridcell").allInnerTexts()).map((c) => c.trim());
  expect(cells).toHaveLength(COLUMNS.length);
  expect(cells[col("#")]).toMatch(/^\d+$/);
  expect(cells[col("Time")]).toMatch(/^\d+\.\d{3} s$/);
  expect(cells[col("SOP")]).toBe("SOP");
  expect(cells[col("Message")]).toBe("Source_Capabilities");
  expect(cells[col("ID")]).toMatch(/^[0-7]$/);
  expect(cells[col("Direction")]).toMatch(/source.*sink/i);
  expect(cells[col("Objects")]).toMatch(/^[1-7]$/);
  expect(cells[col("Revision")]).toMatch(/3/);
  expect(cells[col("VBUS")]).toMatch(/\d.*V$/);
  expect(cells[col("IBUS")]).toMatch(/\d.*A$/);
  expect(cells[col("Summary")]).toMatch(/^Source_Capabilities/);
});

test("event rows look different from message rows (AC-TGE83C.2)", async ({ page }) => {
  await openPd(page);
  await capture(page);
  await stopCapture(page);
  await scrollToTop(page);
  const ev = dataRows(page).and(page.locator('[data-kind="event"]')).first();
  const msg = dataRows(page).and(page.locator('[data-kind="message"]')).first();
  await expect(ev).toBeVisible();
  await expect(msg).toBeVisible();
  const look = (el: Element) => {
    const s = getComputedStyle(el);
    const cell = el.querySelector('[role="gridcell"]');
    const c = cell ? getComputedStyle(cell) : s;
    return [s.backgroundColor, c.color, c.fontStyle, c.fontWeight].join("|");
  };
  expect(await ev.evaluate(look)).not.toBe(await msg.evaluate(look));
});

test("details that cannot be loaded say so instead of loading forever (AC-TGE83C.4)", async ({
  page,
}) => {
  await openPd(page);
  await capture(page);
  await stopCapture(page);
  // The meter goes away: the rows stay, but their details can no longer be asked for.
  await page.getByRole("button", { name: "Disconnect", exact: true }).click();
  await expect(startButton(page)).toBeDisabled();
  await scrollToTop(page);
  const caps = rowNamed(page, "Source_Capabilities");
  await caps.click();
  const pane = details(page);
  await expect(pane).toContainText(/details unavailable/i);
  await expect(pane).not.toContainText("Loading");
});

// ---- AC-TGE83C.3: GoodCRC and the filters ----

test("GoodCRC is hidden by default and the toggle shows it (AC-TGE83C.3)", async ({ page }) => {
  await openPd(page);
  await capture(page);
  await stopCapture(page);
  await scrollToTop(page);
  const toggle = pdView(page).getByRole("checkbox", { name: "Show GoodCRC" });
  await expect(toggle).not.toBeChecked();
  expect(await messageTexts(page)).not.toContain("GoodCRC");
  const hidden = await rowCount(page);
  await toggle.check();
  await expect.poll(() => rowCount(page)).toBeGreaterThan(hidden);
  await scrollToTop(page);
  await expect.poll(async () => (await messageTexts(page)).includes("GoodCRC")).toBe(true);
  await toggle.uncheck();
  await expect.poll(() => rowCount(page)).toBe(hidden);
});

test("the message type and SOP filters narrow the table without dropping data (AC-TGE83C.3)", async ({
  page,
}) => {
  await openPd(page);
  await capture(page);
  await stopCapture(page);
  const all = await rowCount(page);

  const type = pdView(page).getByLabel("Message type", { exact: true });
  await type.selectOption({ label: "Request" });
  await expect.poll(() => rowCount(page)).toBeLessThan(all);
  expect(await rowCount(page)).toBeGreaterThan(0);
  await scrollToTop(page);
  expect(new Set(await messageTexts(page))).toEqual(new Set(["Request"]));
  await type.selectOption({ label: "All" });
  await expect.poll(() => rowCount(page)).toBe(all);

  const sop = pdView(page).getByLabel("SOP", { exact: true });
  await sop.selectOption({ label: "SOP'" });
  await expect.poll(() => rowCount(page)).toBeLessThan(all);
  await scrollToTop(page);
  const sops = await dataRows(page)
    .and(page.locator('[data-kind="message"]'))
    .evaluateAll(
      (rows, i) =>
        rows.map((r) =>
          (r.querySelectorAll('[role="gridcell"]')[i] as HTMLElement).innerText.trim(),
        ),
      col("SOP"),
    );
  expect(sops.length).toBeGreaterThan(0);
  expect(new Set(sops)).toEqual(new Set(["SOP'"]));
  await sop.selectOption({ label: "All" });
  await expect.poll(() => rowCount(page)).toBe(all);
});

// ---- AC-TGE83C.4: details ----

test("selecting a row shows its field tree and raw bytes, highlighting the selected field (AC-TGE83C.4)", async ({
  page,
}) => {
  await openPd(page);
  await capture(page);
  await stopCapture(page);
  await scrollToTop(page);
  const caps = rowNamed(page, "Source_Capabilities");
  const objects = Number((await caps.getByRole("gridcell").nth(col("Objects")).innerText()).trim());
  await caps.click();
  await expect(caps).toHaveAttribute("aria-selected", "true");

  const pane = details(page);
  const tree = pane.getByRole("tree", { name: "Fields" });
  await expect(tree.getByRole("treeitem", { name: /^Header/ })).toBeVisible();
  await expect(tree.getByRole("treeitem", { name: /^PDO 1/ })).toBeVisible();

  const bytes = pane.locator("[data-byte]");
  await expect(bytes).toHaveCount(2 + 4 * objects);
  for (const t of await bytes.allInnerTexts()) expect(t.trim()).toMatch(/^[0-9A-F]{2}$/);

  const marked = async () =>
    pane
      .locator("mark[data-byte]")
      .evaluateAll((els) => els.map((e) => Number(e.getAttribute("data-byte"))));

  await tree.getByRole("treeitem", { name: /^Header/ }).click();
  await expect.poll(marked).toEqual([0, 1]);

  const pdo1 = tree.getByRole("treeitem", { name: /^PDO 1/ });
  await pdo1.click();
  await expect.poll(marked).toEqual([2, 3, 4, 5]);

  // Open PDO 1 and pick its Voltage field (bits 19..10 of the word at byte 2).
  await page.keyboard.press("ArrowRight");
  await tree
    .getByRole("treeitem", { name: /^Voltage/ })
    .first()
    .click();
  await expect.poll(marked).toEqual([3, 4]);
});

// ---- AC-TGE83C.5: follow ----

test("the table follows new rows; scrolling up pauses it and Follow resumes (AC-TGE83C.5)", async ({
  page,
}) => {
  // A seeded capture fills the table at once; live rows keep coming after it.
  await openPd(page, "?pdsynthetic=500");
  await startButton(page).click();
  const follow = pdView(page).getByRole("button", { name: "Follow" });
  await expect(follow).toHaveAttribute("aria-pressed", "true");
  await expect.poll(() => rowCount(page)).toBeGreaterThan(200);
  await expect.poll(() => atBottom(page)).toBe(true);
  // New rows arrive and the view stays at the bottom with them.
  const before = await rowCount(page);
  await expect.poll(() => rowCount(page), { timeout: 10_000 }).toBeGreaterThan(before);
  await expect.poll(() => atBottom(page)).toBe(true);
  const newest = await dataRows(page).evaluateAll((rows) =>
    Math.max(...rows.map((r) => Number(r.getAttribute("data-index")))),
  );
  expect(newest).toBeGreaterThanOrEqual(before - 1);

  // Scroll up: following pauses and the view stays where the user put it.
  const box = await grid(page).boundingBox();
  if (!box) throw new Error("no grid");
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.wheel(0, -2_000);
  await expect(follow).toHaveAttribute("aria-pressed", "false");
  const parked = await grid(page).evaluate((g) => g.scrollTop);
  const count = await rowCount(page);
  await expect.poll(() => rowCount(page), { timeout: 10_000 }).toBeGreaterThan(count);
  expect(await grid(page).evaluate((g) => g.scrollTop)).toBeCloseTo(parked, -1);
  expect(await atBottom(page)).toBe(false);

  // Follow resumes it.
  await follow.click();
  await expect(follow).toHaveAttribute("aria-pressed", "true");
  await expect.poll(() => atBottom(page)).toBe(true);
});

// ---- AC-TGE83C.7: clear, keyboard, screen reader labels, axe ----

test("Clear asks in the app first; Cancel keeps the capture, Clear empties it (AC-TGE83C.7)", async ({
  page,
}) => {
  noNativeDialogs(page);
  await openPd(page);
  await capture(page);
  await stopCapture(page);
  const n = await rowCount(page);
  const clear = pdView(page).getByRole("button", { name: "Clear capture" });

  await clear.click();
  const confirm = page.getByRole("alertdialog");
  await expect(confirm).toBeVisible();
  await expect(confirm).toContainText(/clear/i);
  await confirm.getByRole("button", { name: "Cancel" }).click();
  await expect(confirm).toBeHidden();
  expect(await rowCount(page)).toBe(n);

  await clear.click();
  await page.getByRole("alertdialog").getByRole("button", { name: "Clear", exact: true }).click();
  await expect.poll(() => rowCount(page)).toBe(0);
  await expect(dataRows(page)).toHaveCount(0);
  await expect(details(page).locator("[data-byte]")).toHaveCount(0);
});

test("the table and details work with the keyboard alone (AC-TGE83C.7)", async ({ page }) => {
  await openPd(page);
  await capture(page);
  await stopCapture(page);
  await scrollToTop(page);
  await grid(page).focus();
  await page.keyboard.press("Home");
  const first = dataRows(page).first();
  await expect(first).toHaveAttribute("aria-selected", "true");
  const firstIndex = await first.getAttribute("data-index");
  await page.keyboard.press("ArrowDown");
  const selected = grid(page).locator('[role="row"][aria-selected="true"]');
  await expect(selected).toHaveCount(1);
  await expect(selected).not.toHaveAttribute("data-index", firstIndex ?? "");
  // Move to a message row and open its details with the keyboard.
  for (let i = 0; i < 10; i++) {
    if ((await selected.getAttribute("data-kind")) === "message") break;
    await page.keyboard.press("ArrowDown");
  }
  await expect(selected).toHaveAttribute("data-kind", "message");
  const tree = details(page).getByRole("tree", { name: "Fields" });
  await expect(tree.getByRole("treeitem").first()).toBeVisible();

  // Tab reaches the field tree; arrows move through it and the highlight follows.
  let inTree = false;
  for (let i = 0; i < 10 && !inTree; i++) {
    await page.keyboard.press("Tab");
    inTree = await tree.evaluate((t) => t.contains(document.activeElement));
  }
  expect(inTree, "Tab reaches the field tree").toBe(true);
  await page.keyboard.press("Home");
  const marks = details(page).locator("mark[data-byte]");
  await expect(tree.locator('[role="treeitem"][aria-selected="true"]')).toHaveCount(1);
  await expect.poll(() => marks.count()).toBeGreaterThan(0);
  const firstMarks = await marks.evaluateAll((els) =>
    els.map((e) => e.getAttribute("data-byte")).join(","),
  );
  await page.keyboard.press("ArrowDown");
  await expect
    .poll(() => marks.evaluateAll((els) => els.map((e) => e.getAttribute("data-byte")).join(",")))
    .not.toBe(firstMarks);
});

test("every row has a screen reader label naming its message (AC-TGE83C.7)", async ({ page }) => {
  await openPd(page);
  await capture(page);
  await stopCapture(page);
  await scrollToTop(page);
  const labelled = await dataRows(page).evaluateAll(
    (rows, i) =>
      rows.map((r) => ({
        label: r.getAttribute("aria-label") ?? "",
        message: (r.querySelectorAll('[role="gridcell"]')[i] as HTMLElement).innerText.trim(),
      })),
    col("Message"),
  );
  expect(labelled.length).toBeGreaterThan(0);
  for (const { label, message } of labelled) {
    expect(label, `row ${message}`).toContain(message);
  }
});

for (const theme of ["light", "dark"] as const satisfies readonly Theme[]) {
  test(`axe: the PD view with a capture and details, ${theme} (AC-TGE83C.7)`, async ({ page }) => {
    await useTheme(page, theme);
    await openPd(page);
    await capture(page);
    await stopCapture(page);
    await scrollToTop(page);
    await rowNamed(page, "Source_Capabilities").click();
    await expect(details(page).locator("[data-byte]").first()).toBeVisible();
    const results = await new AxeBuilder({ page })
      .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa", "best-practice"])
      .analyze();
    const blocking = results.violations.filter(
      (v) => v.impact === "serious" || v.impact === "critical",
    );
    expect(
      blocking.map(
        (v) =>
          `${v.impact} ${v.id}: ${v.help} at ${v.nodes.map((n) => n.target.join(" ")).join("; ")}`,
      ),
      "no serious or critical axe violations",
    ).toEqual([]);
  });
}

// ---- AC-TGE83C.8: themes ----

for (const theme of ["light", "dark"] as const satisfies readonly Theme[]) {
  test(`the PD view follows the ${theme} theme (AC-TGE83C.8)`, async ({ page }) => {
    await useTheme(page, theme);
    await openPd(page);
    await capture(page);
    await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
    const bg = await backgroundLuminance(grid(page));
    const row = await backgroundLuminance(dataRows(page).first());
    if (theme === "dark") {
      expect(bg).toBeLessThan(0.2);
      expect(row).toBeLessThan(0.2);
    } else {
      expect(bg).toBeGreaterThan(0.7);
      expect(row).toBeGreaterThan(0.7);
    }
  });
}

test("the PD view follows the system theme (AC-TGE83C.8)", async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem("open-powerz.theme", "system"));
  await page.emulateMedia({ colorScheme: "dark" });
  await openPd(page);
  await capture(page);
  expect(await backgroundLuminance(grid(page))).toBeLessThan(0.2);
  await page.emulateMedia({ colorScheme: "light" });
  await expect.poll(() => backgroundLuminance(grid(page))).toBeGreaterThan(0.7);
});

test("at 900 px the header names fit and the header row stays opaque when scrolled sideways (AC-TGE83C.2)", async ({
  page,
}) => {
  await page.setViewportSize({ width: 900, height: 800 });
  await openPd(page);
  await capture(page);
  await stopCapture(page);
  const head = grid(page).locator('[role="columnheader"]');
  const clipped = await head.evaluateAll((cells) =>
    cells.filter((c) => c.scrollWidth > c.clientWidth).map((c) => c.textContent?.trim()),
  );
  expect(clipped).toEqual([]);
  // Scroll fully right: the header row must still hide the rows scrolled under it.
  await grid(page).evaluate((g) => (g.scrollLeft = g.scrollWidth));
  const headerRow = grid(page).locator('[role="row"][aria-rowindex="1"]');
  const bg = await headerRow.evaluate((r) => getComputedStyle(r).backgroundColor);
  expect(bg).not.toBe("rgba(0, 0, 0, 0)");
  const box = await headerRow.boundingBox();
  const gridBox = await grid(page).boundingBox();
  expect(box && gridBox && box.x + box.width).toBeGreaterThanOrEqual(
    (gridBox?.x ?? 0) + (gridBox?.width ?? 0) - 20,
  );
});
