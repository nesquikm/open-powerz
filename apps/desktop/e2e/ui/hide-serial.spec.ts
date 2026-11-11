/**
 * FR AXHK2Q on the mocked backend: the "Hide serial numbers" switch in the About panel.
 *
 * - AC .1: off by default, remembered across a reload;
 * - AC .2: with it on, no serial appears in the page's text, attributes or labels. The
 *   scan below covers the markup (text and every attribute), the live form values and
 *   the document title; a control test with the switch off proves that it finds them;
 * - AC .3: hidden fields read "***" and labels name the model alone (FR CCF47K), and the
 *   switch restores them with no reconnect;
 * - AC .4: tiles are titled by model and position;
 * - AC .5: recording and export names carry no serial, on or off.
 *
 * `?mock=replug` lists one real meter (`MOCK_REPLUG.serial`) beside the demo devices; it
 * drops out 3 s after connecting and comes back 3 s later, then stays.
 *
 * `?mock=two-meters` (FR CCF47K, AC .2, .5) lists two real KM003C meters beside the demo
 * devices: `MOCK_TWO_METERS.first` (e.g. "MOCK-PAIR-0002") at USB address 5 and `.second`
 * (e.g. "MOCK-PAIR-0001") at address 6. The one at address 6 has the lower serial, so it
 * reads "KM003C (1)".
 */

import { expect, test, type Locator, type Page } from "@playwright/test";
import { MOCK_REPLUG, MOCK_TWO_METERS } from "../../src/lib/mockBackend";
import { badge, open, streamDemo } from "./helpers";

const HIDE_KEY = "open-powerz.hide-serial";
const ADVANCED_KEY = "open-powerz.advanced-firmware";
const MOCK_HOOK = "__OPEN_POWERZ_MOCK__";
const DEMO_PD = "demo:pd-9v-2a";
const SWITCH = "Hide serial numbers";
/** The real meter's picker ids, by id: with the option on, its text holds no serial. */
const FIRST_ADDRESS = "usb:1:7";
const AFTER_REPLUG = "usb:1:9";

test.use({ viewport: { width: 1600, height: 900 } });

/** The demo serials (a second demo meter counts up), whole and as a tile's suffix. */
const DEMO_SERIALS = [/(?<![\d.])12345[67](?!\d)/, /…345[67]/];
const REAL_SERIALS = [MOCK_REPLUG.serial, MOCK_TWO_METERS.first, MOCK_TWO_METERS.second].flatMap(
  (serial) => [serial, `…${serial.slice(-4)}`],
);
/** The two-meter mock's ids: address 6 holds the lower serial (AC-CCF47K.2). */
const PAIR_HIGHER = "usb:1:5";
const PAIR_LOWER = "usb:1:6";

async function hideBeforeLoad(page: Page): Promise<void> {
  await page.addInitScript((key) => localStorage.setItem(key, "true"), HIDE_KEY);
}

async function useAdvancedFirmware(page: Page): Promise<void> {
  await page.addInitScript((key) => localStorage.setItem(key, "true"), ADVANCED_KEY);
}

/** Everything the page exposes: markup with every attribute, live values, the title. */
function pageDump(page: Page): Promise<string> {
  return page.evaluate(() => {
    const parts = [document.title, document.documentElement.outerHTML];
    for (const el of document.querySelectorAll("*")) {
      const value = (el as HTMLInputElement).value;
      if (typeof value === "string" && value !== "") parts.push(value);
    }
    return parts.join("\n");
  });
}

async function serialsOnPage(page: Page): Promise<string[]> {
  const dump = await pageDump(page);
  const found: string[] = [];
  for (const needle of [...DEMO_SERIALS, ...REAL_SERIALS]) {
    const hit = typeof needle === "string" ? dump.includes(needle) : needle.test(dump);
    if (hit) found.push(String(needle));
  }
  return found;
}

async function expectNoSerial(page: Page): Promise<void> {
  expect(await serialsOnPage(page)).toEqual([]);
}

async function openAbout(page: Page): Promise<Locator> {
  await page.locator("#top-bar-about").click();
  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  return dialog;
}

function hideSwitch(dialog: Locator): Locator {
  return dialog.getByRole("switch", { name: SWITCH });
}

function tiles(page: Page): Locator {
  return page.getByRole("region", { name: "Meters" }).getByRole("article");
}

function connectedIds(page: Page): Promise<string[]> {
  return page.evaluate(
    (key) =>
      (window as unknown as Record<string, { connectedIds(): string[] }>)[key].connectedIds(),
    MOCK_HOOK,
  );
}

/** Add the second demo meter beside the one streaming. */
async function addDemoMeter(page: Page): Promise<void> {
  await page.getByRole("button", { name: "Add meter" }).click();
  const dialog = page.getByRole("dialog", { name: "Add a meter" });
  await dialog.getByLabel("Device", { exact: true }).selectOption(DEMO_PD);
  await dialog.getByRole("button", { name: "Connect" }).click();
  await expect(tiles(page)).toHaveCount(2);
}

/** Up to the prompt naming the session the update would end, as FR AXHK2F's spec. */
async function firmwareSessionPrompt(page: Page): Promise<Locator> {
  await page.getByRole("tab", { name: "Firmware" }).click();
  const panel = page.locator("#view-firmware");
  await panel.getByRole("button", { name: /choose file/i }).click();
  await expect(panel.getByRole("region", { name: "Firmware file" })).toBeVisible();
  await panel.getByRole("button", { name: "Update", exact: true }).click();
  const confirm = page.getByRole("dialog", { name: "Confirm firmware update" });
  await confirm.getByRole("textbox", { name: /version/i }).fill("2.0.7");
  await confirm.getByRole("button", { name: "Start update" }).click();
  const prompt = page.getByRole("dialog", { name: /session|recording/i });
  await expect(prompt).toBeVisible();
  return prompt;
}

async function connectRealMeter(page: Page, id = FIRST_ADDRESS): Promise<void> {
  await page.getByLabel("Device", { exact: true }).selectOption(id);
  await page.getByRole("button", { name: "Connect" }).click();
  await expect(badge(page)).toHaveText("Streaming");
}

/** Connect the real meter, wait through its one drop-out, and connect it afresh at its new
 * address: a firmware update needs a meter that stays put. */
async function streamRealMeterAfterReplug(page: Page): Promise<void> {
  await connectRealMeter(page);
  await expect(badge(page)).toHaveText(/waiting/i, { timeout: MOCK_REPLUG.beforeMs + 5_000 });
  await expect(badge(page)).toHaveText("Streaming", { timeout: MOCK_REPLUG.awayMs + 5_000 });
  await page.getByRole("button", { name: "Disconnect" }).click();
  await expect(badge(page)).toHaveText("Disconnected");
  await connectRealMeter(page, AFTER_REPLUG);
}

test.describe("Hide serial numbers (FR AXHK2Q)", () => {
  test("the switch is off by default and survives a reload (AC-AXHK2Q.1)", async ({ page }) => {
    await open(page);
    let about = await openAbout(page);
    await expect(hideSwitch(about)).toHaveAttribute("aria-checked", "false");
    await hideSwitch(about).click();
    await expect(hideSwitch(about)).toHaveAttribute("aria-checked", "true");

    await page.reload();
    about = await openAbout(page);
    await expect(hideSwitch(about)).toHaveAttribute("aria-checked", "true");
    await hideSwitch(about).click();

    await page.reload();
    about = await openAbout(page);
    await expect(hideSwitch(about)).toHaveAttribute("aria-checked", "false");
  });

  test("control: with the switch off the scan finds the serials it looks for", async ({ page }) => {
    await useAdvancedFirmware(page);
    await open(page, "?mock=replug");
    await streamRealMeterAfterReplug(page);
    expect(await serialsOnPage(page)).toContain(MOCK_REPLUG.serial);
    const prompt = await firmwareSessionPrompt(page);
    await expect(prompt).toContainText(MOCK_REPLUG.serial);
  });

  test("control: the scan finds the demo serial in the device details", async ({ page }) => {
    await open(page);
    await streamDemo(page);
    await openAbout(page);
    expect((await serialsOnPage(page)).length).toBeGreaterThan(0);
  });

  test("no serial in the picker, the top bar or the add dialog (AC-AXHK2Q.2, .3)", async ({
    page,
  }) => {
    await hideBeforeLoad(page);
    await open(page, "?mock=replug");
    const picker = page.getByLabel("Device", { exact: true });
    await expect(picker.locator('option[value^="usb:"]')).toHaveText("KM003C");
    await expectNoSerial(page);

    await picker.selectOption(FIRST_ADDRESS);
    await expectNoSerial(page);
    await picker.selectOption("demo:load-5v-1a");

    await streamDemo(page);
    await expectNoSerial(page);
    await page.getByRole("button", { name: "Add meter" }).click();
    await expect(page.getByRole("dialog", { name: "Add a meter" })).toBeVisible();
    await expectNoSerial(page);
  });

  test("no serial on the tiles, even while a meter is away (AC-AXHK2Q.2)", async ({ page }) => {
    await hideBeforeLoad(page);
    await open(page, "?mock=replug");
    await streamDemo(page);
    await page.getByRole("button", { name: "Add meter" }).click();
    const dialog = page.getByRole("dialog", { name: "Add a meter" });
    await dialog.getByLabel("Device", { exact: true }).selectOption(FIRST_ADDRESS);
    await dialog.getByRole("button", { name: "Connect" }).click();
    await expect(tiles(page)).toHaveCount(2);
    await expectNoSerial(page);

    const meterTile = tiles(page).nth(1);
    await expect(meterTile.getByRole("status")).toHaveText(/waiting/i, {
      timeout: MOCK_REPLUG.beforeMs + 5_000,
    });
    await expectNoSerial(page);
    await expect(meterTile.getByRole("status")).toHaveText("Streaming", {
      timeout: MOCK_REPLUG.awayMs + 5_000,
    });
    await expectNoSerial(page);
  });

  test("no serial in the meter details, About or meter settings (AC-AXHK2Q.2, .3)", async ({
    page,
  }) => {
    await hideBeforeLoad(page);
    await open(page);
    await streamDemo(page);
    await expect(page.getByRole("region", { name: "Meter settings" })).toContainText("50%");
    await expectNoSerial(page);

    const about = await openAbout(page);
    const details = about.getByRole("region", { name: "Device details" });
    await expect(details).toContainText("***");
    await expect(details).not.toContainText("Hidden");
    await expectNoSerial(page);
  });

  test("no serial in the firmware view (AC-AXHK2Q.2)", async ({ page }) => {
    await hideBeforeLoad(page);
    await useAdvancedFirmware(page);
    await open(page);
    await streamDemo(page);
    await page.getByRole("tab", { name: "Firmware" }).click();
    await expect(page.locator("#view-firmware")).toBeVisible();
    await expect(hideSwitch(await openAbout(page))).toHaveAttribute("aria-checked", "true");
    await page.keyboard.press("Escape");
    await expectNoSerial(page);
  });

  test("no serial in the records, trigger or PD views (AC-AXHK2Q.2)", async ({ page }) => {
    await hideBeforeLoad(page);
    await open(page);
    await streamDemo(page);
    for (const name of ["Records", "Trigger", "PD analyzer"]) {
      await page.getByRole("tab", { name }).click();
      await expectNoSerial(page);
    }
  });

  test("no serial in the firmware session prompt (AC-AXHK2Q.2)", async ({ page }) => {
    await hideBeforeLoad(page);
    await useAdvancedFirmware(page);
    await open(page, "?mock=replug");
    await streamRealMeterAfterReplug(page);
    await expectNoSerial(page);
    const prompt = await firmwareSessionPrompt(page);
    await expect(prompt.getByRole("listitem")).toContainText("KM003C");
    await expect(prompt.getByRole("listitem")).not.toContainText("Hidden");
    await expect(prompt.getByRole("listitem")).not.toContainText("—");
    await expectNoSerial(page);
  });

  test("turning it off restores the fields with no reconnect (AC-AXHK2Q.3)", async ({ page }) => {
    await open(page);
    await streamDemo(page);
    await addDemoMeter(page);
    const before = await connectedIds(page);
    await expect(tiles(page).nth(0)).toContainText("…3456");

    const about = await openAbout(page);
    await hideSwitch(about).click();
    await page.keyboard.press("Escape");
    await expect(tiles(page).nth(0)).not.toContainText("…3456");
    await expectNoSerial(page);

    await hideSwitch(await openAbout(page)).click();
    await page.keyboard.press("Escape");
    await expect(tiles(page).nth(0)).toContainText("…3456");
    await expect(tiles(page).nth(1)).toContainText("…3457");
    expect(await connectedIds(page)).toEqual(before);
    await expect(tiles(page)).toHaveCount(2);
  });

  // AC-AXHK2Q.4's model-and-position title stays for real meters; demo meters are named as
  // the demo device with their scenario instead (AC-NWGY72.4).
  test("demo tiles are titled as the demo device with serials hidden (AC-NWGY72.4)", async ({
    page,
  }) => {
    await hideBeforeLoad(page);
    await open(page);
    await streamDemo(page);
    await addDemoMeter(page);
    await expect(tiles(page).nth(0)).toContainText("Demo device — load-5v-1a");
    await expect(tiles(page).nth(1)).toContainText("Demo device — pd-9v-2a");
    await expect(tiles(page).nth(0)).not.toContainText("KM003C 1");
    await expect(page.getByLabel("Controls for Demo device — pd-9v-2a")).toHaveCount(1);
  });

  test("the Device select's tooltip follows the switch with no reconnect (AC-NKAD6S.1, .2, .3, .4)", async ({
    page,
  }) => {
    await hideBeforeLoad(page);
    await open(page, "?mock=replug");
    const picker = page.getByLabel("Device", { exact: true });
    await picker.selectOption(FIRST_ADDRESS);
    const before = await connectedIds(page);

    // AC .1: on, the tooltip names the model only.
    await expect(picker).toHaveAttribute("title", "KM003C");
    expect(await picker.getAttribute("title")).not.toContain(MOCK_REPLUG.serial);

    // AC .2, .3: off, the tooltip is the selected option's full label, at once.
    await hideSwitch(await openAbout(page)).click();
    await page.keyboard.press("Escape");
    const selected = picker.locator(`option[value="${FIRST_ADDRESS}"]`);
    await expect(selected).toContainText(MOCK_REPLUG.serial);
    const fullLabel = (await selected.textContent())?.trim() ?? "";
    await expect(picker).toHaveAttribute("title", fullLabel);
    expect(await picker.getAttribute("title")).toContain(MOCK_REPLUG.serial);

    // AC .3: on again, hidden again, still no reconnect.
    await hideSwitch(await openAbout(page)).click();
    await page.keyboard.press("Escape");
    await expect(picker).toHaveAttribute("title", "KM003C");
    await expect(picker).toHaveValue(FIRST_ADDRESS);
    expect(await connectedIds(page)).toEqual(before);
  });

  test("recording and export names carry no serial, on or off (AC-AXHK2Q.5)", async ({ page }) => {
    await open(page);
    await streamDemo(page);
    for (const hidden of [false, true]) {
      if (hidden) {
        await hideSwitch(await openAbout(page)).click();
        await page.keyboard.press("Escape");
      }
      await page.getByRole("button", { name: "Record", exact: true }).click();
      await page.getByRole("button", { name: "Stop", exact: true }).click();
      const saved = page.locator(".notice").filter({ hasText: /Recording saved/ });
      await expect(saved.first()).toBeVisible();
      for (const text of await saved.allInnerTexts()) {
        expect(text).toMatch(/powerz-\d{8}-\d{6}\.csv/);
        expect(text).not.toContain("123456");
      }

      await page.getByRole("button", { name: "Export", exact: true }).click();
      const exported = page.locator(".notice").filter({ hasText: /Export/i });
      await expect(exported.first()).toBeVisible();
      for (const text of await exported.allInnerTexts()) {
        expect(text).toMatch(/powerz-\d{8}-\d{6}\.csv/);
        expect(text).not.toContain("123456");
      }
    }
  });
});

test.describe("Hidden serials read as the model (FR CCF47K)", () => {
  test("no shown text reads Hidden, in the picker, the add dialog or About (AC-CCF47K.1, .3)", async ({
    page,
  }) => {
    await hideBeforeLoad(page);
    await open(page, "?mock=replug");
    const picker = page.getByLabel("Device", { exact: true });
    await expect(picker.locator('option[value^="usb:"]')).toHaveText("KM003C");
    await streamDemo(page);
    await page.getByRole("button", { name: "Add meter" }).click();
    const dialog = page.getByRole("dialog", { name: "Add a meter" });
    await expect(dialog.locator('option[value^="usb:"]')).toHaveText("KM003C");
    await page.keyboard.press("Escape");
    const about = await openAbout(page);
    const serialRow = about.getByRole("region", { name: "Device details" });
    await expect(serialRow).toContainText("***");
    expect(await page.locator("body").innerText()).not.toContain("Hidden");
  });

  test("two meters of one model are numbered by serial, with no serial on the page (AC-CCF47K.2, .5)", async ({
    page,
  }) => {
    await hideBeforeLoad(page);
    await open(page, "?mock=two-meters");
    const picker = page.getByLabel("Device", { exact: true });
    await expect(picker.locator(`option[value="${PAIR_LOWER}"]`)).toHaveText("KM003C (1)");
    await expect(picker.locator(`option[value="${PAIR_HIGHER}"]`)).toHaveText("KM003C (2)");
    await expectNoSerial(page);

    await picker.selectOption(PAIR_HIGHER);
    await expect(picker).toHaveAttribute("title", "KM003C (2)");
    await expectNoSerial(page);

    await picker.selectOption("demo:load-5v-1a");
    await expect(picker.locator('option[value^="demo:"]').first()).toHaveText(/^Demo device — /);
    await streamDemo(page);
    await page.getByRole("button", { name: "Add meter" }).click();
    const dialog = page.getByRole("dialog", { name: "Add a meter" });
    await expect(dialog.locator(`option[value="${PAIR_LOWER}"]`)).toHaveText("KM003C (1)");
    await expect(dialog.locator(`option[value="${PAIR_HIGHER}"]`)).toHaveText("KM003C (2)");
    await expectNoSerial(page);
  });

  test("a meter keeps its number in the add dialog while the other one is connected (AC-CCF47K.2)", async ({
    page,
  }) => {
    await hideBeforeLoad(page);
    await open(page, "?mock=two-meters");
    await connectRealMeter(page, PAIR_LOWER);
    await expectNoSerial(page);
    await page.getByRole("button", { name: "Add meter" }).click();
    const dialog = page.getByRole("dialog", { name: "Add a meter" });
    await expect(dialog.locator(`option[value="${PAIR_LOWER}"]`)).toHaveCount(0);
    await expect(dialog.locator(`option[value="${PAIR_HIGHER}"]`)).toHaveText("KM003C (2)");
    await expectNoSerial(page);
  });

  test("control: with the switch off the two-meter scan finds both serials (AC-CCF47K.4, .5)", async ({
    page,
  }) => {
    await open(page, "?mock=two-meters");
    const found = await serialsOnPage(page);
    expect(found).toContain(MOCK_TWO_METERS.first);
    expect(found).toContain(MOCK_TWO_METERS.second);
    const picker = page.getByLabel("Device", { exact: true });
    await expect(picker.locator(`option[value="${PAIR_HIGHER}"]`)).toHaveText(
      `KM003C — ${MOCK_TWO_METERS.first}`,
    );

    // On at once, with no reconnect: numbered, no serial.
    await hideSwitch(await openAbout(page)).click();
    await page.keyboard.press("Escape");
    await expect(picker.locator(`option[value="${PAIR_HIGHER}"]`)).toHaveText("KM003C (2)");
    await expectNoSerial(page);

    // Off again: the serials are back.
    await hideSwitch(await openAbout(page)).click();
    await page.keyboard.press("Escape");
    await expect(picker.locator(`option[value="${PAIR_LOWER}"]`)).toHaveText(
      `KM003C — ${MOCK_TWO_METERS.second}`,
    );
  });
});
