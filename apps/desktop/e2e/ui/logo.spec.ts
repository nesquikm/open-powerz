/**
 * FR MEHQC0 (AC .1 to .8): the Startup logo tab previews a picture at its real pixel
 * size with its numbers, asks for the word typed before Upload or Remove, shows chunk and
 * read-back progress with no cancel while every meter control is disabled, keeps the
 * write going when the view is left, ends with verified or the failed step and chunk, and
 * shows the startup-screen duration with a 1/3/5 s fix when it is off.
 *
 * Mock URL options: `?logofault=<k>` fails the upload at chunk k, `?logofile=small`
 * chooses a 240×24 picture, `?mock=logo-other-model` connects a KM002C. Runs in both
 * projects (Chromium and WebKit); `open()` installs the wide font when E2E_WIDE_FONT=1.
 */

import { expect, test, type Locator, type Page } from "@playwright/test";
import { holdClock, open, stepHeldClock, streamDemo } from "./helpers";

const MOCK_HOOK = "__OPEN_POWERZ_MOCK__";
const NARROW = { width: 375, height: 812 };

function view(page: Page): Locator {
  return page.getByRole("tabpanel", { name: "Startup logo" });
}

/** A `stepHeldClock` condition: the logo view's status reads `text`. */
function statusSays(page: Page, text: RegExp): () => Promise<boolean> {
  return async () => text.test((await view(page).getByRole("status").allTextContents()).join(" "));
}

function confirmDialog(page: Page, word: RegExp): Locator {
  return page.getByRole("alertdialog", { name: word });
}

/** The meter's settings rows as the mock holds them. */
function mockStartup(page: Page): Promise<string | undefined> {
  return page.evaluate(async (key) => {
    const mock = (
      window as unknown as Record<
        string,
        { meterSettings(): Promise<{ fields: { key: string; value: string }[] }> }
      >
    )[key];
    return (await mock.meterSettings()).fields.find((f) => f.key === "startup_logo")?.value;
  }, MOCK_HOOK);
}

async function openLogo(page: Page, query = ""): Promise<Locator> {
  await open(page, query);
  await streamDemo(page);
  await page.getByRole("tab", { name: "Startup logo" }).click();
  const v = view(page);
  await expect(v).toBeVisible();
  return v;
}

async function choose(v: Locator): Promise<void> {
  await v.getByRole("button", { name: /choose/i }).click();
  await expect(v.getByTestId("logo-preview")).toBeVisible();
}

async function digits(l: Locator): Promise<string> {
  return ((await l.textContent()) ?? "").replace(/\D/g, "");
}

async function confirmUpload(page: Page, v: Locator): Promise<void> {
  await v.getByRole("button", { name: /^upload$/i }).click();
  const d = confirmDialog(page, /upload/i);
  await d.getByRole("textbox").fill("upload");
  await d.getByRole("button", { name: /^upload$/i }).click();
  await expect(d).toHaveCount(0);
}

test.describe("startup logo view (FR MEHQC0)", () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 1600, height: 900 });
  });

  test("preview at the real pixel size with size, blob and chunks (AC-MEHQC0.1)", async ({
    page,
  }) => {
    const v = await openLogo(page);
    await expect(v.getByRole("radio", { name: /opaque/i })).toBeChecked();
    await expect(v.getByRole("radio", { name: /transparent/i })).not.toBeChecked();
    await choose(v);
    const preview = v.getByTestId("logo-preview");
    await expect(preview).toHaveAttribute("width", "100");
    await expect(preview).toHaveAttribute("height", "60");
    const box = await preview.boundingBox();
    expect(box, "the preview is laid out").toBeTruthy();
    expect(Math.round(box!.width)).toBe(100);
    expect(Math.round(box!.height)).toBe(60);
    expect(await digits(v.getByTestId("logo-size"))).toBe("10060");
    expect(await digits(v.getByTestId("logo-blob-size"))).toBe("12016");
    expect(await digits(v.getByTestId("logo-chunks"))).toBe("47");
    await expect(v.getByTestId("logo-small-warning")).toHaveCount(0);

    await v.getByRole("radio", { name: /transparent/i }).check();
    await expect(v.getByTestId("logo-preview")).toHaveAttribute("width", "100");
    await expect.poll(() => digits(v.getByTestId("logo-blob-size"))).toBe("18016");
    expect(await digits(v.getByTestId("logo-chunks"))).toBe("71");
  });

  test("warns when a side is below 48 (AC-MEHQC0.1)", async ({ page }) => {
    const v = await openLogo(page, "?logofile=small");
    await choose(v);
    await expect(v.getByTestId("logo-preview")).toHaveAttribute("height", "24");
    await expect(v.getByTestId("logo-small-warning")).toBeVisible();
    await expect(v.getByTestId("logo-small-warning")).toContainText("48");
  });

  test("Upload needs the word typed; cancel and Escape send nothing (AC-MEHQC0.2)", async ({
    page,
  }) => {
    const v = await openLogo(page);
    await choose(v);
    await v.getByRole("button", { name: /^upload$/i }).click();
    const d = confirmDialog(page, /upload/i);
    await expect(d).toBeVisible();
    await expect(d).toContainText("upload");
    const go = d.getByRole("button", { name: /^upload$/i });
    await expect(go).toBeDisabled();
    await d.getByRole("textbox").fill("Upload");
    await expect(go).toBeDisabled();
    await d.getByRole("textbox").fill("remove");
    await expect(go).toBeDisabled();
    await d.getByRole("button", { name: /cancel/i }).click();
    await expect(d).toHaveCount(0);
    await v.getByRole("button", { name: /^upload$/i }).click();
    await expect(d).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(d).toHaveCount(0);
    await expect(v.getByRole("progressbar")).toHaveCount(0);
    await v.getByRole("button", { name: /^upload$/i }).click();
    await d.getByRole("textbox").fill("upload");
    await expect(go).toBeEnabled();
  });

  test("Remove needs its own word typed (AC-MEHQC0.2)", async ({ page }) => {
    const v = await openLogo(page);
    await v.getByRole("button", { name: /^remove$/i }).click();
    const d = confirmDialog(page, /remove/i);
    await expect(d).toBeVisible();
    const go = d.getByRole("button", { name: /^remove$/i });
    await expect(go).toBeDisabled();
    await d.getByRole("textbox").fill("upload");
    await expect(go).toBeDisabled();
    await d.getByRole("textbox").fill("remove");
    await expect(go).toBeEnabled();
    await go.click();
    await expect(d).toHaveCount(0);
    await expect(v.getByRole("status")).toContainText(/verified|removed/i);
  });

  test("progress with no cancel, meter controls disabled, survives leaving the view (AC-MEHQC0.3, .4)", async ({
    page,
  }) => {
    const v = await openLogo(page);
    await choose(v);
    await confirmUpload(page, v);
    const bar = v.getByRole("progressbar");
    await expect(bar).toBeVisible();
    await expect(v).toContainText(/chunk/i);
    await expect(v.getByRole("button", { name: /cancel|stop/i })).toHaveCount(0);
    for (const name of [/^upload$/i, /^remove$/i, /^read$/i, /choose/i]) {
      await expect(v.getByRole("button", { name })).toBeDisabled();
    }
    await expect(v.getByRole("radio", { name: /transparent/i })).toBeDisabled();

    // Leave the view mid-write and come back: the write went on.
    await page.getByRole("tab", { name: "Meter", exact: true }).click();
    await expect(view(page)).toHaveCount(0);
    await page.getByRole("tab", { name: "Startup logo" }).click();
    await expect(view(page)).toBeVisible();
    await expect(view(page)).toContainText(/read.?back/i, { timeout: 30_000 });
    await expect(view(page).getByRole("status")).toContainText(/verified/i, { timeout: 30_000 });
    await expect(view(page).getByRole("button", { name: /^upload$/i })).toBeEnabled();
  });

  test("a failure at chunk k names the step and chunk with advice (AC-MEHQC0.4)", async ({
    page,
  }) => {
    const v = await openLogo(page, "?logofault=7");
    await choose(v);
    await confirmUpload(page, v);
    const alert = v.getByRole("alert");
    await expect(alert).toContainText("chunk 7", { timeout: 30_000 });
    await expect(alert).toContainText(/upload/i);
    await expect(alert).toContainText(/remove/i);
    await expect(alert).toContainText(/again/i);
    await expect(v.getByRole("button", { name: /^upload$/i })).toBeEnabled();
    await expect(v.getByRole("button", { name: /^remove$/i })).toBeEnabled();
  });

  test("the duration hint offers 1, 3 or 5 s through the settings write (AC-MEHQC0.5)", async ({
    page,
  }) => {
    const v = await openLogo(page);
    const hint = v.getByTestId("logo-duration-hint");
    await expect(hint).toBeVisible();
    await expect(hint).toContainText(/off/i);
    await expect(hint).toContainText(
      "The startup screen is off. Upload turns it on at 3 s, or show it now for:",
    );
    for (const s of ["1 s", "3 s", "5 s"]) {
      await expect(hint.getByRole("button", { name: s })).toBeEnabled();
    }
    await hint.getByRole("button", { name: "3 s" }).click();
    await expect(hint).toHaveCount(0);
    await expect(v.getByTestId("logo-duration")).toContainText("3 s");
    expect(await mockStartup(page)).toBe("3 s");
  });

  test("another model disables Upload and Remove with a reason (AC-MEHQC0.6)", async ({ page }) => {
    const v = await openLogo(page, "?mock=logo-other-model");
    await choose(v);
    await expect(v.getByRole("button", { name: /^upload$/i })).toBeDisabled();
    await expect(v.getByRole("button", { name: /^remove$/i })).toBeDisabled();
    await expect(v.getByTestId("logo-disabled-reason")).toContainText("KM003C");
  });

  test("Read shows what the meter holds and can show the stored logo (AC-MEHQC0.7)", async ({
    page,
  }) => {
    const v = await openLogo(page);
    await v.getByRole("button", { name: /^read$/i }).click();
    await expect(v.getByTestId("logo-area")).toContainText(/empty/i);
    await choose(v);
    await confirmUpload(page, v);
    await expect(v.getByRole("status")).toContainText(/verified/i, { timeout: 30_000 });
    await v.getByRole("button", { name: /^read$/i }).click();
    await expect(v.getByTestId("logo-area")).toContainText(/100\s*×\s*60/);
    await v.getByRole("button", { name: /show (the )?(stored )?logo/i }).click();
    const stored = v.getByTestId("logo-stored");
    await expect(stored).toBeVisible();
    await expect(stored).toHaveAttribute("width", "100");
  });
});

test.describe("startup logo at 375 px wide (AC-MEHQC0.8)", () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize(NARROW);
  });

  async function expectNoSidewaysScroll(page: Page): Promise<void> {
    const scroll = await page.evaluate(() => ({
      documentWidth: document.documentElement.scrollWidth,
      windowWidth: document.documentElement.clientWidth,
    }));
    expect(scroll.documentWidth, "no horizontal scroll").toBeLessThanOrEqual(scroll.windowWidth);
  }

  async function expectInside(l: Locator): Promise<void> {
    await l.scrollIntoViewIfNeeded();
    const box = await l.boundingBox();
    expect(box, "laid out").toBeTruthy();
    expect(box!.x).toBeGreaterThanOrEqual(0);
    expect(box!.x + box!.width).toBeLessThanOrEqual(NARROW.width);
  }

  test("the view, the preview, the hint and the dialog fit", async ({ page }) => {
    const v = await openLogo(page, "?logofile=small");
    await choose(v);
    await expectNoSidewaysScroll(page);
    await expectInside(v.getByTestId("logo-preview"));
    await expectInside(v.getByTestId("logo-duration-hint"));
    for (const name of [/^upload$/i, /^remove$/i, /^read$/i]) {
      await expectInside(v.getByRole("button", { name }));
    }
    await v.getByRole("button", { name: /^upload$/i }).click();
    const d = confirmDialog(page, /upload/i);
    await expect(d).toBeVisible();
    await expectInside(d);
    await expectInside(d.getByRole("button", { name: /^upload$/i }));
    await d.getByRole("textbox").fill("upload");
    await d.getByRole("button", { name: /^upload$/i }).click();
    await expect(v.getByRole("progressbar")).toBeVisible();
    await expectNoSidewaysScroll(page);
    await expect(v.getByRole("status")).toContainText(/verified/i, { timeout: 30_000 });
  });
});

/** The full view's control bar: rate and Disconnect of the meter it shows. */
function meterBar(page: Page): Locator {
  return page.getByRole("group", { name: /^Controls for / });
}

/** The controls outside the Startup logo view that send to the meter, per tab. */
const SENDERS: { tab: string; controls: (page: Page) => Locator }[] = [
  {
    tab: "Meter",
    controls: (page) =>
      page.getByRole("region", { name: "Meter settings" }).getByRole("button", { name: "Apply" }),
  },
  {
    tab: "Records",
    controls: (page) =>
      page
        .getByRole("region", { name: "Offline records" })
        .getByRole("button", { name: /^delete( all)?$/i }),
  },
  {
    tab: "Trigger",
    controls: (page) =>
      page.getByRole("toolbar", { name: "Trigger" }).getByRole("button", { name: "Detect" }),
  },
];

async function gotoTab(page: Page, tab: string): Promise<boolean> {
  const t = page.getByRole("tab", { name: tab, exact: true });
  // A tab the write holds (as a firmware update does) cannot send either.
  if ((await t.getAttribute("aria-disabled")) === "true") return false;
  await t.click();
  await expect(page.getByRole("tabpanel", { name: tab, exact: true })).toBeVisible();
  return true;
}

async function expectSenders(page: Page, enabled: boolean): Promise<void> {
  const bar = meterBar(page);
  const rate = bar.getByLabel("Sample rate");
  const disconnect = bar.getByRole("button", { name: "Disconnect", exact: true });
  for (const c of [rate, disconnect]) {
    await expect(c).toBeVisible();
    if (enabled) await expect(c).toBeEnabled();
    else await expect(c, "the top bar's meter control is disabled during the write").toBeDisabled();
  }
  for (const { tab, controls } of SENDERS) {
    if (!(await gotoTab(page, tab))) {
      expect(enabled, `${tab} tab is held only during a write`).toBe(false);
      continue;
    }
    const all = controls(page);
    await expect(all.first()).toBeVisible();
    for (const c of await all.all()) {
      if (enabled) await expect(c).toBeEnabled();
      else
        await expect(c, `${tab}: a control that sends is disabled during the write`).toBeDisabled();
    }
  }
}

/** Every logo upload the mock got, with the meter it went to. */
async function spyUploads(page: Page): Promise<void> {
  await page.evaluate((key) => {
    const w = window as unknown as Record<string, unknown>;
    const mock = w[key] as {
      logoUpload: (id: string, ...rest: unknown[]) => Promise<unknown>;
    };
    const calls: { id: string; done: boolean }[] = [];
    w.__LOGO_UPLOADS__ = calls;
    const real = mock.logoUpload.bind(mock);
    mock.logoUpload = async (id: string, ...rest: unknown[]) => {
      const call = { id, done: false };
      calls.push(call);
      const out = await real(id, ...rest);
      call.done = true;
      return out;
    };
  }, MOCK_HOOK);
}

function uploads(page: Page): Promise<{ id: string; done: boolean }[]> {
  return page.evaluate(
    () =>
      (window as unknown as { __LOGO_UPLOADS__: { id: string; done: boolean }[] }).__LOGO_UPLOADS__,
  );
}

test.describe("a logo write holds every meter control (AC-MEHQC0.3)", () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 1600, height: 900 });
  });

  test("settings Apply, record Delete, trigger Detect, rate and Disconnect are disabled during the write (AC-MEHQC0.3)", async ({
    page,
  }) => {
    // The page clock runs as usual until the write is under way, then holds it there.
    await page.clock.install();
    await open(page);
    await streamDemo(page);
    // A pending settings edit, so Apply would send.
    const settings = page.getByRole("region", { name: "Meter settings" });
    await expect(settings).toContainText("50%");
    await settings.getByRole("button", { name: "Edit" }).click();
    await settings.getByLabel("Brightness", { exact: true }).fill("80");
    // The records table is read before the write, so its Delete buttons are there.
    await gotoTab(page, "Records");
    await expect(
      page
        .getByRole("region", { name: "Offline records" })
        .getByRole("button", { name: /^delete$/i })
        .first(),
    ).toBeVisible();
    await expectSenders(page, true);

    await page.getByRole("tab", { name: "Startup logo" }).click();
    const v = view(page);
    await choose(v);
    await confirmUpload(page, v);
    await expect(v.getByRole("progressbar")).toBeVisible();
    await holdClock(page);

    await expectSenders(page, false);

    await page.getByRole("tab", { name: "Startup logo" }).click();
    await expect(view(page).getByRole("progressbar")).toBeVisible();
    await stepHeldClock(page, statusSays(page, /verified/i));
    await page.clock.resume();
    await expect(view(page).getByRole("status")).toContainText(/verified/i, { timeout: 30_000 });

    await expectSenders(page, true);
  });

  test("switching the full-view meter during the write finishes it verified on the first meter (AC-MEHQC0.3)", async ({
    page,
  }) => {
    await page.clock.install();
    await open(page);
    await streamDemo(page);
    await page.getByRole("button", { name: "Add meter" }).click();
    const dialog = page.getByRole("dialog", { name: "Add a meter" });
    await dialog.getByLabel("Device", { exact: true }).selectOption("demo:pd-9v-2a");
    await dialog.getByRole("button", { name: "Connect" }).click();
    await expect(dialog).toBeHidden();
    const grid = page.getByRole("region", { name: "Meters" });
    await expect(grid.getByRole("article")).toHaveCount(2);
    await grid
      .getByRole("article")
      .filter({ hasText: "load-5v-1a" })
      .getByRole("button", { name: "Expand" })
      .click();
    await spyUploads(page);

    await page.getByRole("tab", { name: "Startup logo" }).click();
    const v = view(page);
    await choose(v);
    await confirmUpload(page, v);
    await expect(v.getByRole("progressbar")).toBeVisible();
    await holdClock(page);

    // Leave the view through the meter list and put the other meter in the full view.
    await page.getByRole("tab", { name: "Meter", exact: true }).click();
    await page.getByRole("button", { name: "All meters" }).click();
    await grid
      .getByRole("article")
      .filter({ hasText: "pd-9v-2a" })
      .getByRole("button", { name: "Expand" })
      .click();
    await page.getByRole("tab", { name: "Startup logo" }).click();
    await expect(view(page).getByRole("progressbar")).toBeVisible();
    await stepHeldClock(page, statusSays(page, /verified/i));
    await page.clock.resume();
    await expect(view(page).getByRole("status")).toContainText(/verified/i, { timeout: 30_000 });

    expect(await uploads(page)).toEqual([{ id: "demo:load-5v-1a", done: true }]);
    // While the write ran the other meter's controls sent nothing either; now they can.
    await expect(
      meterBar(page).getByRole("button", { name: "Disconnect", exact: true }),
    ).toBeEnabled();
  });
});

/** Every settings check the mock got, with its page time. */
async function spyChecks(page: Page): Promise<void> {
  await page.evaluate((key) => {
    const w = window as unknown as Record<string, unknown>;
    const mock = w[key] as { logoSettingsCheck: (id: string) => Promise<unknown> };
    const at: number[] = [];
    w.__LOGO_CHECKS__ = at;
    const real = mock.logoSettingsCheck.bind(mock);
    mock.logoSettingsCheck = async (id: string) => {
      at.push(Date.now());
      return real(id);
    };
  }, MOCK_HOOK);
}

function checks(page: Page): Promise<number[]> {
  return page.evaluate(() => (window as unknown as { __LOGO_CHECKS__: number[] }).__LOGO_CHECKS__);
}

function settingsReads(page: Page): Promise<number> {
  return page.evaluate(
    (key) =>
      (window as unknown as Record<string, { meterSettingsReads(): number }>)[
        key
      ].meterSettingsReads(),
    MOCK_HOOK,
  );
}

test.describe("after a logo write, the menu save (FR QG6RNY)", () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 1600, height: 900 });
  });

  test("qg6rny: the off hint says Upload turns the startup screen on at 3 s (AC-QG6RNY.7)", async ({
    page,
  }) => {
    const v = await openLogo(page);
    const hint = v.getByTestId("logo-duration-hint");
    await expect(hint).toContainText(
      "The startup screen is off. Upload turns it on at 3 s, or show it now for:",
    );
    await expect(hint).not.toContainText(/will not show/i);
    for (const s of ["1 s", "3 s", "5 s"]) {
      await expect(hint.getByRole("button", { name: s })).toBeEnabled();
    }
  });

  test("qg6rny: the confirmations say what happens to the startup duration (AC-QG6RNY.1)", async ({
    page,
  }) => {
    const v = await openLogo(page);
    await choose(v);
    await v.getByRole("button", { name: /^upload$/i }).click();
    const up = confirmDialog(page, /upload/i);
    await expect(up.getByTestId("logo-confirm-duration")).toHaveText(
      "The startup duration goes from off to 3 s.",
    );
    await up.getByRole("button", { name: /cancel/i }).click();
    await v.getByRole("button", { name: /^remove$/i }).click();
    const rm = confirmDialog(page, /remove/i);
    await expect(rm.getByTestId("logo-confirm-duration")).toHaveText(
      "The startup screen is turned off (startup duration off).",
    );
  });

  test("qg6rny: the panel lists the steps, checks 2 s apart and ends saved (AC-QG6RNY.2, .3, .8)", async ({
    page,
  }) => {
    const v = await openLogo(page);
    await spyChecks(page);
    await choose(v);
    await confirmUpload(page, v);
    const panel = v.getByTestId("logo-menu-save");
    await expect(panel).toBeVisible({ timeout: 30_000 });
    await expect(panel).toHaveAttribute("role", "status");
    await expect(panel).toHaveAttribute("aria-live", "polite");
    await expect(
      panel.getByRole("heading", { name: "Save the settings on the meter" }),
    ).toBeVisible();
    await expect(panel.locator("ol > li")).toHaveText([
      "On the meter, open Settings.",
      "Choose the startup logo duration.",
      "Pick a value and save it (Disable keeps the startup screen off).",
    ]);
    await expect(panel.getByTestId("logo-menu-save-unplug")).toHaveText(
      "Don't unplug the meter until the app says the settings are saved.",
    );
    await expect(panel.getByTestId("logo-menu-save-status")).toContainText(
      /Checking the settings… not saved yet \(check \d+\)/,
    );
    // No other logo write while it checks.
    await expect(v.getByRole("button", { name: /^upload$/i })).toBeDisabled();
    await expect(v.getByRole("button", { name: /^remove$/i })).toBeDisabled();
    const readsBefore = await settingsReads(page);

    // The mock saves by itself a few seconds after the write.
    const done = v.getByTestId("logo-menu-save-done");
    await expect(done).toHaveText("The meter saved its settings. Startup screen: 3 s.", {
      timeout: 15_000,
    });
    await expect(panel).toHaveCount(0);
    await expect(v.getByTestId("logo-duration")).toContainText("3 s");
    // The meter's settings are read again.
    await expect.poll(() => settingsReads(page)).toBeGreaterThan(readsBefore);
    const at = await checks(page);
    expect(at.length).toBeGreaterThanOrEqual(1);
    for (let i = 1; i < at.length; i++) expect(at[i]! - at[i - 1]!).toBeGreaterThanOrEqual(1_990);
    await expect(v.getByRole("button", { name: /^upload$/i })).toBeEnabled();
  });

  test("qg6rny: changed calibration ends the check as an error (AC-QG6RNY.3)", async ({ page }) => {
    const v = await openLogo(page, "?menusave=calibration");
    await spyChecks(page);
    await choose(v);
    await confirmUpload(page, v);
    const alert = v.getByRole("alert");
    await expect(alert).toContainText(/calibration/i, { timeout: 20_000 });
    await expect(v.getByTestId("logo-menu-save")).toHaveCount(0);
    const n = (await checks(page)).length;
    await page.waitForTimeout(4_500);
    expect((await checks(page)).length, "no more checks").toBe(n);
  });

  test("qg6rny: a fresh read already saved shows no panel (AC-QG6RNY.4)", async ({ page }) => {
    const v = await openLogo(page, "?menusave=saved");
    await spyChecks(page);
    await choose(v);
    await confirmUpload(page, v);
    await expect(v.getByTestId("logo-duration")).toContainText("3 s", { timeout: 30_000 });
    await expect(v.getByRole("button", { name: /^upload$/i })).toBeEnabled();
    await expect(v.getByTestId("logo-menu-save")).toHaveCount(0);
    expect(await checks(page)).toEqual([]);
  });

  test("qg6rny: leaving the view stops the check and coming back starts it (AC-QG6RNY.5)", async ({
    page,
  }) => {
    const v = await openLogo(page, "?menusave=never");
    await spyChecks(page);
    await choose(v);
    await confirmUpload(page, v);
    await expect(v.getByTestId("logo-menu-save")).toBeVisible({ timeout: 30_000 });
    await expect
      .poll(() => checks(page).then((a) => a.length), { timeout: 10_000 })
      .toBeGreaterThan(0);
    await page.getByRole("tab", { name: "Meter", exact: true }).click();
    await expect(view(page)).toHaveCount(0);
    const n = (await checks(page)).length;
    await page.waitForTimeout(4_500);
    expect((await checks(page)).length, "no check while the view is closed").toBeLessThanOrEqual(
      n + 1,
    );
    const m = (await checks(page)).length;
    await page.waitForTimeout(4_500);
    expect((await checks(page)).length, "stopped").toBe(m);
    await page.getByRole("tab", { name: "Startup logo" }).click();
    await expect(view(page).getByTestId("logo-menu-save")).toBeVisible();
    await expect
      .poll(() => checks(page).then((a) => a.length), { timeout: 10_000 })
      .toBeGreaterThan(m);
  });

  test("qg6rny: disconnecting the meter stops the check (AC-QG6RNY.5)", async ({ page }) => {
    const v = await openLogo(page, "?menusave=never");
    await spyChecks(page);
    await choose(v);
    await confirmUpload(page, v);
    await expect(v.getByTestId("logo-menu-save")).toBeVisible({ timeout: 30_000 });
    await expect
      .poll(() => checks(page).then((a) => a.length), { timeout: 10_000 })
      .toBeGreaterThan(0);
    await page
      .getByRole("group", { name: /^Controls for / })
      .getByRole("button", { name: "Disconnect", exact: true })
      .click();
    await expect(page.getByRole("tab", { name: "Startup logo" })).toHaveCount(0);
    const n = (await checks(page)).length;
    await page.waitForTimeout(4_500);
    expect((await checks(page)).length, "no check after the disconnect").toBe(n);
  });
});

/**
 * FR 70X5YX: the built-in open-powerz logos. The view offers "Use the open-powerz logo"
 * as a group of two buttons, "Mascot" and "Mascot with lettering"; picking one previews
 * it (200 × 200, transparent, 469 chunks), locks the mode to Transparent with the reason
 * in `logo-mode-reason`, and Upload writes it on the demo meter.
 */
test.describe("built-in open-powerz logos (FR 70X5YX)", () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 1600, height: 900 });
  });

  function builtins(v: Locator): Locator {
    return v.getByRole("group", { name: /open-powerz logo/i });
  }

  async function pick(v: Locator, name: RegExp): Promise<void> {
    await builtins(v).getByRole("button", { name }).click();
    await expect(v.getByTestId("logo-preview")).toBeVisible();
  }

  test("AC-70X5YX.5: offers the mascot and the mascot with lettering, each previewed at 200 × 200", async ({
    page,
  }) => {
    const v = await openLogo(page);
    const group = builtins(v);
    await expect(group).toBeVisible();
    await expect(group.getByRole("button", { name: /^mascot$/i })).toBeEnabled();
    await expect(group.getByRole("button", { name: /mascot with lettering/i })).toBeEnabled();

    await pick(v, /^mascot$/i);
    const preview = v.getByTestId("logo-preview");
    await expect(preview).toHaveAttribute("width", "200");
    await expect(preview).toHaveAttribute("height", "200");
    const box = await preview.boundingBox();
    expect(box, "the preview is laid out").toBeTruthy();
    expect(Math.round(box!.width)).toBe(200);
    expect(Math.round(box!.height)).toBe(200);
    expect(await digits(v.getByTestId("logo-size"))).toBe("200200");
    expect(await digits(v.getByTestId("logo-blob-size"))).toBe("120016");
    expect(await digits(v.getByTestId("logo-chunks"))).toBe("469");
    const mascot = await preview.evaluate((c) => (c as HTMLCanvasElement).toDataURL());

    await pick(v, /mascot with lettering/i);
    await expect
      .poll(() => preview.evaluate((c) => (c as HTMLCanvasElement).toDataURL()))
      .not.toBe(mascot);
    await expect(preview).toHaveAttribute("width", "200");
    expect(await digits(v.getByTestId("logo-chunks"))).toBe("469");
  });

  test("AC-70X5YX.6: a built-in locks the mode to transparent; a file brings opaque back", async ({
    page,
  }) => {
    const v = await openLogo(page);
    const opaque = v.getByRole("radio", { name: /opaque/i });
    const transparent = v.getByRole("radio", { name: /transparent/i });
    await expect(opaque).toBeEnabled();
    await expect(v.getByTestId("logo-mode-reason")).toHaveCount(0);

    await pick(v, /^mascot$/i);
    await expect(transparent).toBeChecked();
    await expect(opaque).toBeDisabled();
    const reason = v.getByTestId("logo-mode-reason");
    await expect(reason).toBeVisible();
    await expect(reason).toContainText(/transparent/i);

    await choose(v);
    await expect(opaque).toBeEnabled();
    await expect(transparent).toBeEnabled();
    await expect(v.getByTestId("logo-mode-reason")).toHaveCount(0);
    await opaque.check();
    await expect(v.getByTestId("logo-preview")).toHaveAttribute("width", "100");
    await expect.poll(() => digits(v.getByTestId("logo-blob-size"))).toBe("12016");
  });

  test("AC-70X5YX.5, AC-70X5YX.7: Upload writes a built-in on the demo meter with confirmation, progress, read-back and the settings guidance", async ({
    page,
  }) => {
    const v = await openLogo(page);
    await pick(v, /mascot with lettering/i);
    await v.getByRole("button", { name: /^upload$/i }).click();
    const d = confirmDialog(page, /upload/i);
    await expect(d).toBeVisible();
    await expect(d.getByTestId("logo-confirm-duration")).toHaveText(
      "The startup duration goes from off to 3 s.",
    );
    await d.getByRole("textbox").fill("upload");
    await d.getByRole("button", { name: /^upload$/i }).click();
    await expect(d).toHaveCount(0);

    await expect(v.getByRole("progressbar")).toBeVisible();
    await expect(v).toContainText(/chunk \d+ of 469/i);
    await expect(v).toContainText(/read.?back/i, { timeout: 30_000 });
    await expect(v.getByRole("status")).toContainText(/verified/i, { timeout: 30_000 });
    await expect(v.getByTestId("logo-menu-save")).toBeVisible({ timeout: 30_000 });

    await v.getByRole("button", { name: /^read$/i }).click();
    await expect(v.getByTestId("logo-area")).toContainText(/200\s*×\s*200/);
    await expect(v.getByTestId("logo-area")).toContainText(/transparent/i);
  });

  test("AC-70X5YX.5: the built-in buttons are disabled during a write", async ({ page }) => {
    const v = await openLogo(page);
    await pick(v, /^mascot$/i);
    await confirmUpload(page, v);
    await expect(v.getByRole("progressbar")).toBeVisible();
    await expect(builtins(v).getByRole("button", { name: /^mascot$/i })).toBeDisabled();
    await expect(
      builtins(v).getByRole("button", { name: /mascot with lettering/i }),
    ).toBeDisabled();
    await expect(v.getByRole("status")).toContainText(/verified/i, { timeout: 30_000 });
  });

  test("AC-70X5YX.5: the picker fits at 375 px wide", async ({ page }) => {
    await page.setViewportSize(NARROW);
    const v = await openLogo(page);
    await pick(v, /mascot with lettering/i);
    const scroll = await page.evaluate(() => ({
      documentWidth: document.documentElement.scrollWidth,
      windowWidth: document.documentElement.clientWidth,
    }));
    expect(scroll.documentWidth, "no horizontal scroll").toBeLessThanOrEqual(scroll.windowWidth);
    for (const name of [/^mascot$/i, /mascot with lettering/i]) {
      const b = builtins(v).getByRole("button", { name });
      await b.scrollIntoViewIfNeeded();
      const box = await b.boundingBox();
      expect(box, "laid out").toBeTruthy();
      expect(box!.x).toBeGreaterThanOrEqual(0);
      expect(box!.x + box!.width).toBeLessThanOrEqual(NARROW.width);
    }
  });
});
