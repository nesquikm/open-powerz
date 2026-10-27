/**
 * FR V9SK55: the firmware update screen on the mocked backend.
 *
 * - AC .1: the Firmware tab is not there (not rendered, not reachable by keyboard) until
 *   the Advanced firmware switch in the settings (the About panel) is on; the switch
 *   persists across a reload.
 * - AC .2: the screen shows the meter's mode and versions and the chosen file's
 *   metadata; Update is enabled only for a file that passed inspection and fits the
 *   meter's model.
 * - AC .3: Update asks for the target version to be typed and warns to keep the meter
 *   connected and the laptop on power; Cancel sends nothing.
 * - AC .4: during the update the device picker, Connect and the PD analyzer / Trigger
 *   tabs are blocked, and each stage shows with the write progress.
 * - AC .5: success shows the new version; a failure shows the error and the recovery
 *   steps (hold Back while plugging in, then retry).
 * - AC .6: the keyboard alone gets through it all, axe passes on every state, and a
 *   failure at each stage is covered (`?mock=firmware-fail-<stage>`).
 *
 * WebKit (as Safari) tabs only to form fields unless Option is held, so the keyboard
 * path presses Alt+Tab there (as `focus.spec.ts` does).
 */

import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Locator, type Page } from "@playwright/test";
import { open, streamDemo } from "./helpers";

/** localStorage key of the Advanced firmware switch (`src/lib/settings.ts`). */
const ADVANCED_KEY = "open-powerz.advanced-firmware";

/** Turn the Advanced firmware switch on before the app loads, as a returning user. */
async function useAdvancedFirmware(page: Page): Promise<void> {
  await page.addInitScript((key) => localStorage.setItem(key, "true"), ADVANCED_KEY);
}

function firmwareTab(page: Page): Locator {
  return page.getByRole("tab", { name: "Firmware" });
}

function firmwarePanel(page: Page): Locator {
  return page.locator("#view-firmware");
}

function updateButton(page: Page): Locator {
  return firmwarePanel(page).getByRole("button", { name: "Update", exact: true });
}

function confirmDialog(page: Page): Locator {
  return page.getByRole("dialog", { name: "Confirm firmware update" });
}

/** No serious or critical axe violation on the page as it is now. */
async function expectAxeClean(page: Page, state: string): Promise<void> {
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
    `no serious or critical axe violations (${state})`,
  ).toEqual([]);
}

/** Press `key` until `target` has focus (at most `max` presses). */
async function pressUntilFocused(page: Page, target: Locator, key: string, max = 80) {
  for (let i = 0; i < max; i++) {
    if (await target.evaluate((el) => el === document.activeElement)) return;
    await page.keyboard.press(key);
  }
  await expect(target, `reached with ${key}`).toBeFocused();
}

/** Open the Firmware tab, choose the (mocked) file, open the confirmation. */
async function chooseAndConfirm(page: Page): Promise<void> {
  await firmwareTab(page).click();
  await expect(firmwarePanel(page)).toBeVisible();
  await firmwarePanel(page)
    .getByRole("button", { name: /choose file/i })
    .click();
  await expect(firmwarePanel(page).getByRole("region", { name: "Firmware file" })).toBeVisible();
  await expect(updateButton(page)).toBeEnabled();
  await updateButton(page).click();
  await expect(confirmDialog(page)).toBeVisible();
}

/** Type the target version and start the update. */
async function startUpdate(page: Page, version = "2.0.7"): Promise<void> {
  await confirmDialog(page)
    .getByRole("textbox", { name: /version/i })
    .fill(version);
  await confirmDialog(page).getByRole("button", { name: "Start update" }).click();
}

/** The mock backend's record of the updates it ran. */
function flashLog(page: Page): Promise<unknown[]> {
  return page.evaluate(() => {
    const mock = (window as unknown as Record<string, { firmwareFlashLog(): unknown[] }>)
      .__OPEN_POWERZ_MOCK__;
    return mock ? mock.firmwareFlashLog() : [];
  });
}

test.describe("firmware update (FR V9SK55)", () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 1600, height: 900 });
  });

  test("the Firmware screen is hidden until the Advanced firmware switch is on (AC-V9SK55.1)", async ({
    page,
  }) => {
    await open(page);
    await expect(page.getByRole("tab")).toHaveCount(4);
    await expect(firmwareTab(page)).toHaveCount(0);
    await expect(firmwarePanel(page)).toHaveCount(0);
    // Not reachable by keyboard either: End on the tabs stops at Trigger.
    await page.getByRole("tab", { name: "Meter" }).focus();
    await page.keyboard.press("End");
    await expect(page.getByRole("tab", { name: "Trigger" })).toBeFocused();

    await page.locator("#top-bar-about").click();
    const toggle = page.getByRole("dialog").getByRole("switch", { name: "Advanced firmware" });
    await expect(toggle).toHaveAttribute("aria-checked", "false");
    await toggle.click();
    await expect(toggle).toHaveAttribute("aria-checked", "true");
    await page.keyboard.press("Escape");
    await expect(firmwareTab(page)).toBeVisible();
    await expect(page.getByRole("tab")).toHaveCount(5);

    // Remembered after a reload.
    await page.reload();
    await expect(firmwareTab(page)).toBeVisible();

    // And off again hides it.
    await page.locator("#top-bar-about").click();
    await page.getByRole("dialog").getByRole("switch", { name: "Advanced firmware" }).click();
    await page.keyboard.press("Escape");
    await expect(firmwareTab(page)).toHaveCount(0);
    await expect(firmwarePanel(page)).toHaveCount(0);
  });

  test("the keyboard alone enables the switch, picks the file, confirms and sees the new version (AC-V9SK55.6)", async ({
    page,
    browserName,
  }) => {
    const tab = browserName === "webkit" ? "Alt+Tab" : "Tab";
    await open(page);
    await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());

    // Settings: About → Advanced firmware switch, with Space.
    await pressUntilFocused(page, page.locator("#top-bar-about"), tab);
    await page.keyboard.press("Enter");
    const toggle = page.getByRole("dialog").getByRole("switch", { name: "Advanced firmware" });
    await pressUntilFocused(page, toggle, tab);
    await page.keyboard.press("Space");
    await expect(toggle).toHaveAttribute("aria-checked", "true");
    await page.keyboard.press("Escape");
    await expect(page.getByRole("dialog")).toHaveCount(0);

    // The tabs: End moves to Firmware.
    await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
    await pressUntilFocused(page, page.getByRole("tab", { selected: true }), tab);
    await page.keyboard.press("End");
    await expect(firmwareTab(page)).toBeFocused();
    await expect(firmwareTab(page)).toHaveAttribute("aria-selected", "true");
    await expect(firmwarePanel(page)).toBeVisible();

    // Choose the file (the mocked dialog), then Update.
    const choose = firmwarePanel(page).getByRole("button", { name: /choose file/i });
    await pressUntilFocused(page, choose, tab);
    await page.keyboard.press("Enter");
    await expect(firmwarePanel(page).getByRole("region", { name: "Firmware file" })).toContainText(
      "2.0.7",
    );
    await pressUntilFocused(page, updateButton(page), tab);
    await page.keyboard.press("Enter");

    // The confirmation takes focus in its version field.
    const field = confirmDialog(page).getByRole("textbox", { name: /version/i });
    await expect(field).toBeFocused();
    await page.keyboard.type("2.0.7");
    const start = confirmDialog(page).getByRole("button", { name: "Start update" });
    await pressUntilFocused(page, start, tab);
    await page.keyboard.press("Enter");

    const done = firmwarePanel(page).getByRole("status").filter({ hasText: "2.0.7" });
    await expect(done).toBeVisible({ timeout: 30_000 });
    await expect(done).toContainText(/updated/i);
    // Focus stays on the screen, not lost to the page.
    await expect
      .poll(() =>
        page.evaluate(
          () => document.getElementById("view-firmware")?.contains(document.activeElement) ?? false,
        ),
      )
      .toBe(true);
  });

  test("shows the meter's mode and versions and the file's metadata (AC-V9SK55.2)", async ({
    page,
  }) => {
    await useAdvancedFirmware(page);
    await open(page);
    await streamDemo(page);
    await firmwareTab(page).click();
    const meter = firmwarePanel(page).getByRole("region", { name: "Meter" });
    await expect(meter).toContainText("Application");
    await expect(meter).toContainText("2.0.6");
    await expect(meter).toContainText("2.5");
    await expect(updateButton(page)).toBeDisabled();
    await expectAxeClean(page, "no file");

    await firmwarePanel(page)
      .getByRole("button", { name: /choose file/i })
      .click();
    const file = firmwarePanel(page).getByRole("region", { name: "Firmware file" });
    await expect(file).toContainText("KM003C");
    await expect(file).toContainText("2.0.7");
    await expect(file).toContainText("Release notes");
    await expect(updateButton(page)).toBeEnabled();
    await expectAxeClean(page, "file chosen");
  });

  test("a file for another model leaves Update disabled and says why (AC-V9SK55.2)", async ({
    page,
  }) => {
    await useAdvancedFirmware(page);
    await open(page, "?mock=firmware-other-model");
    await firmwareTab(page).click();
    await firmwarePanel(page)
      .getByRole("button", { name: /choose file/i })
      .click();
    await expect(firmwarePanel(page).getByRole("region", { name: "Firmware file" })).toContainText(
      "KM002C",
    );
    await expect(updateButton(page)).toBeDisabled();
    await expect(firmwarePanel(page)).toContainText(/KM002C.*KM003C|KM003C.*KM002C/);
    await expectAxeClean(page, "model mismatch");
  });

  test("a file that fails inspection shows the reason and leaves Update disabled (AC-V9SK55.2)", async ({
    page,
  }) => {
    await useAdvancedFirmware(page);
    await open(page, "?mock=firmware-bad-file");
    await firmwareTab(page).click();
    await firmwarePanel(page)
      .getByRole("button", { name: /choose file/i })
      .click();
    await expect(firmwarePanel(page).getByRole("alert")).toBeVisible();
    await expect(firmwarePanel(page).getByRole("region", { name: "Firmware file" })).toHaveCount(0);
    await expect(updateButton(page)).toBeDisabled();
    await expectAxeClean(page, "bad file");
  });

  test("Update needs the typed target version and warns to stay connected and on power (AC-V9SK55.3)", async ({
    page,
  }) => {
    await useAdvancedFirmware(page);
    await open(page);
    await chooseAndConfirm(page);
    const dialog = confirmDialog(page);
    await expect(dialog).toContainText(/keep the meter connected/i);
    await expect(dialog).toContainText(/laptop/i);
    await expect(dialog).toContainText(/power/i);
    await expect(dialog).toContainText("2.0.7");
    const start = dialog.getByRole("button", { name: "Start update" });
    const field = dialog.getByRole("textbox", { name: /version/i });
    await expect(start).toBeDisabled();
    for (const wrong of ["2.0.6", "2.0.7x", "yes"]) {
      await field.fill(wrong);
      await expect(start).toBeDisabled();
    }
    await field.fill("2.0.7");
    await expect(start).toBeEnabled();
    await expectAxeClean(page, "confirmation");

    // Cancel (and Escape) send nothing.
    await dialog.getByRole("button", { name: "Cancel" }).click();
    await expect(dialog).toHaveCount(0);
    await updateButton(page).click();
    await expect(confirmDialog(page)).toBeVisible();
    await expect(confirmDialog(page).getByRole("textbox", { name: /version/i })).toHaveValue("");
    await page.keyboard.press("Escape");
    await expect(confirmDialog(page)).toHaveCount(0);
    expect(await flashLog(page)).toEqual([]);
  });

  test("during the update everything else that talks to the meter is blocked and each stage shows (AC-V9SK55.4, .5)", async ({
    page,
  }) => {
    await useAdvancedFirmware(page);
    await open(page);
    await streamDemo(page);
    await chooseAndConfirm(page);
    await startUpdate(page);

    const progress = firmwarePanel(page).getByRole("region", { name: "Update progress" });
    await expect(progress).toBeVisible();
    // After the jump the meter needs a replug (AC-V9SK55.4).
    await expect(
      progress.getByRole("status").filter({ hasText: /unplug the meter and plug it back in/i }),
    ).toContainText(/bootloader/i);
    for (const stage of [/bootloader/i, /eras/i, /writ/i, /verif/i, /application/i]) {
      await expect(progress).toContainText(stage);
    }
    // Blocked while it runs: device switching, the stream, PD capture and triggering.
    await expect(page.getByLabel("Device", { exact: true })).toBeDisabled();
    await expect(page.getByRole("button", { name: /^(Connect|Disconnect)$/ })).toBeDisabled();
    await expect(page.getByRole("tab", { name: "PD analyzer" })).toHaveAttribute(
      "aria-disabled",
      "true",
    );
    await expect(page.getByRole("tab", { name: "Trigger" })).toHaveAttribute(
      "aria-disabled",
      "true",
    );
    await page.getByRole("tab", { name: "PD analyzer" }).click({ force: true });
    await expect(firmwarePanel(page)).toBeVisible();
    await expect(updateButton(page).and(page.locator(":enabled"))).toHaveCount(0);

    // The file cannot change under the running update, and progress cannot be hidden.
    await expect(firmwarePanel(page).getByRole("button", { name: "Choose file…" })).toBeDisabled();
    await page.locator("#top-bar-about").click();
    const lockedSwitch = page
      .getByRole("dialog")
      .getByRole("switch", { name: "Advanced firmware" });
    await expect(lockedSwitch).toBeDisabled();
    await expect(lockedSwitch).toHaveAttribute("aria-checked", "true");
    await page.keyboard.press("Escape");
    await expect(firmwareTab(page)).toBeVisible();

    const bar = progress.getByRole("progressbar", { name: "Write progress" });
    await expect
      .poll(async () => Number((await bar.getAttribute("aria-valuenow")) ?? "0"), {
        timeout: 30_000,
      })
      .toBeGreaterThan(0);
    await expectAxeClean(page, "updating");

    const done = firmwarePanel(page).getByRole("status").filter({ hasText: "2.0.7" });
    await expect(done).toBeVisible({ timeout: 30_000 });
    await expect(done).toContainText(/updated/i);
    await expectAxeClean(page, "success");

    // Released afterwards.
    await expect(page.getByLabel("Device", { exact: true })).toBeEnabled();
    await expect(page.getByRole("tab", { name: "PD analyzer" })).not.toHaveAttribute(
      "aria-disabled",
      "true",
    );
    expect(await flashLog(page)).toHaveLength(1);
  });

  const FAILURES = [
    { stage: "switching", label: /bootloader/i },
    { stage: "erasing", label: /eras/i },
    { stage: "writing", label: /writ/i },
    { stage: "verifying", label: /verif/i },
    { stage: "returning", label: /application/i },
  ] as const;

  for (const { stage, label } of FAILURES) {
    test(`a failure while ${stage} shows the error and the recovery steps (AC-V9SK55.5, .6)`, async ({
      page,
    }) => {
      await useAdvancedFirmware(page);
      await open(page, `?mock=firmware-fail-${stage}`);
      await chooseAndConfirm(page);
      await startUpdate(page);
      const alert = firmwarePanel(page).getByRole("alert");
      await expect(alert).toBeVisible({ timeout: 30_000 });
      await expect(alert).toContainText(label);
      await expect(alert).toContainText("Back");
      await expect(alert).toContainText(/plug/i);
      await expect(alert).toContainText(/again|retry/i);
      await expect(
        firmwarePanel(page)
          .getByRole("status")
          .filter({ hasText: /updated/i }),
      ).toHaveCount(0);
      await expectAxeClean(page, `failed while ${stage}`);
      // The meter is released for the retry.
      await expect(page.getByLabel("Device", { exact: true })).toBeEnabled();
    });
  }
});
