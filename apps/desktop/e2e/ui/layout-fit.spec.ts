/**
 * Layout rough edges of FR EWQ6FQ, measured from bounding boxes:
 *
 * - AC-EWQ6FQ.2: at 375 px the four main tabs (Meter, PD analyzer, Records, Trigger)
 *   and the Open recording and Add meter buttons are all whole and hit-testable, the
 *   page does not scroll sideways, and the two buttons sit on their own line under the
 *   tabs; at 900 px and wider tabs and buttons share one row.
 * - AC-EWQ6FQ.3: in Chart options each control sits at most 4 px from its own (i)
 *   button, on the same line, and the next control's group starts at least 16 px after
 *   that (i), at 1280 px and 375 px.
 * - AC-EWQ6FQ.4: the Meter settings title is one line at 1280 px and 900 px, with its
 *   (i), Refresh and Edit buttons visible.
 * - AC-EWQ6FQ.8: at 375 px each series' style row (colour, width, transparency, Reset)
 *   lies inside the Chart options panel and the viewport and is hit-testable, with no
 *   sideways page scroll.
 */

import { expect, test, type Locator, type Page } from "@playwright/test";
import { open, streamDemo } from "./helpers";

const MAIN_TABS = ["Meter", "PD analyzer", "Records", "Trigger"] as const;
const STRIP_BUTTONS = ["Open recording", "Add meter"] as const;

interface Box {
  left: number;
  right: number;
  top: number;
  bottom: number;
}

async function box(locator: Locator): Promise<Box> {
  const b = await locator.boundingBox();
  expect(b, "laid out").toBeTruthy();
  return { left: b!.x, right: b!.x + b!.width, top: b!.y, bottom: b!.y + b!.height };
}

const centre = (b: Box): number => (b.top + b.bottom) / 2;

function tab(page: Page, name: string): Locator {
  return page.getByRole("tablist", { name: "Views" }).getByRole("tab", { name, exact: true });
}

function stripButton(page: Page, name: string): Locator {
  return page.getByRole("button", { name, exact: true });
}

/** Points along the element (inset 2 px from each end) where something else is on top,
 * or where the element is clipped away by a scrolling parent. */
function hiddenPoints(locator: Locator): Promise<number[]> {
  return locator.evaluate((el) => {
    const r = el.getBoundingClientRect();
    const middle = r.top + r.height / 2;
    return [r.left + 2, r.left + r.width / 2, r.right - 2].filter((x) => {
      const hit = document.elementFromPoint(x, middle);
      return !hit || !(hit === el || el.contains(hit));
    });
  });
}

async function expectWhole(page: Page, locator: Locator, label: string): Promise<void> {
  await expect(locator, `${label} shown`).toBeVisible();
  const width = page.viewportSize()!.width;
  const b = await box(locator);
  expect(b.left, `${label} cut off on the left`).toBeGreaterThanOrEqual(0);
  expect(b.right, `${label} cut off on the right`).toBeLessThanOrEqual(width);
  expect(await hiddenPoints(locator), `${label}: points covered or clipped`).toEqual([]);
}

async function expectNoSidewaysScroll(page: Page): Promise<void> {
  const scroll = await page.evaluate(() => ({
    documentWidth: document.documentElement.scrollWidth,
    windowWidth: document.documentElement.clientWidth,
  }));
  expect(scroll.documentWidth, "no horizontal scroll").toBeLessThanOrEqual(scroll.windowWidth);
}

test.describe("AC-EWQ6FQ.2: the main tab row", () => {
  test("at 375 px every main tab and both buttons are whole and clickable, buttons under the tabs", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 375, height: 812 });
    await open(page);
    await streamDemo(page);
    await expectNoSidewaysScroll(page);

    for (const name of STRIP_BUTTONS) await expectWhole(page, stripButton(page, name), name);
    for (const name of MAIN_TABS) await expectWhole(page, tab(page, name), `tab ${name}`);

    // The two buttons have their own line, under the tabs.
    const tabsBottom = Math.max(
      ...(await Promise.all(MAIN_TABS.map((n) => box(tab(page, n))))).map((b) => b.bottom),
    );
    for (const name of STRIP_BUTTONS) {
      const b = await box(stripButton(page, name));
      expect(b.top, `${name} below the tabs`).toBeGreaterThanOrEqual(tabsBottom - 1);
    }
    const [openBox, addBox] = await Promise.all(
      STRIP_BUTTONS.map((n) => box(stripButton(page, n))),
    );
    expect(
      Math.abs(centre(openBox!) - centre(addBox!)),
      "the two buttons share a line",
    ).toBeLessThan(4);

    // Each tab takes a click where it is (no scrolling of the tab list needed).
    for (const name of MAIN_TABS) {
      const t = tab(page, name);
      if ((await t.getAttribute("aria-disabled")) === "true") continue;
      await t.click();
      await expect(t, `tab ${name} selected by a click`).toHaveAttribute("aria-selected", "true");
    }
    await tab(page, "Meter").click();
    await expectNoSidewaysScroll(page);
  });

  for (const width of [900, 1280] as const) {
    test(`at ${width} px tabs and buttons stay on one row`, async ({ page }) => {
      await page.setViewportSize({ width, height: 800 });
      await open(page);
      await streamDemo(page);
      await expectNoSidewaysScroll(page);
      const tabBoxes = await Promise.all(MAIN_TABS.map((n) => box(tab(page, n))));
      const buttonBoxes = await Promise.all(STRIP_BUTTONS.map((n) => box(stripButton(page, n))));
      const tabTop = Math.min(...tabBoxes.map((b) => b.top));
      const tabBottom = Math.max(...tabBoxes.map((b) => b.bottom));
      for (const [i, b] of buttonBoxes.entries()) {
        const c = centre(b);
        expect(c, `${STRIP_BUTTONS[i]} on the tab row`).toBeGreaterThan(tabTop);
        expect(c, `${STRIP_BUTTONS[i]} on the tab row`).toBeLessThan(tabBottom);
      }
      for (const name of [...MAIN_TABS, ...STRIP_BUTTONS]) {
        const l = MAIN_TABS.includes(name as never) ? tab(page, name) : stripButton(page, name);
        await expectWhole(page, l, name);
      }
    });
  }
});

interface OptionItem {
  /** "info" for an (i) button, else the control's text or label. */
  kind: "info" | "control";
  name: string;
  left: number;
  right: number;
  centre: number;
}

/** The Chart options group's top-level items in DOM order: labels, buttons, selects,
 * inputs and (i) buttons, without the series-style block (it has no (i) buttons). */
function optionItems(group: Locator): Promise<OptionItem[]> {
  return group.evaluate((root) => {
    const els = [...root.querySelectorAll<HTMLElement>("label, button, select, input")].filter(
      (el) =>
        !el.closest(".series-style") &&
        !el.closest("[role=dialog], [popover]") &&
        el.getClientRects().length > 0 &&
        el.offsetWidth > 0 &&
        // A <label> wrapping its control is measured through the control.
        !(el.tagName === "LABEL" && el.querySelector("input, select")),
    );
    return els.map((el) => {
      const r = el.getBoundingClientRect();
      const info = el.hasAttribute("data-glossary-key");
      return {
        kind: info ? ("info" as const) : ("control" as const),
        name: info
          ? (el.getAttribute("data-glossary-key") ?? "")
          : (el.getAttribute("aria-label") ?? el.innerText ?? el.id).trim() || el.id,
        left: r.left,
        right: r.right,
        centre: r.top + r.height / 2,
      };
    });
  });
}

async function openChartOptions(page: Page): Promise<Locator> {
  const chart = page.getByRole("region", { name: "Chart", exact: true });
  const toggle = chart.getByRole("button", { name: "Chart options", exact: true });
  if ((await toggle.getAttribute("aria-expanded")) !== "true") await toggle.click();
  await expect(toggle).toHaveAttribute("aria-expanded", "true");
  const group = chart.getByRole("group", { name: "Chart options", exact: true });
  await expect(group).toBeVisible();
  return group;
}

for (const width of [1280, 375] as const) {
  test(`AC-EWQ6FQ.3: each Chart option sits with its own (i), groups apart, at ${width} px`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 900 });
    await open(page);
    await streamDemo(page);
    const group = await openChartOptions(page);
    const items = await optionItems(group);
    const infos = items.flatMap((it, i) => (it.kind === "info" ? [i] : []));
    expect(infos.length, "Chart options has (i) buttons").toBeGreaterThanOrEqual(3);

    const problems: string[] = [];
    for (const i of infos) {
      const info = items[i]!;
      const control = items[i - 1];
      if (!control || control.kind === "info") {
        problems.push(`${info.name}: no control before it`);
        continue;
      }
      if (Math.abs(control.centre - info.centre) > 4) {
        problems.push(`${info.name}: not on the same line as ${control.name}`);
      } else {
        const gap = info.left - control.right;
        if (gap < -0.5 || gap > 4) {
          problems.push(`${info.name}: ${gap.toFixed(1)} px from ${control.name} (want 0–4)`);
        }
      }
      const next = items[i + 1];
      // The next group on the same line starts at least 16 px after this (i).
      if (next && Math.abs(next.centre - info.centre) <= 4 && next.left > info.left) {
        const gap = next.left - info.right;
        if (gap < 16) {
          problems.push(`${info.name}: next group (${next.name}) only ${gap.toFixed(1)} px away`);
        }
      }
    }
    expect(problems).toEqual([]);
  });
}

for (const width of [1280, 900] as const) {
  test(`AC-EWQ6FQ.4: the Meter settings title is one line at ${width} px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await open(page);
    await streamDemo(page);
    const panel = page.getByRole("region", { name: "Meter settings" });
    await expect(panel).toBeVisible();
    const title = panel.locator("#meter-settings-title");
    await expect(title).toHaveText("Meter settings");
    const lines = await title.evaluate((el) => {
      const range = document.createRange();
      range.selectNodeContents(el);
      const tops = new Set([...range.getClientRects()].map((r) => Math.round(r.top)));
      return tops.size;
    });
    expect(lines, "title lines").toBe(1);
    for (const control of [
      panel.locator("[data-glossary-key]").first(),
      panel.getByRole("button", { name: "Refresh", exact: true }),
      panel.getByRole("button", { name: "Edit", exact: true }),
    ]) {
      await control.scrollIntoViewIfNeeded();
      await expect(control).toBeVisible();
      expect(await hiddenPoints(control), "covered or clipped").toEqual([]);
    }
  });
}

test("AC-EWQ6FQ.8: at 375 px every series style row fits the Chart options panel", async ({
  page,
}) => {
  await page.setViewportSize({ width: 375, height: 812 });
  await open(page);
  await streamDemo(page);
  const group = await openChartOptions(page);
  const chart = page.getByRole("region", { name: "Chart", exact: true });
  const styles = group.locator(".series-style");
  await expect(styles, "series style block shown").toBeVisible();

  const rows = styles.locator(".series-look");
  const count = await rows.count();
  expect(count, "series style rows").toBeGreaterThanOrEqual(2);

  const problems: string[] = [];
  for (let i = 0; i < count; i++) {
    const row = rows.nth(i);
    const controls: [string, Locator][] = [
      ["colour", row.locator('input[type="color"]')],
      ["width", row.getByRole("spinbutton").nth(0)],
      ["transparency", row.getByRole("spinbutton").nth(1)],
      ["Reset", row.getByRole("button", { name: /^Reset .* style$/ })],
    ];
    for (const [what, control] of controls) {
      await control.scrollIntoViewIfNeeded();
      await expect(control, `row ${i} ${what} shown`).toBeVisible();
      const label = (await control.getAttribute("aria-label")) ?? (await control.innerText());
      const name = `row ${i} ${what} (${label.trim()})`;
      const viewport = page.viewportSize()!.width;
      const [c, g, r] = await Promise.all([box(control), box(group), box(chart)]);
      if (c.left < 0) problems.push(`${name}: cut off on the left of the viewport`);
      if (c.right > viewport + 0.5) {
        problems.push(`${name}: right ${c.right.toFixed(1)} past viewport ${viewport}`);
      }
      if (c.right > g.right + 0.5) {
        problems.push(
          `${name}: right ${c.right.toFixed(1)} past Chart options ${g.right.toFixed(1)}`,
        );
      }
      if (c.right > r.right + 0.5) {
        problems.push(
          `${name}: right ${c.right.toFixed(1)} past Chart region ${r.right.toFixed(1)}`,
        );
      }
      const hidden = await hiddenPoints(control);
      if (hidden.length) problems.push(`${name}: covered or clipped at x=${hidden.join(", ")}`);
    }
  }
  expect(problems).toEqual([]);
  await expectNoSidewaysScroll(page);
});
