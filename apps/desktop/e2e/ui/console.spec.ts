/**
 * FR DGSQ6T: the PD message builder and the raw console in the trigger view, on the
 * mocked backend (AC .1–.5).
 *
 * Page contract (inside `#view-trigger`, while a trigger session is open, i.e. after
 * Detect):
 * - a region "PD message builder" with a select labelled "Target" (options "SOP",
 *   "SOP'", "SOP''"), a field labelled "Message type" (the 5-bit type number), a
 *   field labelled "Data objects" (hex words; empty = a control message), a button
 *   "Send message", and a button "Discover cable identity" that sends the Discover
 *   Identity preset to the cable (SOP');
 * - a region "Cable identity" with the cable's e-marker details once a cable answered
 *   (VID / PID in hex, "50 V", "5 A", "EPR");
 * - a region "Console" with a field labelled "Command", a button "Send", and a log
 *   (role `log`) named "Console transcript" whose lines read
 *   `HH:MM:SS.mmm > <sent>` / `HH:MM:SS.mmm < <received>`;
 * - a refusal (GoodCRC) or failure is shown as an alert in the view;
 * - anything classified above 5 V opens the trigger panel's confirmation dialog
 *   ("Cancel" / "Confirm"); Cancel sends nothing.
 * The mock records what it sent on `window.__OPEN_POWERZ_MOCK__.triggerCommands()`.
 */

import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Locator, type Page } from "@playwright/test";
import { open, streamDemo } from "./helpers";

test.use({ viewport: { width: 1600, height: 900 } });

const MOCK_HOOK = "__OPEN_POWERZ_MOCK__";
const TIME = String.raw`\d{2}:\d{2}:\d{2}\.\d{3}`;

function view(page: Page): Locator {
  return page.locator("#view-trigger");
}

function builder(page: Page): Locator {
  return view(page).getByRole("region", { name: "PD message builder" });
}

function consoleRegion(page: Page): Locator {
  return view(page).getByRole("region", { name: "Console", exact: true });
}

function transcript(page: Page): Locator {
  return consoleRegion(page).getByRole("log", { name: "Console transcript" });
}

function dialog(page: Page): Locator {
  return page.getByRole("dialog").or(page.getByRole("alertdialog"));
}

function alert(page: Page): Locator {
  return view(page).getByRole("alert");
}

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

/** Open the app, connect the demo device, show the trigger view and detect. */
async function detected(page: Page): Promise<void> {
  await open(page);
  await streamDemo(page);
  await page.getByRole("tab", { name: "Trigger" }).click();
  await expect(view(page)).toBeVisible();
  await view(page).getByRole("button", { name: "Detect", exact: true }).click();
  await expect(builder(page)).toBeVisible();
  await expect(consoleRegion(page)).toBeVisible();
}

async function typeLine(page: Page, line: string): Promise<void> {
  await consoleRegion(page).getByLabel("Command").fill(line);
  await consoleRegion(page).getByRole("button", { name: "Send", exact: true }).click();
}

async function build(page: Page, target: string, type: number, objects: string): Promise<void> {
  const b = builder(page);
  await b.getByLabel("Target").selectOption({ label: target });
  await b.getByLabel("Message type").fill(String(type));
  await b.getByLabel("Data objects").fill(objects);
  await b.getByRole("button", { name: "Send message" }).click();
}

// ---- AC-DGSQ6T.4: the raw console ----

test("the console sends a line and shows both sides with times (AC-DGSQ6T.4)", async ({ page }) => {
  await detected(page);
  await typeLine(page, "pdm parm");
  await expect(transcript(page)).toContainText(new RegExp(`${TIME} > pdm parm`));
  await expect(transcript(page)).toContainText(new RegExp(`${TIME} < type=0,em=2,sink=0`));
  expect(await commands(page)).toContain("pdm parm");
});

test("the console shows a parsed pdo block (AC-DGSQ6T.4)", async ({ page }) => {
  await detected(page);
  await typeLine(page, "entry pd");
  await expect(transcript(page)).toContainText(new RegExp(`${TIME} < pdo.*9\\.00 V`));
});

// ---- AC-DGSQ6T.3: GoodCRC ----

test("GoodCRC is refused in the console and the builder (AC-DGSQ6T.3)", async ({ page }) => {
  await detected(page);
  const before = await commands(page);
  await typeLine(page, "pd cmd=1");
  await expect(alert(page)).toContainText(/GoodCRC/);
  await build(page, "SOP'", 1, "");
  await expect(alert(page)).toContainText(/GoodCRC/);
  await expect(dialog(page)).toBeHidden();
  expect(await commands(page)).toEqual(before);
});

test("a refused console line shows its reason next to the console (AC-DGSQ6T.3 regression)", async ({
  page,
}) => {
  // Found on the real app: `pd cmd=01` + Send wrote nothing, but no reason
  // could be seen anywhere near the console.
  await detected(page);
  const before = await commands(page);
  await typeLine(page, "pd cmd=01");
  const reason = consoleRegion(page).getByRole("alert");
  await expect(reason).toBeVisible();
  await expect(reason).toContainText(/GoodCRC/);
  await expect(reason).toBeInViewport();
  await expect(dialog(page)).toBeHidden();
  expect(await commands(page)).toEqual(before);
});

// ---- AC-DGSQ6T.5: the confirmation ----

test("an unparsable line asks for the confirmation; Cancel sends nothing (AC-DGSQ6T.5)", async ({
  page,
}) => {
  await detected(page);
  const before = (await commands(page)).length;
  await typeLine(page, "hello meter");
  await expect(dialog(page)).toBeVisible();
  await expect(dialog(page)).toContainText(/nothing unsafe is connected to the output/i);
  await dialog(page).getByRole("button", { name: "Cancel" }).click();
  await expect(dialog(page)).toBeHidden();
  expect((await commands(page)).slice(before)).toEqual([]);
  await typeLine(page, "hello meter");
  await dialog(page).getByRole("button", { name: "Confirm" }).click();
  await expect(dialog(page)).toBeHidden();
  await expect(transcript(page)).toContainText(new RegExp(`${TIME} > hello meter`));
  expect((await commands(page)).slice(before)).toEqual(["hello meter"]);
});

test("a built Request for 9 V asks for the confirmation first (AC-DGSQ6T.1, .5)", async ({
  page,
}) => {
  await detected(page);
  const before = (await commands(page)).length;
  await build(page, "SOP", 2, "2304B12C");
  await expect(dialog(page)).toBeVisible();
  expect((await commands(page)).slice(before)).toEqual([]);
  await dialog(page).getByRole("button", { name: "Confirm" }).click();
  await expect(dialog(page)).toBeHidden();
  await expect
    .poll(async () => (await commands(page)).slice(before))
    .toEqual(["pd data=0082102CB10423"]);
});

// ---- AC-DGSQ6T.1: control messages ----

test("built control messages go out as pd cmd and pd data (AC-DGSQ6T.1)", async ({ page }) => {
  await detected(page);
  await build(page, "SOP", 13, "");
  await build(page, "SOP''", 13, "");
  await expect
    .poll(async () => commands(page))
    .toEqual(expect.arrayContaining(["pd cmd=13", "pd data=028D00"]));
  await expect(dialog(page)).toBeHidden();
});

// ---- AC-DGSQ6T.2: the cable preset ----

test("Discover cable identity shows the e-marker details (AC-DGSQ6T.2)", async ({ page }) => {
  await detected(page);
  const cable = view(page).getByRole("region", { name: "Cable identity" });
  await builder(page).getByRole("button", { name: "Discover cable identity" }).click();
  // Whether a VDM needs the confirmation is the classifier's call.
  await expect(dialog(page).or(cable.getByText(/0x050D/i))).toBeVisible();
  if (await dialog(page).isVisible()) {
    await dialog(page).getByRole("button", { name: "Confirm" }).click();
  }
  await expect(cable).toContainText(/0x050D/i);
  await expect(cable).toContainText(/0x8254/i);
  await expect(cable).toContainText(/50 V/);
  await expect(cable).toContainText(/5 A/);
  await expect(cable).toContainText(/EPR/);
  expect((await commands(page)).some((c) => c.startsWith("pd data=01"))).toBe(true);
});

test("the builder and the console pass axe (AC-DGSQ6T.1, .4)", async ({ page }) => {
  await detected(page);
  await typeLine(page, "pdm parm");
  const results = await new AxeBuilder({ page })
    .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa", "best-practice"])
    .include("#view-trigger")
    .analyze();
  const blocking = results.violations
    .filter((v) => v.impact === "serious" || v.impact === "critical")
    .map((v) => `${v.impact} ${v.id}: ${v.help}`);
  expect(blocking).toEqual([]);
});
