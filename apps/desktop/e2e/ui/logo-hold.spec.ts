/**
 * FR 70X5YY (AC .1 to .5): a startup logo write holds every session action, not only the
 * controls that send to the meter. While a mock upload is held mid-write (the page clock
 * paused once the progress shows), Add meter, Open recording, Record, Pause, Recorder
 * options, Export and Export .db are disabled in the top bar, the "⋯" menu and every
 * meter tile, each with a tooltip naming the write; Space does nothing; the PD analyzer,
 * Trigger, Records and Firmware tabs are held. About, the theme, the menu itself and the
 * chart's own view controls stay usable, and all of it comes back when the write ends,
 * verified or failed, also while the view waits for the menu save.
 *
 * Runs in both projects (Chromium and WebKit); `open()` installs the wide font when
 * E2E_WIDE_FONT=1.
 */

import { expect, test, type Locator, type Page } from "@playwright/test";
import { holdClock, open, stepHeldClock, streamDemo } from "./helpers";

const MOCK_HOOK = "__OPEN_POWERZ_MOCK__";
const HOLD_TITLE = /startup logo write is running/i;
const WIDE = { width: 1600, height: 900 };
/** Narrow enough that Export and Export .db move into the "⋯" menu. */
const MENU_WIDTH = { width: 600, height: 800 };

function view(page: Page): Locator {
  return page.getByRole("tabpanel", { name: "Startup logo" });
}

/** A `stepHeldClock` condition: the logo view's text reads `text`. */
function viewSays(page: Page, text: RegExp): () => Promise<boolean> {
  return async () => text.test((await view(page).textContent()) ?? "");
}

function topBar(page: Page): Locator {
  return page.locator(".top-bar");
}

function stripAction(page: Page, name: string): Locator {
  return page.getByRole("button", { name, exact: true });
}

function tab(page: Page, name: string): Locator {
  return page.getByRole("tab", { name, exact: true });
}

function tiles(page: Page): Locator {
  return page.getByRole("region", { name: "Meters" }).getByRole("article");
}

/** Shows the Firmware tab through About's Advanced firmware switch. */
async function showFirmwareTab(page: Page): Promise<void> {
  await page.addInitScript(() => {
    try {
      localStorage.setItem("open-powerz.advanced-firmware", "true");
    } catch {
      // A page without storage just has no Firmware tab.
    }
  });
}

async function choose(v: Locator): Promise<void> {
  await v.getByRole("button", { name: /choose/i }).click();
  await expect(v.getByTestId("logo-preview")).toBeVisible();
}

async function confirmUpload(page: Page, v: Locator): Promise<void> {
  await v.getByRole("button", { name: /^upload$/i }).click();
  const d = page.getByRole("alertdialog", { name: /upload/i });
  await d.getByRole("textbox").fill("upload");
  await d.getByRole("button", { name: /^upload$/i }).click();
  await expect(d).toHaveCount(0);
}

/** Starts a logo upload on the full-view meter and holds it mid-write. */
async function holdWrite(page: Page): Promise<void> {
  await tab(page, "Startup logo").click();
  const v = view(page);
  await expect(v).toBeVisible();
  await choose(v);
  await confirmUpload(page, v);
  await expect(v.getByRole("progressbar")).toBeVisible();
  await holdClock(page);
  await expect(v).toContainText(/chunk|read.?back/i);
}

/** Lets the held write finish and waits for `ending` in the logo view. */
async function finishWrite(page: Page, ending: RegExp): Promise<void> {
  if (!(await view(page).isVisible())) await tab(page, "Startup logo").click();
  await stepHeldClock(page, viewSays(page, ending));
  await page.clock.resume();
  await expect(view(page)).toContainText(ending, { timeout: 30_000 });
}

/** Counts every recording start the mock gets. */
async function spyRecordingStarts(page: Page): Promise<void> {
  await page.evaluate((key) => {
    const w = window as unknown as Record<string, unknown>;
    const mock = w[key] as { startRecording: (...args: unknown[]) => Promise<unknown> };
    w.__RECORDING_STARTS__ = 0;
    const real = mock.startRecording.bind(mock);
    mock.startRecording = async (...args: unknown[]) => {
      w.__RECORDING_STARTS__ = (w.__RECORDING_STARTS__ as number) + 1;
      return real(...args);
    };
  }, MOCK_HOOK);
}

function recordingStarts(page: Page): Promise<number> {
  return page.evaluate(
    () => (window as unknown as { __RECORDING_STARTS__: number }).__RECORDING_STARTS__,
  );
}

async function expectHeld(c: Locator, what: string): Promise<void> {
  await expect(c, `${what} is visible`).toBeVisible();
  await expect(c, `${what} is disabled during the logo write`).toBeDisabled();
  await expect(c, `${what} says why`).toHaveAttribute("title", HOLD_TITLE);
  // And it looks disabled, as a disabled button does (the stage 4 run saw held
  // controls that "looked enabled").
  await expect(c, `${what} looks dimmed`).toHaveCSS("opacity", "0.55");
  await expect(c, `${what} shows the not-allowed cursor`).toHaveCSS("cursor", "not-allowed");
}

async function expectFree(c: Locator, what: string): Promise<void> {
  await expect(c, `${what} is visible`).toBeVisible();
  await expect(c, `${what} is enabled after the write`).toBeEnabled();
  const title = (await c.getAttribute("title")) ?? "";
  expect(title, `${what} no longer names the write`).not.toMatch(HOLD_TITLE);
  await expect(c, `${what} no longer looks dimmed`).toHaveCSS("opacity", "1");
}

/** The full view's top-bar session actions, with a running recording's Pause. */
function topBarActions(page: Page, recording: boolean): [Locator, string][] {
  const bar = topBar(page);
  const list: [Locator, string][] = [
    [stripAction(page, "Add meter"), "Add meter"],
    [stripAction(page, "Open recording"), "Open recording"],
    [
      bar.getByRole("button", { name: recording ? "Stop" : "Record", exact: true }),
      recording ? "Stop" : "Record",
    ],
    [bar.getByRole("button", { name: "Recorder options", exact: true }), "Recorder options"],
    [bar.getByRole("button", { name: "Export", exact: true }), "Export"],
    [bar.getByRole("button", { name: "Export .db", exact: true }), "Export .db"],
  ];
  if (recording) list.push([bar.getByRole("button", { name: /^(pause|resume)$/i }), "Pause"]);
  return list;
}

test.describe("a logo write holds the session actions (FR 70X5YY)", () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize(WIDE);
  });

  test("top-bar actions are disabled with a tooltip and come back verified (AC-70X5YY.1, AC-70X5YY.5)", async ({
    page,
  }) => {
    await page.clock.install();
    await open(page);
    await streamDemo(page);
    for (const [c, what] of topBarActions(page, false)) await expectFree(c, what);

    await holdWrite(page);
    for (const [c, what] of topBarActions(page, false)) await expectHeld(c, what);
    // On the Meter tab too: the hold is not the logo view's alone.
    await tab(page, "Meter").click();
    for (const [c, what] of topBarActions(page, false)) await expectHeld(c, what);

    // Verified, then the view waits for the meter's menu save: nothing stays held.
    await tab(page, "Startup logo").click();
    await stepHeldClock(page, () => view(page).getByTestId("logo-menu-save").isVisible());
    await page.clock.resume();
    await expect(view(page).getByTestId("logo-menu-save")).toBeVisible({ timeout: 30_000 });
    for (const [c, what] of topBarActions(page, false)) await expectFree(c, what);
  });

  test("a recording keeps running, Pause and Stop are held, then it stops (AC-70X5YY.1, AC-70X5YY.4)", async ({
    page,
  }) => {
    await page.clock.install();
    await open(page);
    await streamDemo(page);
    await topBar(page).getByRole("button", { name: "Record", exact: true }).click();
    const timer = topBar(page).getByRole("timer");
    await expect(timer).toBeVisible();

    await holdWrite(page);
    for (const [c, what] of topBarActions(page, true)) await expectHeld(c, what);
    await expect(timer, "the recording keeps running during the write").toBeVisible();

    await finishWrite(page, /verified/i);
    await expect(timer).toBeVisible();
    const stop = topBar(page).getByRole("button", { name: "Stop", exact: true });
    await expectFree(stop, "Stop");
    await expectFree(topBar(page).getByRole("button", { name: /^(pause|resume)$/i }), "Pause");
    await stop.click();
    await expect(timer).toHaveCount(0, { timeout: 10_000 });
  });

  test("every meter tile's actions are held during the write (AC-70X5YY.1)", async ({ page }) => {
    await page.clock.install();
    await open(page);
    await streamDemo(page);
    await stripAction(page, "Add meter").click();
    const dialog = page.getByRole("dialog", { name: "Add a meter" });
    await dialog.getByLabel("Device", { exact: true }).selectOption("demo:pd-9v-2a");
    await dialog.getByRole("button", { name: "Connect" }).click();
    await expect(dialog).toBeHidden();
    await expect(tiles(page)).toHaveCount(2);
    await tiles(page)
      .filter({ hasText: "load-5v-1a" })
      .getByRole("button", { name: "Expand" })
      .click();

    await holdWrite(page);
    await tab(page, "Meter").click();
    await page.getByRole("button", { name: "All meters" }).click();
    await expect(tiles(page)).toHaveCount(2);
    await expectHeld(stripAction(page, "Add meter"), "Add meter");
    await expectHeld(stripAction(page, "Open recording"), "Open recording");
    for (const tile of await tiles(page).all()) {
      for (const name of ["Record", "Recorder options", "Export", "Export .db"]) {
        await expectHeld(tile.getByRole("button", { name, exact: true }), `tile ${name}`);
      }
    }

    await stepHeldClock(page, () => stripAction(page, "Add meter").isEnabled());
    await page.clock.resume();
    await expect(stripAction(page, "Add meter")).toBeEnabled({ timeout: 30_000 });
    for (const tile of await tiles(page).all()) {
      for (const name of ["Record", "Recorder options", "Export", "Export .db"]) {
        await expectFree(tile.getByRole("button", { name, exact: true }), `tile ${name}`);
      }
    }
  });

  test("Space starts no recording during the write (AC-70X5YY.2)", async ({ page }) => {
    await page.clock.install();
    await open(page);
    await streamDemo(page);
    await spyRecordingStarts(page);
    await holdWrite(page);
    await tab(page, "Meter").click();
    await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
    await page.keyboard.press("Space");
    await page.keyboard.press("Space");
    // The page clock is held, so give the promise chain a moment of real time.
    await page.waitForTimeout(300);
    expect(await recordingStarts(page), "Space sent no start during the write").toBe(0);
    await expect(topBar(page).getByRole("timer")).toHaveCount(0);

    await finishWrite(page, /verified/i);
    // Space records again once the write is over.
    await tab(page, "Meter").click();
    await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
    await page.keyboard.press("Space");
    await expect.poll(() => recordingStarts(page)).toBe(1);
    await expect(topBar(page).getByRole("timer")).toBeVisible();
  });

  test("PD analyzer, Trigger, Records and Firmware tabs are held; Meter and Startup logo are not (AC-70X5YY.3)", async ({
    page,
  }) => {
    await showFirmwareTab(page);
    await page.clock.install();
    await open(page);
    await streamDemo(page);
    for (const name of ["PD analyzer", "Trigger", "Records", "Firmware"]) {
      await expect(tab(page, name), `${name} is usable before`).not.toHaveAttribute(
        "aria-disabled",
        "true",
      );
    }

    await holdWrite(page);
    for (const name of ["PD analyzer", "Trigger", "Records", "Firmware"]) {
      await expect(tab(page, name), `${name} is held`).toHaveAttribute("aria-disabled", "true");
      // A held tab ignores a click (force: past Playwright's own enabled check).
      await tab(page, name).click({ force: true });
      await expect(page.getByRole("tabpanel", { name, exact: true })).toHaveCount(0);
    }
    for (const name of ["Meter", "Startup logo"]) {
      await expect(tab(page, name)).not.toHaveAttribute("aria-disabled", "true");
      await tab(page, name).click();
      await expect(page.getByRole("tabpanel", { name, exact: true })).toBeVisible();
    }

    await finishWrite(page, /verified/i);
    for (const name of ["PD analyzer", "Trigger", "Records", "Firmware"]) {
      await expect(tab(page, name)).not.toHaveAttribute("aria-disabled", "true");
    }
  });

  test("About with its switches, the theme and the chart's view controls stay usable (AC-70X5YY.4)", async ({
    page,
  }) => {
    await page.clock.install();
    await open(page);
    await streamDemo(page);
    await holdWrite(page);
    await tab(page, "Meter").click();

    // The theme control.
    const theme = page.getByRole("radiogroup", { name: "Theme" });
    await theme.getByRole("radio", { name: /dark/i }).click();
    await expect(theme.getByRole("radio", { name: /dark/i })).toBeChecked();
    await theme.getByRole("radio", { name: /light/i }).click();
    await expect(theme.getByRole("radio", { name: /light/i })).toBeChecked();

    // About and its switches.
    await page.locator("#top-bar-about").click();
    const about = page.getByRole("dialog", { name: "About" });
    await expect(about).toBeVisible();
    for (const name of ["Advanced firmware", "Hide serial numbers", "Show (i) icons"]) {
      const sw = about.getByRole("switch", { name });
      await expect(sw, `${name} is usable`).toBeEnabled();
      const before = await sw.getAttribute("aria-checked");
      await sw.click();
      await expect(sw).not.toHaveAttribute("aria-checked", before ?? "");
      await sw.click();
      await expect(sw).toHaveAttribute("aria-checked", before ?? "");
    }
    await page.keyboard.press("Escape");
    await expect(about).toHaveCount(0);

    // The chart's own view controls.
    const zero = page.getByRole("button", { name: "Zero-based axes" });
    await expect(zero).toBeEnabled();
    const pressed = await zero.getAttribute("aria-pressed");
    await zero.click();
    await expect(zero).not.toHaveAttribute("aria-pressed", pressed ?? "");
    await expect(page.getByRole("button", { name: "Chart options" })).toBeEnabled();
    await page.getByRole("button", { name: "Chart options" }).click();
    await expect(page.getByRole("button", { name: "Chart options" })).toHaveAttribute(
      "aria-expanded",
      "true",
    );
    for (const b of await page
      .getByRole("group", { name: "Live window" })
      .getByRole("button")
      .all()) {
      await expect(b).toBeEnabled();
    }
    // The hold is real meanwhile: Record says so.
    await expectHeld(topBar(page).getByRole("button", { name: "Record", exact: true }), "Record");
  });

  test("a failed write frees every action again (AC-70X5YY.5)", async ({ page }) => {
    await page.clock.install();
    await open(page, "?logofault=40");
    await streamDemo(page);
    await holdWrite(page);
    await expectHeld(topBar(page).getByRole("button", { name: "Record", exact: true }), "Record");
    await finishWrite(page, /chunk 40/i);
    await expect(view(page).getByRole("alert")).toBeVisible();
    for (const [c, what] of topBarActions(page, false)) await expectFree(c, what);
    for (const name of ["PD analyzer", "Trigger", "Records"]) {
      await expect(tab(page, name)).not.toHaveAttribute("aria-disabled", "true");
    }
  });
});

test.describe("a logo write holds the actions in the overflow menu (FR 70X5YY)", () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize(MENU_WIDTH);
  });

  function more(page: Page): Locator {
    return page.getByRole("button", { name: /^More:/ });
  }

  function menuItem(page: Page, name: string): Locator {
    return page.locator("#top-bar-menu").getByRole("menuitem", { name, exact: true });
  }

  test("Export and Export .db in the menu are held, the menu and its theme are not (AC-70X5YY.1, AC-70X5YY.4, AC-70X5YY.5)", async ({
    page,
  }) => {
    await page.clock.install();
    await open(page);
    await streamDemo(page);
    await expect(topBar(page)).toHaveClass(/export-in-menu/);

    await holdWrite(page);
    // The menu itself stays usable.
    await expect(more(page)).toBeEnabled();
    await more(page).click();
    await expect(page.locator("#top-bar-menu")).toBeVisible();
    await expectHeld(menuItem(page, "Export"), "menu Export");
    await expectHeld(menuItem(page, "Export .db"), "menu Export .db");
    await expect(menuItem(page, "About")).toBeEnabled();
    const dark = page.locator("#top-bar-menu").getByRole("menuitemradio", { name: /dark/i });
    await expect(dark).toBeEnabled();
    await dark.click();
    await expect(page.locator("#top-bar-menu")).toHaveCount(0);
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");

    await tab(page, "Startup logo").click();
    await stepHeldClock(page, viewSays(page, /verified/i));
    await page.clock.resume();
    await expect(view(page)).toContainText(/verified/i, { timeout: 30_000 });
    await more(page).click();
    await expectFree(menuItem(page, "Export"), "menu Export");
    await expectFree(menuItem(page, "Export .db"), "menu Export .db");
  });
});
