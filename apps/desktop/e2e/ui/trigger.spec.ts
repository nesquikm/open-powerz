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
 * - a "Back to 5 V" button while a session is open;
 * - (AC .8) in "QC": one button per QC 2.0 voltage whose name starts with it ("5 V",
 *   "9 V", "12 V", "20 V"), a slider named with "QC 3.0" (3.6-20 V in 200 mV steps)
 *   and a button whose name starts with "Request QC 3.0"; in "FCP", "AFC", "SFCP":
 *   "5 V", "9 V", "12 V" buttons; in "SCP", "VFCP", "TFCP": a slider named with
 *   "voltage" and one named with "current", bounded by the decoded limits (mV / mA or
 *   V / A), and a button whose name starts with "Request"; in "UFCS" the same per
 *   capability, every name carrying the capability's "#<index>". The current a
 *   request sends never exceeds the block's power at the requested voltage.
 * The mock records what it sent to its scripted charger on
 * `window.__OPEN_POWERZ_MOCK__.triggerCommands()`; `?mock=trigger-sag` keeps VBUS
 * more than 5 % below a request; `?mock=trigger-all` puts a charger that speaks every
 * protocol behind the demo meter (SCP 3.4-11 V at 0.5-5 A; UFCS #1 3.4-11 V at
 * 0.5-5 A up to 40 W, #2 3.4-21 V at 0.5-3 A up to 63 W).
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

/** Press Tab until `target` has the focus (at most `max` presses).
 * WebKit (as Safari) tabs only to form fields unless Option is held, so it
 * gets Alt+Tab, as in focus.spec.ts (CI run 40070430207, macOS WebKit). */
async function tabTo(page: Page, target: Locator, max = 120): Promise<void> {
  const key = page.context().browser()?.browserType().name() === "webkit" ? "Alt+Tab" : "Tab";
  for (let i = 0; i < max; i++) {
    if (await target.evaluate((el) => el === document.activeElement)) return;
    await page.keyboard.press(key);
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

// ---- AC-DGSQ6S.8: request controls for the other protocols ----

function legacyButton(page: Page, name: (typeof PROTOCOL_REGIONS)[number], volts: number): Locator {
  return section(page, name).getByRole("button", { name: new RegExp(`^${volts} V\\b`) });
}

/** A slider in `region` whose name has every one of `words`. */
function slider(page: Page, name: (typeof PROTOCOL_REGIONS)[number], ...words: string[]): Locator {
  const pattern = new RegExp(words.map((w) => `(?=.*${w})`).join(""), "i");
  return section(page, name).getByRole("slider", { name: pattern });
}

/** A slider's bound in mV / mA, whichever unit it uses. */
async function bound(s: Locator, attr: "min" | "max"): Promise<number> {
  const v = Number(await s.getAttribute(attr));
  return v < 100 ? Math.round(v * 1000) : v;
}

/** Moves a slider to its end with the keyboard. */
async function toEnd(s: Locator, key: "Home" | "End"): Promise<void> {
  await s.focus();
  await s.press(key);
}

async function confirmDialog(page: Page, volts: RegExp): Promise<void> {
  await expect(dialog(page)).toBeVisible();
  await expect(dialog(page)).toContainText(volts);
  await expect(dialog(page)).toContainText(/output/i);
  await dialog(page).getByRole("button", { name: "Confirm" }).click();
  await expect(dialog(page)).toBeHidden();
}

test("the QC section offers 5, 9, 12 and 20 V and a QC 3.0 control (AC-DGSQ6S.8)", async ({
  page,
}) => {
  await detected(page);
  for (const volts of [5, 9, 12, 20]) await expect(legacyButton(page, "QC", volts)).toBeVisible();
  const qc3 = slider(page, "QC", "QC 3\\.0");
  await expect(qc3).toBeVisible();
  expect(await bound(qc3, "min")).toBe(3600);
  expect(await bound(qc3, "max")).toBe(20000);
  expect([200, 0.2]).toContain(Number(await qc3.getAttribute("step")));
  await expect(section(page, "QC").getByRole("button", { name: /^Request QC 3\.0/ })).toBeVisible();
});

test("a QC 9 V request confirms, follows VBUS and goes back to 5 V (AC-DGSQ6S.8)", async ({
  page,
}) => {
  await detected(page);
  const before = await commands(page);
  await legacyButton(page, "QC", 9).click();
  await expect(dialog(page)).toBeVisible();
  // Nothing goes out while the dialog waits.
  expect(await commands(page)).toEqual(before);
  await confirmDialog(page, /\b9(\.0+)? V\b/);
  await expect.poll(async () => (await commands(page)).at(-1)).toBe("qc 9V");
  await expect(result(page)).toContainText(/\b9(\.0+)? V\b/);
  await expect
    .poll(async () => Math.abs((await liveVbus(page)) - 9) / 9, { timeout: 5_000 })
    .toBeLessThanOrEqual(0.05);
  await page.waitForTimeout(2_500);
  await expect(result(page).getByText(/more than 5\s?%/i)).toHaveCount(0);
  await backButton(page).click();
  await expect.poll(async () => (await commands(page)).at(-1)).toBe("reset");
  await expect.poll(async () => liveVbus(page), { timeout: 5_000 }).toBeLessThan(5.5);
});

test("QC 5 V and QC 3.0 at 3.6 V go out at once; QC 3.0 at 20 V asks first (AC-DGSQ6S.8)", async ({
  page,
}) => {
  await detected(page);
  await legacyButton(page, "QC", 5).click();
  await expect(dialog(page)).toBeHidden();
  await expect.poll(async () => (await commands(page)).at(-1)).toBe("qc 5V");
  const qc3 = slider(page, "QC", "QC 3\\.0");
  const request = section(page, "QC").getByRole("button", { name: /^Request QC 3\.0/ });
  await toEnd(qc3, "Home");
  await request.click();
  await expect(dialog(page)).toBeHidden();
  await expect.poll(async () => (await commands(page)).at(-1)).toBe("qc3 volt=3600");
  await toEnd(qc3, "End");
  await request.click();
  await confirmDialog(page, /\b20(\.0+)? V\b/);
  await expect.poll(async () => (await commands(page)).at(-1)).toBe("qc3 volt=20000");
});

test("FCP, AFC and SFCP offer 5, 9 and 12 V (AC-DGSQ6S.8)", async ({ page }) => {
  await detected(page, "?mock=trigger-all");
  for (const name of ["FCP", "AFC", "SFCP"] as const) {
    await expect(section(page, name)).not.toContainText(/not supported/i);
    for (const volts of [5, 9, 12]) await expect(legacyButton(page, name, volts)).toBeVisible();
    await expect(legacyButton(page, name, 20)).toHaveCount(0);
  }
  await legacyButton(page, "AFC", 12).click();
  await confirmDialog(page, /\b12(\.0+)? V\b/);
  await expect.poll(async () => (await commands(page)).at(-1)).toBe("afc 12V");
});

test("SCP offers voltage and current inside its limits (AC-DGSQ6S.8)", async ({ page }) => {
  await detected(page, "?mock=trigger-all");
  const volt = slider(page, "SCP", "voltage");
  const cur = slider(page, "SCP", "current");
  await expect(volt).toBeVisible();
  await expect(cur).toBeVisible();
  expect([await bound(volt, "min"), await bound(volt, "max")]).toEqual([3400, 11000]);
  expect([await bound(cur, "min"), await bound(cur, "max")]).toEqual([500, 5000]);
  await toEnd(volt, "End");
  await toEnd(cur, "End");
  await section(page, "SCP")
    .getByRole("button", { name: /^Request/ })
    .click();
  await confirmDialog(page, /\b11(\.0+)? V\b/);
  await expect.poll(async () => (await commands(page)).at(-1)).toBe("scp volt=11000,cur=5000");
  await expect
    .poll(async () => Math.abs((await liveVbus(page)) - 11) / 11, { timeout: 5_000 })
    .toBeLessThanOrEqual(0.05);
  await backButton(page).click();
  await expect.poll(async () => (await commands(page)).at(-1)).toBe("reset");
});

test("VFCP and TFCP offer voltage and current controls (AC-DGSQ6S.8)", async ({ page }) => {
  await detected(page, "?mock=trigger-all");
  for (const name of ["VFCP", "TFCP"] as const) {
    await expect(slider(page, name, "voltage")).toBeVisible();
    await expect(slider(page, name, "current")).toBeVisible();
    await expect(section(page, name).getByRole("button", { name: /^Request/ })).toBeVisible();
  }
});

test("UFCS offers each capability inside its limits and power (AC-DGSQ6S.8)", async ({ page }) => {
  await detected(page, "?mock=trigger-all");
  const v2 = slider(page, "UFCS", "#2\\b", "voltage");
  const c2 = slider(page, "UFCS", "#2\\b", "current");
  expect([await bound(v2, "min"), await bound(v2, "max")]).toEqual([3400, 21000]);
  expect([await bound(c2, "min"), await bound(c2, "max")]).toEqual([500, 3000]);
  await toEnd(v2, "End");
  await toEnd(c2, "End");
  await section(page, "UFCS")
    .getByRole("button", { name: /^Request.*#2\b/ })
    .click();
  await confirmDialog(page, /\b21(\.0+)? V\b/);
  await expect
    .poll(async () => (await commands(page)).at(-1))
    .toBe("ufcs req=2,volt=21000,cur=3000");
  // Capability #1 at 11 V is held to its 40 W.
  const v1 = slider(page, "UFCS", "#1\\b", "voltage");
  const c1 = slider(page, "UFCS", "#1\\b", "current");
  await toEnd(v1, "End");
  await toEnd(c1, "End");
  await section(page, "UFCS")
    .getByRole("button", { name: /^Request.*#1\b/ })
    .click();
  await confirmDialog(page, /\b11(\.0+)? V\b/);
  await expect.poll(async () => (await commands(page)).at(-1)).toMatch(/^ufcs req=1,volt=11000,/);
  const last = (await commands(page)).at(-1) ?? "";
  const ma = Number(/cur=(\d+)/.exec(last)?.[1]);
  expect(ma).toBeGreaterThanOrEqual(500);
  expect(11000 * ma).toBeLessThanOrEqual(40000 * 1000);
  await backButton(page).click();
  await expect.poll(async () => (await commands(page)).at(-1)).toBe("reset");
});

// AC .9 / .10 page contract: `?mock=trigger-req-error` makes the scripted charger
// answer every request above 5 V with `req error 252`; `?mock=trigger-out-of-sync`
// makes the meter answer requests with a reply that belongs to another command, which
// the backend reports as an `out_of_sync` error telling the user to power-cycle the
// meter.

test("a failed request shows the failure and its reason in the result (AC-DGSQ6S.9)", async ({
  page,
}) => {
  await detected(page, "?mock=trigger-req-error");
  await request9V(page);
  await expect(result(page)).not.toContainText("No request yet");
  await expect(result(page)).toContainText(/fail/i);
  await expect(result(page)).toContainText("252");
});

test("an out-of-sync meter shows the power-cycle error and disables requests (AC-DGSQ6S.10)", async ({
  page,
}) => {
  await detected(page, "?mock=trigger-out-of-sync");
  await request9V(page);
  await expect(view(page)).toContainText(/power[- ]cycle/i);
  await expect(offerButton(page, 9)).toBeDisabled();
  await expect(offerButton(page, 5)).toBeDisabled();
  // Back to 5 V stays available; the session is reopened by detecting again.
  await expect(backButton(page)).toBeVisible();
  await expect(backButton(page)).toBeEnabled();
});

// FR DGSQ6R AC .7 / .8 page contract: `?mock=trigger-list-lag` makes the meter answer
// one command late after the automatic list is cut off, so detection's `pdm parm`
// check fails and the backend reports `out_of_sync` (the mock records `entry bc`,
// `entry apple`, `entry list`, `reset`, `pdm parm` and nothing after);
// `?mock=trigger-detect-disagree` is the 2026-10-23 trace: `entry bc` → fail while the
// list says `bc1.2` supported, and the Detection region's BC1.2 row reads
// "entry bc: not supported · list: supported".

test("a meter out of step after a cut list shows the power-cycle error (AC-DGSQ6R.7)", async ({
  page,
}) => {
  await open(page, "?mock=trigger-list-lag");
  await streamDemo(page);
  await showTrigger(page);
  await detectButton(page).click();
  await expect(view(page)).toContainText(/power[- ]cycle/i);
  const sent = await commands(page);
  expect(sent.slice(sent.indexOf("entry list"))).toEqual(["entry list", "reset", "pdm parm"]);
  // Nothing was listed after the failed check.
  await expect(section(page, "USB PD").getByRole("button")).toHaveCount(0);
});

test("detection names which check said what when they disagree (AC-DGSQ6R.8)", async ({ page }) => {
  await detected(page, "?mock=trigger-detect-disagree");
  const detection = view(page).getByRole("region", { name: "Detection" });
  await expect(detection).toContainText("entry bc: not supported · list: supported");
});
