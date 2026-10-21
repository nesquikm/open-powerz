/**
 * FR M46CTX, keyboard and overlay behaviour (QA findings 6, 7, 8 and 16).
 *
 * - AC-M46CTX.1: Escape cancels the clear-history confirm; focus returns to
 *   "Clear chart history…".
 * - AC-M46CTX.2: the ⋯ menu follows the menu pattern (roles, arrow keys, Home / End,
 *   Enter / Space pick, closes after a pick, on an outside click and on Escape,
 *   focus back on ⋯).
 * - AC-M46CTX.3: closing About returns focus to what opened it (About, or ⋯).
 * - AC-M46CTX.4: no shortcut description wraps between 900 and 1600 px; Escape is listed.
 */

import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Locator, type Page } from "@playwright/test";
import { blur, open, streamDemo } from "./helpers";

const AXE_TAGS = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa", "best-practice"];

async function expectNoBlockingAxe(page: Page, include: string, what: string): Promise<void> {
  const results = await new AxeBuilder({ page }).include(include).withTags(AXE_TAGS).analyze();
  const blocking = results.violations.filter(
    (v) => v.impact === "serious" || v.impact === "critical",
  );
  expect(
    blocking.map(
      (v) =>
        `${v.impact} ${v.id}: ${v.help} at ${v.nodes.map((n) => n.target.join(" ")).join("; ")}`,
    ),
    `no serious or critical axe violations in ${what}`,
  ).toEqual([]);
}

function moreButton(page: Page): Locator {
  return page.getByRole("button", { name: /^More:/ });
}

function menu(page: Page): Locator {
  return page.locator("#top-bar-menu");
}

function menuItems(page: Page): Locator {
  return menu(page).locator('[role="menuitem"], [role="menuitemradio"]');
}

/** Open the ⋯ menu from the keyboard. */
async function openMenuWithKeyboard(page: Page): Promise<void> {
  await moreButton(page).focus();
  await page.keyboard.press("Enter");
  await expect(menu(page)).toBeVisible();
}

/** Press ArrowDown until the focused menu item's text is `name` (at most one lap). */
async function arrowTo(page: Page, name: string): Promise<void> {
  const count = await menuItems(page).count();
  for (let i = 0; i <= count; i++) {
    const text = await page.evaluate(() => document.activeElement?.textContent?.trim() ?? "");
    if (text === name) return;
    await page.keyboard.press("ArrowDown");
  }
  throw new Error(`ArrowDown never reached the "${name}" menu item`);
}

test.describe("AC-M46CTX.1: Escape cancels the clear confirm", () => {
  test.use({ viewport: { width: 1600, height: 900 } });

  test("Escape closes the confirm without clearing and focus is back on the button", async ({
    page,
  }) => {
    await open(page);
    await streamDemo(page);
    const panel = page.getByRole("complementary", { name: "Statistics" });
    const clear = panel.getByRole("button", { name: "Clear chart history…" });
    await clear.click();
    const cancel = panel.getByRole("button", { name: "Cancel", exact: true });
    await expect(cancel).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(cancel).toHaveCount(0);
    await expect(panel.getByRole("button", { name: "Clear", exact: true })).toHaveCount(0);
    await expect(panel.getByRole("button", { name: "Clear chart history…" })).toBeFocused();
  });

  test("Escape cancels even when focus moved to Clear inside the confirm", async ({ page }) => {
    await open(page);
    await streamDemo(page);
    const panel = page.getByRole("complementary", { name: "Statistics" });
    await panel.getByRole("button", { name: "Clear chart history…" }).click();
    await panel.getByRole("button", { name: "Clear", exact: true }).focus();
    await page.keyboard.press("Escape");
    await expect(panel.getByRole("button", { name: "Cancel", exact: true })).toHaveCount(0);
    await expect(panel.getByRole("button", { name: "Clear chart history…" })).toBeFocused();
  });
});

test.describe("AC-M46CTX.2: the ⋯ menu follows the menu pattern (900 px)", () => {
  test.use({ viewport: { width: 900, height: 700 } });

  test("menu roles: the button announces a menu, the panel is a menu of menu items", async ({
    page,
  }) => {
    await open(page);
    const more = moreButton(page);
    await expect(more).toHaveAttribute("aria-haspopup", "menu");
    await more.click();
    await expect(menu(page)).toHaveAttribute("role", "menu");
    await expect(more).toHaveAttribute("aria-expanded", "true");
    const items = menuItems(page);
    expect(await items.count()).toBeGreaterThanOrEqual(4);
    await expect(menu(page).getByRole("menuitem", { name: "About" })).toBeVisible();
    for (const name of ["System", "Light", "Dark"]) {
      await expect(menu(page).getByRole("menuitemradio", { name })).toBeVisible();
    }
    await expect(menu(page).getByRole("menuitemradio", { checked: true })).toHaveCount(1);
    await expectNoBlockingAxe(page, "header.top-bar", "the open ⋯ menu");
  });

  test("ArrowDown / ArrowUp / Home / End move focus between items", async ({ page }) => {
    await open(page);
    await openMenuWithKeyboard(page);
    const items = menuItems(page);
    const count = await items.count();
    expect(count).toBeGreaterThanOrEqual(2);
    await expect(items.nth(0), "opening from the keyboard focuses the first item").toBeFocused();
    await page.keyboard.press("ArrowDown");
    await expect(items.nth(1)).toBeFocused();
    await page.keyboard.press("ArrowUp");
    await expect(items.nth(0)).toBeFocused();
    await page.keyboard.press("End");
    await expect(items.nth(count - 1)).toBeFocused();
    await page.keyboard.press("Home");
    await expect(items.nth(0)).toBeFocused();
    // Only one item is in the Tab order (roving tabindex).
    const tabbable = await items.evaluateAll(
      (els) => els.filter((el) => (el as HTMLElement).tabIndex >= 0).length,
    );
    expect(tabbable).toBe(1);
    // The menu's arrow keys do not reach the window shortcuts: still open, nothing else moved.
    await expect(menu(page)).toBeVisible();
  });

  test("Enter picks an item, the menu closes and focus returns to ⋯", async ({ page }) => {
    await open(page);
    await openMenuWithKeyboard(page);
    await arrowTo(page, "Dark");
    await page.keyboard.press("Enter");
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
    await expect(menu(page)).toBeHidden();
    await expect(moreButton(page)).toHaveAttribute("aria-expanded", "false");
    await expect(moreButton(page)).toBeFocused();
  });

  test("Space picks an item and the menu closes", async ({ page }) => {
    await open(page);
    await openMenuWithKeyboard(page);
    await arrowTo(page, "Dark");
    await page.keyboard.press("Enter");
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
    await openMenuWithKeyboard(page);
    await arrowTo(page, "Light");
    await page.keyboard.press(" ");
    await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
    await expect(menu(page)).toBeHidden();
    await expect(moreButton(page)).toBeFocused();
  });

  test("a mouse pick closes the menu", async ({ page }) => {
    await open(page);
    await moreButton(page).click();
    await menu(page).getByRole("menuitemradio", { name: "Dark" }).click();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
    await expect(menu(page)).toBeHidden();
  });

  test("a click outside closes the menu", async ({ page }) => {
    await open(page);
    await moreButton(page).click();
    await expect(menu(page)).toBeVisible();
    await page.getByRole("complementary", { name: "Statistics" }).getByRole("heading").click();
    await expect(menu(page)).toBeHidden();
    await expect(moreButton(page)).toHaveAttribute("aria-expanded", "false");
  });

  test("a click inside the menu (not on an item) keeps it open", async ({ page }) => {
    await open(page);
    await moreButton(page).click();
    const box = await menu(page).boundingBox();
    expect(box).not.toBeNull();
    // The menu's padding, top-left corner.
    await page.mouse.click(box!.x + 3, box!.y + 3);
    await expect(menu(page)).toBeVisible();
  });

  test("Escape closes the menu and focus returns to ⋯", async ({ page }) => {
    await open(page);
    await openMenuWithKeyboard(page);
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("Escape");
    await expect(menu(page)).toBeHidden();
    await expect(moreButton(page)).toBeFocused();
  });
});

test.describe("AC-M46CTX.3: closing About returns focus to its opener", () => {
  test("1600 px: About from the inline button, closed with Escape", async ({ page }) => {
    await page.setViewportSize({ width: 1600, height: 900 });
    await open(page);
    const about = page.getByRole("button", { name: "About", exact: true });
    await about.focus();
    await page.keyboard.press("Enter");
    const dialog = page.getByRole("dialog", { name: "About" });
    await expect(dialog).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(dialog).toHaveCount(0);
    await expect(about).toBeFocused();
  });

  test("1600 px: About from the inline button, closed with its Close button", async ({ page }) => {
    await page.setViewportSize({ width: 1600, height: 900 });
    await open(page);
    const about = page.getByRole("button", { name: "About", exact: true });
    await about.click();
    const dialog = page.getByRole("dialog", { name: "About" });
    await expect(dialog).toBeVisible();
    await dialog.getByRole("button", { name: "Close About" }).click();
    await expect(dialog).toHaveCount(0);
    await expect(about).toBeFocused();
  });

  test("900 px: About from the ⋯ menu, closed with Escape, focus on ⋯", async ({ page }) => {
    await page.setViewportSize({ width: 900, height: 700 });
    await open(page);
    await openMenuWithKeyboard(page);
    await arrowTo(page, "About");
    await page.keyboard.press("Enter");
    const dialog = page.getByRole("dialog", { name: "About" });
    await expect(dialog).toBeVisible();
    await expect(menu(page)).toBeHidden();
    await page.keyboard.press("Escape");
    await expect(dialog).toHaveCount(0);
    await expect(moreButton(page)).toBeFocused();
  });

  test("900 px: About from the ⋯ menu, closed with its Close button, focus on ⋯", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 900, height: 700 });
    await open(page);
    await moreButton(page).click();
    await menu(page).getByRole("menuitem", { name: "About" }).click();
    const dialog = page.getByRole("dialog", { name: "About" });
    await expect(dialog).toBeVisible();
    await dialog.getByRole("button", { name: "Close About" }).click();
    await expect(dialog).toHaveCount(0);
    await expect(moreButton(page)).toBeFocused();
  });
});

test.describe("AC-M46CTX.4: the shortcut overlay", () => {
  for (const width of [900, 1200, 1600] as const) {
    test(`${width} px: no row wraps and Escape is listed`, async ({ page }) => {
      await page.setViewportSize({ width, height: 900 });
      await open(page);
      await blur(page);
      await page.keyboard.press("?");
      const dialog = page.getByRole("dialog", { name: "Keyboard shortcuts" });
      await expect(dialog).toBeVisible();

      await expect(dialog.locator("dt").filter({ hasText: /^\s*Esc(ape)?\s*$/ })).toHaveCount(1);

      // Every description and key label sits on one line: its text lays out in one line box.
      const rows = await dialog.locator("dt, dd").evaluateAll((els) =>
        els.map((el) => {
          const range = document.createRange();
          range.selectNodeContents(el);
          const tops = new Set(
            [...range.getClientRects()].filter((r) => r.width > 0).map((r) => Math.round(r.top)),
          );
          const style = getComputedStyle(el);
          return {
            text: (el.textContent ?? "").trim(),
            lines: tops.size,
            clipped: el.scrollWidth > el.clientWidth + 1 || style.textOverflow === "ellipsis",
          };
        }),
      );
      expect(rows.length).toBeGreaterThan(0);
      for (const row of rows) {
        expect(row.lines, `"${row.text}" stays on one line`).toBe(1);
        expect(row.clipped, `"${row.text}" is not cut off`).toBe(false);
      }

      // The overlay stays inside the window.
      const box = await dialog.boundingBox();
      expect(box).not.toBeNull();
      expect(box!.x).toBeGreaterThanOrEqual(0);
      expect(box!.x + box!.width).toBeLessThanOrEqual(width);

      await expectNoBlockingAxe(page, "#shortcut-help", "the shortcut overlay");
    });
  }
});

test.describe("review fixes (FR M46CTX)", () => {
  test.use({ viewport: { width: 900, height: 800 } });

  test("Escape in the menu closes only the menu, not an open shortcut overlay", async ({
    page,
  }) => {
    await open(page);
    await blur(page);
    await page.keyboard.press("?");
    const overlay = page.locator("#shortcut-help");
    await expect(overlay).toBeVisible();
    await openMenuWithKeyboard(page);
    await page.keyboard.press("Escape");
    await expect(menu(page)).toBeHidden();
    await expect(overlay).toBeVisible();
  });

  test("Tab out of the menu closes it", async ({ page }) => {
    await open(page);
    await openMenuWithKeyboard(page);
    await page.keyboard.press("Tab");
    await expect(menu(page)).toBeHidden();
    await expect(moreButton(page)).toHaveAttribute("aria-expanded", "false");
  });

  test("keys pressed in the menu don't reach the chart shortcuts", async ({ page }) => {
    await open(page);
    await streamDemo(page);
    const elapsed = page.locator(
      '//*[@aria-label="Statistics"]//dt[normalize-space()="Elapsed"]/following-sibling::dd[1]',
    );
    await expect.poll(async () => (await elapsed.textContent())?.trim()).not.toBe("0:00:00");
    await openMenuWithKeyboard(page);
    await page.keyboard.press("r");
    // R resets the statistics; inside the menu it must do nothing (read once, no retry).
    await page.waitForTimeout(150);
    expect((await elapsed.textContent())?.trim()).not.toBe("0:00:00");
    await expect(menu(page)).toBeVisible();
  });
});
