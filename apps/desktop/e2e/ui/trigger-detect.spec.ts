/**
 * FR AXHK2N, AC .14: while Detect runs, every control that sends to the meter is
 * disabled (the offers, Back to 5 V, Read cable and the console), and a press that
 * reached the page during Detect is dropped: no dialog opens by itself once Detect
 * ends, and nothing is sent. On the a5442bc hardware run a 9 V press made during
 * Detect opened its confirmation 55 s later, when Detect ended.
 *
 * The mock answers in turn like the meter's session here: Detect is slowed, and so
 * is the risk a press asks for, so that it answers only after Detect has ended.
 */

import { expect, test, type Locator, type Page } from "@playwright/test";
import { open, streamDemo } from "./helpers";

test.use({ viewport: { width: 1600, height: 900 } });

const MOCK_HOOK = "__OPEN_POWERZ_MOCK__";
/** How long the slowed Detect takes, ms. */
const DETECT_MS = 3000;

function view(page: Page): Locator {
  return page.locator("#view-trigger");
}

function detectButton(page: Page): Locator {
  return view(page).getByRole("button", { name: "Detect", exact: true });
}

function pdSection(page: Page): Locator {
  return view(page).getByRole("region", { name: "USB PD", exact: true });
}

function offerButton(page: Page, volts: number): Locator {
  return pdSection(page).getByRole("button", { name: new RegExp(`^${volts} V\\b`) });
}

function dialog(page: Page): Locator {
  return page.getByRole("dialog").or(page.getByRole("alertdialog"));
}

function consoleRegion(page: Page): Locator {
  return view(page).getByRole("region", { name: "Console", exact: true });
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

/** Makes the mock answer `method` only after `ms`. */
async function slowMock(page: Page, method: string, ms: number): Promise<void> {
  await page.evaluate(
    ({ key, method, ms }) => {
      const mock = (window as unknown as Record<string, Record<string, unknown>>)[key]!;
      const call = (mock[method] as (...args: unknown[]) => Promise<unknown>).bind(mock);
      mock[method] = async (...args: unknown[]) => {
        await new Promise((resolve) => setTimeout(resolve, ms));
        return call(...args);
      };
    },
    { key: MOCK_HOOK, method, ms },
  );
}

/** Detect once, then start a slow second Detect with the offers still on screen. */
async function detectingAgain(page: Page): Promise<void> {
  await open(page);
  await streamDemo(page);
  await page.getByRole("tab", { name: "Trigger" }).click();
  await detectButton(page).click();
  await expect(offerButton(page, 9)).toBeVisible();
  await slowMock(page, "triggerDetect", DETECT_MS);
  // A press asked during Detect is answered only after it, as the meter's session does.
  await slowMock(page, "triggerRisk", DETECT_MS + 500);
  await detectButton(page).click();
  await expect(detectButton(page)).toBeDisabled();
}

/** Detect has ended: its button is back. */
async function detectEnded(page: Page): Promise<void> {
  await expect(detectButton(page)).toBeEnabled({ timeout: DETECT_MS + 10_000 });
}

test("every control that sends is disabled while Detect runs (AC-AXHK2N.14)", async ({ page }) => {
  await detectingAgain(page);
  for (const volts of [5, 9, 15, 20]) await expect(offerButton(page, volts)).toBeDisabled();
  await expect(page.getByRole("button", { name: "Back to 5 V" })).toBeDisabled();
  await expect(view(page).getByRole("button", { name: "Read cable" })).toBeDisabled();
  await expect(
    consoleRegion(page).getByRole("button", { name: "Send", exact: true }),
  ).toBeDisabled();
  await expect(view(page).getByRole("button", { name: "Send message" })).toBeDisabled();

  await detectEnded(page);
  await expect(offerButton(page, 9)).toBeEnabled();
  await expect(page.getByRole("button", { name: "Back to 5 V" })).toBeEnabled();
});

// AC-AXHK2P.10: while Detect runs Read cable is off and its card says why. Page
// contract: the button's accessible description is the reason shown in the card.
const DETECT_REASON = /detecting|detect (is )?run/i;

function readCable(page: Page): Locator {
  return view(page).getByRole("button", { name: "Read cable", exact: true });
}

function cableCard(page: Page): Locator {
  return view(page).getByRole("region", { name: "Cable", exact: true });
}

test("Read cable says it is off because Detect runs, and only then (AC-AXHK2P.10)", async ({
  page,
}) => {
  await detectingAgain(page);
  await expect(readCable(page)).toBeDisabled();
  await expect(readCable(page)).toHaveAccessibleDescription(DETECT_REASON);
  await expect(cableCard(page).getByText(DETECT_REASON)).toBeVisible();

  await detectEnded(page);
  await expect(readCable(page)).not.toHaveAccessibleDescription(DETECT_REASON);
  await expect(cableCard(page).getByText(DETECT_REASON)).toBeHidden();
  // AC-AXHK2P.13: Detect ends on the last protocol it listed; a PD offer enters PD.
  await offerButton(page, 5).click();
  await expect(readCable(page)).toBeEnabled();
});

test("a 9 V press during Detect opens no dialog once Detect ends (AC-AXHK2N.14)", async ({
  page,
}) => {
  await detectingAgain(page);
  const before = await commands(page);
  // As the click on the meter did: it reached the page while Detect ran.
  await offerButton(page, 9).click({ force: true });
  await detectEnded(page);
  // Past the slowed risk's answer.
  await page.waitForTimeout(2000);
  await expect(dialog(page)).toBeHidden();
  const after = (await commands(page)).slice(before.length);
  expect(after.filter((c) => c === "pd req=2")).toEqual([]);
});

test("a 9 V press by keyboard during Detect opens no dialog either (AC-AXHK2N.14)", async ({
  page,
}) => {
  await detectingAgain(page);
  await offerButton(page, 9).focus();
  await page.keyboard.press("Enter");
  await detectEnded(page);
  await page.waitForTimeout(2000);
  await expect(dialog(page)).toBeHidden();
});

test("a console line typed during Detect is not sent (AC-AXHK2N.14)", async ({ page }) => {
  await detectingAgain(page);
  const before = (await commands(page)).length;
  await consoleRegion(page).getByLabel("Command").fill("pd cmd=13");
  await consoleRegion(page).getByLabel("Command").press("Enter");
  await detectEnded(page);
  await page.waitForTimeout(500);
  expect((await commands(page)).slice(before)).not.toContain("pd cmd=13");
});
