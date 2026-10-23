/**
 * FR DGSQ6S: the desktop trigger view on the mocked backend (AC .1–.7): detection and
 * one section per protocol with unsupported ones marked, the PD offers as buttons with
 * PPS / AVS sliders and the emulation settings, the confirmation every request above
 * 5 V opens (Cancel sends nothing), the reply with the live VBUS and the 5 % / 2 s
 * flag, Back to 5 V, the cleanup when the view is left or the meter disconnected,
 * keyboard-only use, axe, and the themes.
 *
 * Page contract:
 * - a view tab "Trigger" whose tabpanel is `#view-trigger`;
 * - a "Detect" button (disabled with a "Connect a meter" reason without one) that
 *   opens the trigger session, detects, and lists every protocol;
 * - a region "Detection" with the result, and one region per protocol named "USB PD",
 *   "QC", "FCP", "AFC", "SFCP", "SCP", "VFCP", "TFCP", "UFCS"; a protocol the charger
 *   lacks says "Not supported" in its region;
 * - in "USB PD": one button per fixed offer whose name starts with its voltage
 *   ("9 V ..."), a slider per PPS / AVS offer named with "PPS" / "AVS", and the "PD
 *   emulation" settings;
 * - a request above 5 V opens a dialog naming the voltage and the output, with
 *   "Cancel" and "Confirm";
 * - a region "Request result" with the reply and the live VBUS (labelled "Live VBUS"),
 *   which says "more than 5 %" when VBUS is off by more than 5 % after 2 s;
 * - a "Back to 5 V" button while a session is open.
 * The mock records what it sent to its scripted charger on
 * `window.__OPEN_POWERZ_MOCK__.triggerCommands()`; `?mock=trigger-sag` keeps VBUS
 * more than 5 % below a request.
 */

import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Locator, type Page } from "@playwright/test";
import { open, streamDemo, useTheme, type Theme } from "./helpers";
import { backgroundLuminance } from "./pd";

test.use({ viewport: { width: 1600, height: 900 } });

const MOCK_HOOK = "__OPEN_POWERZ_MOCK__";

const PROTOCOL_REGIONS = [
  "USB PD",
  "QC",
  "FCP",
  "AFC",
  "SFCP",
  "SCP",
  "VFCP",
  "TFCP",
  "UFCS",
] as const;

function view(page: Page): Locator {
  return page.locator("#view-trigger");
}

function section(page: Page, name: (typeof PROTOCOL_REGIONS)[number]): Locator {
  return view(page).getByRole("region", { name, exact: true });
}

function detectButton(page: Page): Locator {
  return view(page).getByRole("button", { name: "Detect", exact: true });
}

function backButton(page: Page): Locator {
  return page.getByRole("button", { name: "Back to 5 V" });
}

function dialog(page: Page): Locator {
  return page.getByRole("dialog").or(page.getByRole("alertdialog"));
}

function offerButton(page: Page, volts: number): Locator {
  return section(page, "USB PD").getByRole("button", { name: new RegExp(`^${volts} V\\b`) });
}

function result(page: Page): Locator {
  return view(page).getByRole("region", { name: "Request result" });
}

/** What the mock's scripted charger received, oldest first. */
async function commands(page: Page): Promise<string[]> {
  return page.evaluate(
    (key) =>
      (
        (window as unknown as Record<string, { triggerCommands(): string[] }>)[key] ?? {
          triggerCommands: () => [],
        }
      ).triggerCommands(),
    MOCK_HOOK,
  );
}

/** The live VBUS the result shows, V. */
async function liveVbus(page: Page): Promise<number> {
  const text = await result(page).getByLabel("Live VBUS").innerText();
  return Number.parseFloat(text.replace(/−/g, "-").replace(/[^\d.+-]/g, ""));
}

async function showTrigger(page: Page): Promise<void> {
  await page.getByRole("tab", { name: "Trigger" }).click();
  await expect(view(page)).toBeVisible();
}

/** Open the app (with `query`), connect the demo device, show the view and detect. */
async function detected(page: Page, query = ""): Promise<void> {
  await open(page, query);
  await streamDemo(page);
  await showTrigger(page);
  await detectButton(page).click();
  await expect(section(page, "USB PD").getByRole("button").first()).toBeVisible();
}

/** Request 9 V on the PD section and confirm. */
async function request9V(page: Page): Promise<void> {
  await offerButton(page, 9).click();
  await expect(dialog(page)).toBeVisible();
  await dialog(page).getByRole("button", { name: "Confirm" }).click();
  await expect(dialog(page)).toBeHidden();
}

/** Press Tab until `target` has the focus (at most `max` presses). */
async function tabTo(page: Page, target: Locator, max = 120): Promise<void> {
  for (let i = 0; i < max; i++) {
    if (await target.evaluate((el) => el === document.activeElement)) return;
    await page.keyboard.press("Tab");
  }
  await expect(target, "reached with Tab").toBeFocused();
}

async function axeBlocking(page: Page, include?: string): Promise<string[]> {
  let builder = new AxeBuilder({ page }).withTags([
    "wcag2a",
    "wcag2aa",
    "wcag21a",
    "wcag21aa",
    "wcag22aa",
    "best-practice",
  ]);
  if (include) builder = builder.include(include);
  const results = await builder.analyze();
  return results.violations
    .filter((v) => v.impact === "serious" || v.impact === "critical")
    .map(
      (v) =>
        `${v.impact} ${v.id}: ${v.help} at ${v.nodes.map((n) => n.target.join(" ")).join("; ")}`,
    );
}

// ---- AC-DGSQ6S.1: detection and one section per protocol ----

test("Detect is disabled with a reason while no meter is connected (AC-DGSQ6S.1)", async ({
  page,
}) => {
  await open(page);
  await showTrigger(page);
  await expect(detectButton(page)).toBeDisabled();
  await expect(
    view(page)
      .getByText(/connect a meter/i)
      .first(),
  ).toBeVisible();
  await expect(backButton(page)).toBeHidden();
});

test("detection shows the result and marks unsupported protocols (AC-DGSQ6S.1, .7)", async ({
  page,
}) => {
  await detected(page);
  const detection = view(page).getByRole("region", { name: "Detection" });
  await expect(detection).toBeVisible();
  await expect(detection).toContainText(/bc1\.2/i);
  await expect(detection).toContainText(/apple/i);
  for (const name of PROTOCOL_REGIONS) await expect(section(page, name)).toBeVisible();
  await expect(section(page, "USB PD")).not.toContainText(/not supported/i);
  await expect(section(page, "QC")).not.toContainText(/not supported/i);
  for (const name of ["FCP", "AFC", "SFCP", "SCP", "VFCP", "TFCP", "UFCS"] as const) {
    await expect(section(page, name)).toContainText(/not supported/i);
  }
  // Detection asked for nothing above 5 V.
  expect((await commands(page)).filter((c) => /^pd req=(?!1\b)/.test(c))).toEqual([]);
});

// ---- AC-DGSQ6S.2: PD offers, PPS / AVS sliders, emulation settings ----

test("the PD section lists offers as buttons, PPS / AVS sliders and the settings (AC-DGSQ6S.2)", async ({
  page,
}) => {
  await detected(page);
  const pd = section(page, "USB PD");
  for (const volts of [5, 9, 15, 20]) await expect(offerButton(page, volts)).toBeVisible();
  const pps = pd.getByRole("slider", { name: /PPS/ });
  const avs = pd.getByRole("slider", { name: /AVS/ });
  await expect(pps).toBeVisible();
  await expect(avs).toBeVisible();
  // The PPS range is 3.3-21 V (in volts or millivolts).
  expect([3.3, 3300]).toContain(Number(await pps.getAttribute("min")));
  expect([21, 21000]).toContain(Number(await pps.getAttribute("max")));
  await expect(pd.getByText(/PD emulation/i)).toBeVisible();
  await expect(pd).toContainText(/e-?marker/i);
});

// ---- AC-DGSQ6S.3: confirmation above 5 V; Cancel sends nothing ----

test("a PD request above 5 V asks for confirmation naming the voltage (AC-DGSQ6S.3, .7)", async ({
  page,
}) => {
  await detected(page);
  const before = await commands(page);
  await offerButton(page, 9).click();
  await expect(dialog(page)).toBeVisible();
  await expect(dialog(page)).toContainText(/\b9(\.0+)? V\b/);
  await expect(dialog(page)).toContainText(/output/i);
  // Nothing goes out while the dialog waits.
  expect(await commands(page)).toEqual(before);
  await dialog(page).getByRole("button", { name: "Confirm" }).click();
  await expect(dialog(page)).toBeHidden();
  await expect.poll(async () => (await commands(page)).at(-1)).toMatch(/^pd req=2/);
});

test("Cancel sends nothing (AC-DGSQ6S.3, .7)", async ({ page }) => {
  await detected(page);
  const before = await commands(page);
  await offerButton(page, 9).click();
  await expect(dialog(page)).toBeVisible();
  await dialog(page).getByRole("button", { name: "Cancel" }).click();
  await expect(dialog(page)).toBeHidden();
  await page.waitForTimeout(500);
  expect(await commands(page)).toEqual(before);
});

test("a 5 V offer is requested without a dialog (AC-DGSQ6S.3)", async ({ page }) => {
  await detected(page);
  await offerButton(page, 5).click();
  await expect(dialog(page)).toBeHidden();
  await expect.poll(async () => (await commands(page)).at(-1)).toBe("pd req=1");
});

// ---- AC-DGSQ6S.4: reply, live VBUS, the 5 % / 2 s flag ----

test("after a request the panel shows the reply and the live VBUS (AC-DGSQ6S.4)", async ({
  page,
}) => {
  await detected(page);
  await request9V(page);
  await expect(result(page)).toContainText(/requested/i);
  await expect(result(page)).toContainText(/\b9(\.0+)? V\b/);
  await expect
    .poll(async () => Math.abs((await liveVbus(page)) - 9) / 9, { timeout: 5_000 })
    .toBeLessThanOrEqual(0.05);
  await page.waitForTimeout(2_500);
  await expect(result(page).getByText(/more than 5\s?%/i)).toHaveCount(0);
});

test("VBUS more than 5 % off the request is flagged after 2 s (AC-DGSQ6S.4)", async ({ page }) => {
  await detected(page, "?mock=trigger-sag");
  await request9V(page);
  await expect(result(page).getByText(/more than 5\s?%/i)).toBeVisible({ timeout: 5_000 });
});

// ---- AC-DGSQ6S.5: Back to 5 V; leaving and disconnecting close the session ----

test("Back to 5 V is visible while a session is open and returns to 5 V (AC-DGSQ6S.5, .7)", async ({
  page,
}) => {
  await open(page);
  await streamDemo(page);
  await showTrigger(page);
  await expect(backButton(page)).toBeHidden();
  await detectButton(page).click();
  await expect(backButton(page)).toBeVisible();
  await request9V(page);
  await expect(backButton(page)).toBeVisible();
  await backButton(page).click();
  await expect.poll(async () => (await commands(page)).at(-1)).toBe("pd req=1");
  await expect.poll(async () => liveVbus(page), { timeout: 5_000 }).toBeLessThan(5.5);
  await expect(backButton(page)).toBeVisible();
});

test("leaving the view returns to 5 V and closes the session (AC-DGSQ6S.5)", async ({ page }) => {
  await detected(page);
  await request9V(page);
  await page.getByRole("tab", { name: "Meter" }).click();
  await expect
    .poll(async () => (await commands(page)).slice(-2))
    .toEqual(["pd req=1", "pdm close"]);
  // Coming back finds no session open.
  await showTrigger(page);
  await expect(backButton(page)).toBeHidden();
});

test("disconnecting the meter returns to 5 V and closes the session (AC-DGSQ6S.5)", async ({
  page,
}) => {
  await detected(page);
  await request9V(page);
  await page.getByRole("button", { name: "Disconnect" }).click();
  await expect
    .poll(async () => (await commands(page)).slice(-2))
    .toEqual(["pd req=1", "pdm close"]);
  await expect(backButton(page)).toBeHidden();
});

// ---- AC-DGSQ6S.6: keyboard alone, axe, themes ----

test("the view works with the keyboard alone (AC-DGSQ6S.6)", async ({ page }) => {
  await open(page);
  await streamDemo(page);
  // To the Trigger tab with the arrow keys.
  await page.getByRole("tab", { name: "Meter" }).focus();
  await page.keyboard.press("End");
  await expect(page.getByRole("tab", { name: "Trigger" })).toBeFocused();
  await expect(page.getByRole("tab", { name: "Trigger" })).toHaveAttribute("aria-selected", "true");
  await expect(view(page)).toBeVisible();
  await tabTo(page, detectButton(page));
  await page.keyboard.press("Enter");
  await expect(offerButton(page, 9)).toBeVisible();
  // A request opens the dialog with the focus inside; Escape cancels and sends nothing.
  const before = await commands(page);
  await tabTo(page, offerButton(page, 9));
  await page.keyboard.press("Enter");
  await expect(dialog(page)).toBeVisible();
  await expect
    .poll(() => dialog(page).evaluate((d) => d.contains(document.activeElement)))
    .toBe(true);
  await page.keyboard.press("Escape");
  await expect(dialog(page)).toBeHidden();
  await expect(offerButton(page, 9)).toBeFocused();
  expect(await commands(page)).toEqual(before);
  // Again, confirmed from the keyboard.
  await page.keyboard.press("Enter");
  await expect(dialog(page)).toBeVisible();
  await tabTo(page, dialog(page).getByRole("button", { name: "Confirm" }), 10);
  await page.keyboard.press("Enter");
  await expect.poll(async () => (await commands(page)).at(-1)).toMatch(/^pd req=2/);
  // The PPS slider moves with the arrow keys.
  const pps = section(page, "USB PD").getByRole("slider", { name: /PPS/ });
  await tabTo(page, pps);
  const was = await pps.inputValue();
  await page.keyboard.press("ArrowRight");
  await expect.poll(() => pps.inputValue()).not.toBe(was);
  // Back to 5 V.
  await tabTo(page, backButton(page));
  await page.keyboard.press("Enter");
  await expect.poll(async () => (await commands(page)).at(-1)).toBe("pd req=1");
});

for (const theme of ["light", "dark"] as const satisfies readonly Theme[]) {
  test(`axe: the trigger view after detection, ${theme} (AC-DGSQ6S.6)`, async ({ page }) => {
    await useTheme(page, theme);
    await detected(page);
    await request9V(page);
    await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
    expect(await axeBlocking(page), "no serious or critical axe violations").toEqual([]);
  });

  test(`axe: the confirmation dialog, ${theme} (AC-DGSQ6S.6)`, async ({ page }) => {
    await useTheme(page, theme);
    await detected(page);
    await offerButton(page, 9).click();
    await expect(dialog(page)).toBeVisible();
    expect(await axeBlocking(page), "no serious or critical axe violations").toEqual([]);
  });

  test(`the trigger view follows the ${theme} theme (AC-DGSQ6S.6)`, async ({ page }) => {
    await useTheme(page, theme);
    await detected(page);
    await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
    const bg = await backgroundLuminance(section(page, "USB PD"));
    if (theme === "dark") expect(bg).toBeLessThan(0.2);
    else expect(bg).toBeGreaterThan(0.7);
  });
}

test("the trigger view follows the system theme (AC-DGSQ6S.6)", async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem("open-powerz.theme", "system"));
  await page.emulateMedia({ colorScheme: "dark" });
  await detected(page);
  expect(await backgroundLuminance(section(page, "USB PD"))).toBeLessThan(0.2);
  await page.emulateMedia({ colorScheme: "light" });
  await expect.poll(() => backgroundLuminance(section(page, "USB PD"))).toBeGreaterThan(0.7);
});
