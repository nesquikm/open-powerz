/**
 * FR 60QB2S on Chromium: the built web edition shows an (i) beside the device menu that
 * explains the connections.
 *
 * - AC-60QB2S.1: the (i) button sits right after the Device select, has an accessible
 *   name and is reachable by keyboard.
 * - AC-60QB2S.2: it opens a glossary-style popover: a second press closes it, Escape or
 *   a click outside closes it and focus returns to the button.
 * - AC-60QB2S.3 / .4 / .5: the popover names WebUSB, HID and Web Serial, the browsers,
 *   links to the desktop app download page, and says the session uses the demo meter.
 * - AC-60QB2S.7: at 1280 and 1600 px the top bar does not overflow with the button in it.
 */

import { expect, test, type Locator, type Page } from "@playwright/test";
import { badge, DEMO_LOAD } from "../ui/helpers";

const BASE = "/open-powerz/";
const RELEASES = "https://github.com/nesquikm/open-powerz/releases";

async function openSite(page: Page): Promise<void> {
  await page.goto(BASE);
  const picker = page.getByLabel("Device", { exact: true });
  await expect(picker).toBeEnabled();
  await expect(picker.locator(`option[value="${DEMO_LOAD}"]`)).toHaveCount(1);
}

async function streamDemo(page: Page): Promise<void> {
  await page.getByLabel("Device", { exact: true }).selectOption(DEMO_LOAD);
  await page.getByRole("button", { name: "Connect", exact: true }).click();
  await expect(badge(page)).toHaveText("Streaming", { timeout: 15_000 });
}

/** The connection (i) button. */
const infoButton = (page: Page): Locator =>
  page.getByRole("button", { name: "About connections", exact: true });

/** The popover the button controls. */
async function popoverOf(page: Page, trigger: Locator): Promise<Locator> {
  const id = await trigger.getAttribute("aria-controls");
  expect(id, "the button points at its popover").toBeTruthy();
  return page.locator(`[id="${id}"]`);
}

/** WebKit has no WebUSB or WebHID, so the page would show its not-supported view. For
 * the layout check only, stand in an empty WebUSB and WebHID there: the web edition then
 * draws its top bar with the demo meter, as Chrome does (AC-60QB2S.7 on WebKit). */
async function standInWebUsb(page: Page): Promise<void> {
  await page.addInitScript(() => {
    const api = {
      getDevices: async () => [],
      requestDevice: async () => {
        throw new DOMException("No device selected.", "NotFoundError");
      },
      addEventListener: () => {},
      removeEventListener: () => {},
    };
    for (const name of ["usb", "hid"]) {
      if (!(name in navigator)) Object.defineProperty(navigator, name, { value: api });
    }
  });
}

for (const width of [1280, 1600]) {
  test(`the (i) sits beside the Device menu and the top bar fits at ${width} px (AC-60QB2S.1, AC-60QB2S.7)`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 900 });
    await standInWebUsb(page);
    await openSite(page);
    const button = infoButton(page);
    await expect(button).toBeVisible();
    await expect(button).toHaveAttribute("data-glossary-key", "section.connection");

    // Right after the select, on the same row.
    const select = await page.getByLabel("Device", { exact: true }).boundingBox();
    const box = await button.boundingBox();
    expect(select).not.toBeNull();
    expect(box).not.toBeNull();
    if (select && box) {
      expect(box.x, "after the select").toBeGreaterThanOrEqual(select.x + select.width - 1);
      expect(box.x - (select.x + select.width), "next to it").toBeLessThan(48);
      const mid = box.y + box.height / 2;
      expect(mid).toBeGreaterThan(select.y);
      expect(mid).toBeLessThan(select.y + select.height);
      // Inside the window.
      expect(box.x).toBeGreaterThanOrEqual(0);
      expect(box.x + box.width).toBeLessThanOrEqual(width);
    }

    const bar = await page.locator(".top-bar").evaluate((el) => ({
      scrollWidth: el.scrollWidth,
      clientWidth: el.clientWidth,
    }));
    expect(bar.scrollWidth, "nothing sticks out of the bar").toBeLessThanOrEqual(bar.clientWidth);
    const doc = await page.evaluate(() => ({
      scrollWidth: document.documentElement.scrollWidth,
      clientWidth: document.documentElement.clientWidth,
    }));
    expect(doc.scrollWidth, "no horizontal scroll").toBeLessThanOrEqual(doc.clientWidth);
  });
}

test("the popover explains the connections and names the demo meter (AC-60QB2S.3-.5)", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await openSite(page);
  await streamDemo(page);

  const button = infoButton(page);
  await expect(button).toHaveAttribute("aria-expanded", "false");
  await button.click();
  const popover = await popoverOf(page, button);
  await expect(popover).toBeVisible();
  await expect(popover).toHaveAttribute("role", "dialog");
  await expect(button).toHaveAttribute("aria-expanded", "true");

  await expect(popover).toContainText("WebUSB");
  await expect(popover).toContainText("Connect over HID…");
  await expect(popover).toContainText(/basic readings only/i);
  await expect(popover).toContainText("Web Serial");
  await expect(popover).toContainText(/Chrome or Edge/);
  await expect(popover).toContainText(/Safari and Firefox/);
  await expect(popover).toContainText(/the demo meter/i);
  await expect(popover.locator(`a[href="${RELEASES}"]`)).toHaveCount(1);
});

test("Escape, a second press and an outside click close it; focus returns (AC-60QB2S.2)", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await openSite(page);
  const button = infoButton(page);
  const popover = await popoverOf(page, button);

  // Keyboard: focus and Enter open it, Escape closes it and focus is back on the button.
  await button.focus();
  await page.keyboard.press("Enter");
  await expect(popover).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(popover).toBeHidden();
  await expect(button).toBeFocused();

  // A second press closes it.
  await button.click();
  await expect(popover).toBeVisible();
  await button.click();
  await expect(popover).toBeHidden();

  // A click outside closes it and focus returns to the button. The click lands below the
  // top bar's views strip, which overlaps the top of <main>.
  await button.click();
  await expect(popover).toBeVisible();
  await page
    .locator("main")
    .first()
    .click({ position: { x: 5, y: 200 } });
  await expect(popover).toBeHidden();
  await expect(button).toBeFocused();
});

test("only one popover is open at a time (AC-60QB2S.2)", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await openSite(page);
  const button = infoButton(page);
  const second = page
    .locator('button.info-button:not([data-glossary-key="section.connection"])')
    .first();
  await expect(second).toBeVisible();
  const mine = await popoverOf(page, button);
  const theirs = await popoverOf(page, second);

  await button.click();
  await expect(mine).toBeVisible();
  await second.click();
  await expect(theirs).toBeVisible();
  await expect(mine).toBeHidden();
});
