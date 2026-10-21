/**
 * AC-AH2AEN.5: the web build against the scripted mock backend: picker,
 * connect, live values, the error banner with the Linux permission hint, the
 * rate-locked notice, every shortcut and the theme toggle. Mock scenarios
 * (`?mock=…`) are listed in `helpers.ts`.
 */

import { expect, test } from "@playwright/test";
import { SHORTCUTS } from "../../src/lib/shortcuts";
import {
  DEMO_LOAD,
  DEMO_LOAD_LABEL,
  LINUX_PERMISSION_HINT,
  PERMISSION_MESSAGE,
  badge,
  blur,
  chartSeriesLengths,
  connectDemo,
  expectReadoutNear,
  open,
  readoutValue,
  statsElapsedSeconds,
  streamDemo,
} from "./helpers";

test.use({ viewport: { width: 1600, height: 900 } });

test.describe("picker and connect (AC-AH2AEN.5)", () => {
  test("the picker lists one demo device per simulator scenario", async ({ page }) => {
    await open(page);
    const options = page.getByLabel("Device", { exact: true }).locator("option");
    await expect(options).toHaveText([
      "Demo device — idle",
      DEMO_LOAD_LABEL,
      "Demo device — pd-9v-2a",
      "Demo device — noisy-negative",
    ]);
    await expect(badge(page)).toHaveText("Disconnected");
  });

  test("connect streams live values; disconnect stops them", async ({ page }) => {
    await open(page);
    await connectDemo(page);
    await expect(badge(page)).toHaveText("Streaming");
    await expect(page.getByLabel("Device", { exact: true })).toBeDisabled();
    await expectReadoutNear(page, "VBUS", 5, 0.05);
    await expectReadoutNear(page, "IBUS", 1, 0.05);

    // Live: the stats elapsed time advances and the chart keeps drawing.
    const elapsed0 = await statsElapsedSeconds(page);
    await expect
      .poll(() => statsElapsedSeconds(page), { timeout: 5_000 })
      .toBeGreaterThan(elapsed0);
    const drawn0 = await chartSeriesLengths(page);
    expect(drawn0.vbus).toBeGreaterThan(0);
    expect(drawn0.ibus).toBeGreaterThan(0);

    await page.getByRole("button", { name: "Disconnect" }).click();
    await expect(badge(page)).toHaveText("Disconnected");
    await expect(page.getByLabel("Device", { exact: true })).toBeEnabled();
    // Disconnected readouts show "—", not a stale value (AC-AH2AEK.2).
    expect(Number.isNaN(await readoutValue(page, "VBUS"))).toBe(true);
  });

  test("another scenario shows its own values", async ({ page }) => {
    await open(page);
    await streamDemo(page, "demo:pd-9v-2a");
    await expectReadoutNear(page, "VBUS", 9, 0.05);
    await expectReadoutNear(page, "IBUS", 2, 0.05);
  });
});

test.describe("errors and notices (AC-AH2AEN.5)", () => {
  test("a refused open shows the error banner with the Linux permission hint", async ({ page }) => {
    await open(page, "?mock=permission-denied");
    await connectDemo(page);
    const alert = page.getByRole("alert");
    await expect(alert).toContainText(PERMISSION_MESSAGE);
    await expect(alert.locator("code")).toHaveText(LINUX_PERMISSION_HINT);
    await expect(badge(page)).not.toHaveText("Streaming");
    await expect(page.getByRole("button", { name: "Connect" })).toBeEnabled();
    await alert.getByRole("button", { name: "Dismiss" }).click();
    await expect(page.getByRole("alert")).toHaveCount(0);
  });

  test("a meter that refuses authentication shows the rate-locked notice", async ({ page }) => {
    await open(page, "?mock=rate-locked");
    await connectDemo(page);
    await expect(badge(page)).toHaveText("Rate locked");
    await expect(
      page.getByText("Rate set on the meter: 50 SPS — authentication unavailable"),
    ).toBeVisible();
    await expect(page.getByLabel("Sample rate")).toBeDisabled();
    // The stream goes on at the meter's rate.
    await expectReadoutNear(page, "VBUS", 5, 0.05);
    // Recording still works while the rate is locked.
    await expect(page.getByRole("button", { name: "Record" })).toBeEnabled();
  });
});

test.describe("keyboard shortcuts (AC-AH2AEN.5)", () => {
  test("? lists every shortcut of src/lib/shortcuts.ts", async ({ page }) => {
    await open(page);
    await blur(page);
    await page.keyboard.press("?");
    const dialog = page.locator("#shortcut-help");
    await expect(dialog).toBeVisible();
    await expect(dialog.locator("dt")).toHaveText(SHORTCUTS.map((s) => s.keys));
    await expect(dialog.locator("dd")).toHaveText(SHORTCUTS.map((s) => s.description));
    await page.getByRole("button", { name: "Close shortcut list" }).click();
    await expect(dialog).toHaveCount(0);
  });

  test("C connects and disconnects", async ({ page }) => {
    await open(page);
    await page.getByLabel("Device", { exact: true }).selectOption(DEMO_LOAD);
    await blur(page);
    await page.keyboard.press("c");
    await expect(badge(page)).toHaveText("Streaming");
    await blur(page);
    await page.keyboard.press("c");
    await expect(badge(page)).toHaveText("Disconnected");
  });

  test("1–4 pick 2, 10, 50 and 1000 SPS", async ({ page }) => {
    await open(page);
    await streamDemo(page);
    const rate = page.getByLabel("Sample rate");
    for (const [key, sps] of [
      ["4", "1000"],
      ["1", "2"],
      ["2", "10"],
      ["3", "50"],
    ] as const) {
      await blur(page);
      await page.keyboard.press(key);
      await expect(rate).toHaveValue(sps);
    }
  });

  test("Space starts and stops a recording", async ({ page }) => {
    await open(page);
    await streamDemo(page);
    await blur(page);
    await page.keyboard.press(" ");
    const stop = page.getByRole("button", { name: "Stop", exact: true });
    await expect(stop).toHaveAttribute("aria-pressed", "true");
    await expect(page.getByRole("timer")).toBeVisible();
    await blur(page);
    await page.keyboard.press(" ");
    await expect(page.getByRole("button", { name: "Record", exact: true })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
  });

  test("R resets the statistics", async ({ page }) => {
    await open(page);
    await streamDemo(page);
    await expect
      .poll(() => statsElapsedSeconds(page), { timeout: 6_000 })
      .toBeGreaterThanOrEqual(2);
    await blur(page);
    await page.keyboard.press("r");
    await expect.poll(() => statsElapsedSeconds(page), { timeout: 2_000 }).toBeLessThan(2);
  });

  test("L returns the chart to live; arrow keys pause it (AC-DDDYDJ.3)", async ({ page }) => {
    await open(page);
    await streamDemo(page);
    const live = page.locator(".chart-wrap button.live");
    await expect(live).toHaveText("Live");
    await expect(live).toHaveAttribute("aria-pressed", "true");
    await expect.poll(async () => (await chartSeriesLengths(page)).vbus).toBeGreaterThan(10);
    // Arrow keys on the focused chart pan it into history.
    await page.getByRole("application").focus();
    await page.keyboard.press("ArrowLeft");
    await expect(live).toHaveAttribute("aria-pressed", "false");
    await expect(live).toHaveText("Paused · Go live");
    await blur(page);
    await page.keyboard.press("l");
    await expect(live).toHaveAttribute("aria-pressed", "true");
  });

  test("T cycles the theme System → Light → Dark → System", async ({ page }) => {
    await open(page);
    const theme = page.getByRole("radiogroup", { name: "Theme" });
    await expect(theme.getByRole("radio", { name: "System" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    for (const [label, resolved] of [
      ["Light", "light"],
      ["Dark", "dark"],
    ] as const) {
      await blur(page);
      await page.keyboard.press("t");
      await expect(theme.getByRole("radio", { name: label })).toHaveAttribute(
        "aria-checked",
        "true",
      );
      await expect(page.locator("html")).toHaveAttribute("data-theme", resolved);
    }
    await blur(page);
    await page.keyboard.press("t");
    await expect(theme.getByRole("radio", { name: "System" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
  });

  test("shortcuts do nothing while a modifier is held", async ({ page }) => {
    await open(page);
    await page.getByLabel("Device", { exact: true }).selectOption(DEMO_LOAD);
    // A listener added after the app's own sees the key once the app has handled it.
    await page.evaluate(() => {
      const w = window as unknown as { altCSeen?: boolean };
      window.addEventListener("keydown", (e) => {
        if (e.code === "KeyC" && e.altKey) w.altCSeen = true;
      });
    });
    await blur(page);
    await page.keyboard.press("Alt+c");
    await page.waitForFunction(() => (window as unknown as { altCSeen?: boolean }).altCSeen);
    // Two frames, so a state change the shortcut had started would be rendered.
    await page.evaluate(
      () =>
        new Promise<void>((done) =>
          requestAnimationFrame(() => requestAnimationFrame(() => done())),
        ),
    );
    await expect(badge(page)).toHaveText("Disconnected");
    // Without the modifier the same key connects, so the key did reach the shortcuts.
    await page.keyboard.press("c");
    await expect(badge(page)).toHaveText("Streaming");
  });
});

test.describe("theme toggle (AC-AH2AEN.5)", () => {
  test("picking Dark or Light repaints the page and survives a reload", async ({ page }) => {
    await open(page);
    const theme = page.getByRole("radiogroup", { name: "Theme" });
    await theme.getByRole("radio", { name: "Dark" }).click();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
    const darkBg = await page.evaluate(() => getComputedStyle(document.body).backgroundColor);
    await page.reload();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
    await page
      .getByRole("radiogroup", { name: "Theme" })
      .getByRole("radio", { name: "Light" })
      .click();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
    const lightBg = await page.evaluate(() => getComputedStyle(document.body).backgroundColor);
    expect(lightBg).not.toBe(darkBg);
  });

  test("at 900 px the theme lives in the overflow menu", async ({ page }) => {
    await page.setViewportSize({ width: 900, height: 700 });
    await open(page);
    await page.getByRole("button", { name: "More: theme and About" }).click();
    await page.locator("#top-bar-menu").getByRole("radio", { name: "Dark" }).click();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  });
});
