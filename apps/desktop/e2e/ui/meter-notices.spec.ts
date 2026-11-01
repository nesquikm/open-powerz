/**
 * FR AXHK2K, on the mocked backend with several demo meters: every notice shows, on the
 * meter it belongs to.
 *
 * - AC-AXHK2K.1 / .2: a recording stopped on a tile, by its Stop or by agreeing to a
 *   firmware update, shows the saved-file notice with Reveal. A real update holds every
 *   real meter (`flash_scope`), so the backend names tile meters too; the mock's demo
 *   meters are told apart, so {@link holdEveryMeter} makes it answer like the backend.
 * - AC-AXHK2K.3: a failed rate change shows on the tile it was asked on.
 * - AC-AXHK2K.4: a notice stays with a parked meter: in its tile, and in the full view
 *   when it comes back.
 */

import { expect, type Locator, type Page, test } from "@playwright/test";
import { MOCK_REPLUG } from "../../src/lib/mockBackend";
import { badge, DEMO_LOAD, open, streamDemo } from "./helpers";

const DEMO_PD = "demo:pd-9v-2a";
const DEMO_NOISY = "demo:noisy-negative";
const LOAD = "load-5v-1a";
const PD = "pd-9v-2a";
const NOISY = "noisy-negative";
const SAVED = /Recording saved: .*File: powerz-\d{8}-\d{6}\.csv/;
const EXPORTED = /Exported [\d,]+ rows? to powerz-\d{8}-\d{6}\.csv/;
const REVEAL = /^(Reveal in Finder|Show in folder)$/;

interface MockHook {
  connectedIds(): string[];
  firmwareFlashLog(): unknown[];
  setRate(sps: number): Promise<void>;
  firmwareFlash(id: string, ...rest: unknown[]): Promise<unknown>;
  disconnect(): Promise<void>;
  exportSession(...args: unknown[]): Promise<unknown>;
  stopRecording(...args: unknown[]): Promise<unknown>;
  saveRecordingAs(...args: unknown[]): Promise<unknown>;
}
type HookWindow = { __OPEN_POWERZ_MOCK__: MockHook; __RELEASE_HELD__?: () => void };
type Holdable = "exportSession" | "stopRecording" | "saveRecordingAs";

test.use({ viewport: { width: 1600, height: 900 } });

test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem("open-powerz.advanced-firmware", "true"));
});

function grid(page: Page): Locator {
  return page.getByRole("region", { name: "Meters" });
}

function tileOf(page: Page, name: string): Locator {
  return grid(page).getByRole("article").filter({ hasText: name });
}

function button(scope: Locator, name: string | RegExp): Locator {
  return scope.getByRole("button", { name, exact: typeof name === "string" });
}

function tab(page: Page, name: string): Locator {
  return page.getByRole("tab", { name, exact: true });
}

/** The notices on screen in `scope`, those saying `text`. */
function notices(scope: Page | Locator, text: RegExp): Locator {
  return scope.locator(".notice").filter({ hasText: text }).filter({ visible: true });
}

async function addMeter(page: Page, id: string): Promise<void> {
  await page.getByRole("button", { name: "Add meter" }).click();
  const dialog = page.getByRole("dialog", { name: "Add a meter" });
  await dialog.getByLabel("Device", { exact: true }).selectOption(id);
  await dialog.getByRole("button", { name: "Connect" }).click();
  await expect(dialog).toBeHidden();
}

async function metersRunning(page: Page, ...ids: string[]): Promise<void> {
  await open(page);
  await streamDemo(page);
  for (const id of ids) await addMeter(page, id);
  await expect(grid(page).getByRole("article")).toHaveCount(ids.length + 1);
}

async function record(tile: Locator): Promise<void> {
  await button(tile, "Record").click();
  await expect(tile.getByRole("timer")).toHaveText(/[1-9][\d,]* rows?/);
}

/** The backend's answer to an update while real meters run: it names each of them. */
async function holdEveryMeter(page: Page, recordingId: string): Promise<void> {
  await page.evaluate((recording) => {
    const mock = (window as unknown as HookWindow).__OPEN_POWERZ_MOCK__;
    const real = mock.firmwareFlash.bind(mock);
    mock.firmwareFlash = (id, path, confirmation, onStage, ...more) => {
      const agreed = (more[0] ?? []) as { id: string }[];
      if (agreed.length > 0) return real(id, path, confirmation, onStage, ...more);
      const sessions = mock.connectedIds().map((held) => ({
        id: held,
        label: held,
        recording: held === recording,
        ...(held === recording && { recording_id: 1 }),
      }));
      return Promise.reject({
        kind: "flash_needs_consent",
        message: "The firmware update would end a running session.",
        hint: null,
        sessions,
      });
    };
  }, recordingId);
}

async function requestUpdate(page: Page): Promise<void> {
  const panel = page.locator("#view-firmware");
  await tab(page, "Firmware").click();
  await button(panel, /choose file/i).click();
  await expect(panel.getByRole("region", { name: "Firmware file" })).toBeVisible();
  await button(panel, "Update").click();
  const confirm = page.getByRole("dialog", { name: "Confirm firmware update" });
  await confirm.getByRole("textbox", { name: /version/i }).fill("2.0.7");
  await button(confirm, "Start update").click();
}

async function agreeToUpdate(page: Page): Promise<void> {
  await button(page.getByRole("dialog", { name: /session|recording/i }), /^end /i).click();
  await expect
    .poll(() =>
      page.evaluate(
        () => (window as unknown as HookWindow).__OPEN_POWERZ_MOCK__.firmwareFlashLog().length,
      ),
    )
    .toBe(1);
}

/** Leaves `command` unanswered until {@link releaseHeld}, as a save dialog left open does.
 * It still reaches the mock at once: a command reads its meter before its first await. */
async function hold(page: Page, command: Holdable): Promise<void> {
  await page.evaluate((name) => {
    const hook = window as unknown as HookWindow;
    const mock = hook.__OPEN_POWERZ_MOCK__;
    const real = mock[name].bind(mock);
    mock[name] = (...args) => {
      const answer = real(...args);
      return new Promise((resolve) => {
        hook.__RELEASE_HELD__ = () => resolve(answer);
      });
    };
  }, command);
}

/** Waits until a {@link hold}ed command was sent. */
async function expectHeld(page: Page): Promise<void> {
  await expect
    .poll(() =>
      page.evaluate(() => (window as unknown as HookWindow).__RELEASE_HELD__ !== undefined),
    )
    .toBe(true);
}

async function releaseHeld(page: Page): Promise<void> {
  await page.evaluate(() => (window as unknown as HookWindow).__RELEASE_HELD__?.());
}

/** The full view takes the meter on tile `name`. */
async function expand(page: Page, name: string): Promise<void> {
  await button(tileOf(page, name), "Expand").click();
  await expect(grid(page)).toBeHidden();
}

async function backToGrid(page: Page): Promise<void> {
  await button(page.locator("main"), "All meters").click();
  await expect(grid(page)).toBeVisible();
}

test("a tile Stop during an update shows the saved file (AC-AXHK2K.1, AC-AXHK2K.2)", async ({
  page,
}) => {
  await metersRunning(page, DEMO_PD);
  await record(tileOf(page, PD));
  await holdEveryMeter(page, DEMO_PD);
  await requestUpdate(page);
  await agreeToUpdate(page);

  // The updated meter left; the one recording is the only meter on the Meter tab.
  await tab(page, "Meter").click();
  const saved = notices(page, SAVED);
  await expect(saved).toHaveCount(1);
  await expect(button(saved, REVEAL)).toBeVisible();
});

test("agreeing to an update shows the saved file in the recording meter's tile (AC-AXHK2K.2)", async ({
  page,
}) => {
  await metersRunning(page, DEMO_PD, DEMO_NOISY);
  const noisy = tileOf(page, NOISY);
  await record(noisy);
  await holdEveryMeter(page, DEMO_NOISY);
  await requestUpdate(page);
  await agreeToUpdate(page);

  await tab(page, "Meter").click();
  const saved = notices(noisy, SAVED);
  await expect(saved).toHaveCount(1);
  await expect(button(saved, REVEAL)).toBeVisible();
  await expect(notices(tileOf(page, PD), SAVED)).toHaveCount(0);
});

test("the Stop button of a tile during an update shows the saved file (AC-AXHK2K.2)", async ({
  page,
}) => {
  await metersRunning(page, DEMO_PD, DEMO_NOISY);
  const noisy = tileOf(page, NOISY);
  await record(noisy);
  await tab(page, "Firmware").click();
  await requestUpdate(page);
  await agreeToUpdate(page);

  await tab(page, "Meter").click();
  await button(noisy, "Stop").click();
  const saved = notices(noisy, SAVED);
  await expect(saved).toHaveCount(1);
  await expect(button(saved, REVEAL)).toBeVisible();
});

test("the Stop button of any tile shows the saved file with no update running (AC-AXHK2K.2)", async ({
  page,
}) => {
  await metersRunning(page, DEMO_PD);
  for (const name of [LOAD, PD]) {
    const tile = tileOf(page, name);
    await record(tile);
    await button(tile, "Stop").click();
    const saved = notices(tile, SAVED);
    await expect(saved).toHaveCount(1);
    await expect(button(saved, REVEAL)).toBeVisible();
  }
});

test("a failed rate change shows its error on that tile (AC-AXHK2K.3)", async ({ page }) => {
  const refusal = "The meter refused that rate.";
  await metersRunning(page, DEMO_PD);
  await page.evaluate((message) => {
    (window as unknown as HookWindow).__OPEN_POWERZ_MOCK__.setRate = () =>
      Promise.reject({ kind: "usage", message, hint: null });
  }, refusal);

  for (const [name, other] of [
    [PD, LOAD],
    [LOAD, PD],
  ] as const) {
    const tile = tileOf(page, name);
    await tile.getByLabel("Sample rate").selectOption("10");
    await expect(tile.getByText(refusal)).toBeVisible();
    await expect(tileOf(page, other).getByText(refusal)).toHaveCount(0);
    await button(tile.locator(".notice").filter({ hasText: refusal }), /dismiss/i).click();
    await expect(tile.getByText(refusal)).toHaveCount(0);
  }
  await expect(badge(page)).toHaveText("Streaming");
});

test("a notice raised while a meter is parked waits in its tile and in the full view (AC-AXHK2K.4)", async ({
  page,
}) => {
  const unplugged = /the meter was unplugged/;
  await open(page, "?mock=replug");
  await streamDemo(page);
  await page.getByRole("button", { name: "Add meter" }).click();
  const dialog = page.getByRole("dialog", { name: "Add a meter" });
  const device = dialog.getByLabel("Device", { exact: true });
  const option = device.locator("option", { hasText: MOCK_REPLUG.serial });
  await device.selectOption((await option.getAttribute("value")) ?? "");
  await dialog.getByRole("button", { name: "Connect" }).click();

  const suffix = `…${MOCK_REPLUG.serial.slice(-4)}`;
  const tile = () => grid(page).getByRole("article").filter({ hasText: suffix });
  await button(tile(), "Record").click();
  // The other meter takes the view: this one records on, out of sight, until its unplug.
  await button(tileOf(page, LOAD), "Expand").click();
  await expect(grid(page)).toBeHidden();
  const running = () =>
    page.evaluate(() => (window as unknown as HookWindow).__OPEN_POWERZ_MOCK__.connectedIds());
  await expect.poll(running, { timeout: MOCK_REPLUG.beforeMs + 5_000 }).toEqual([DEMO_LOAD]);
  await expect
    .poll(async () => (await running()).length, { timeout: MOCK_REPLUG.awayMs + 5_000 })
    .toBe(2);

  await button(page.locator("main"), "All meters").click();
  await expect(notices(tile(), unplugged)).toHaveCount(1);

  await button(tile(), "Expand").click();
  await expect(grid(page)).toBeHidden();
  const back = notices(page, unplugged);
  await expect(back).toHaveCount(1);
  await expect(button(back, REVEAL)).toBeVisible();
});

test("a notice stays with its meter when that meter leaves the full view (AC-AXHK2K.4)", async ({
  page,
}) => {
  await metersRunning(page, DEMO_PD);
  const load = tileOf(page, LOAD);
  const pd = tileOf(page, PD);
  await record(load);
  await button(load, "Stop").click();
  await expect(notices(load, SAVED)).toHaveCount(1);

  await button(pd, "Expand").click();
  await expect(notices(page, SAVED)).toHaveCount(0);
  await button(page.locator("main"), "All meters").click();
  await expect(notices(load, SAVED)).toHaveCount(1);
  await expect(notices(pd, SAVED)).toHaveCount(0);
});

test("a refused Disconnect on a tile shows on that tile (AC-AXHK2K.4)", async ({ page }) => {
  const refusal = "The meter refused to let go.";
  await metersRunning(page, DEMO_PD);
  await page.evaluate((message) => {
    (window as unknown as HookWindow).__OPEN_POWERZ_MOCK__.disconnect = () =>
      Promise.reject({ kind: "usage", message, hint: null });
  }, refusal);
  const pd = tileOf(page, PD);

  await button(pd, "Disconnect").click();

  await expect(notices(pd, new RegExp(refusal))).toHaveCount(1);
  await expect(notices(tileOf(page, LOAD), new RegExp(refusal))).toHaveCount(0);
  // Nowhere else either: the full view's error banner stays quiet.
  await expect(page.getByText(refusal)).toHaveCount(1);
  await expand(page, PD);
  await expect(notices(page, new RegExp(refusal))).toHaveCount(1);
});

test("a tile's export goes with its meter into the full view (AC-AXHK2K.4)", async ({ page }) => {
  await metersRunning(page, DEMO_PD);
  const pd = tileOf(page, PD);

  await button(pd, "Export").click();

  await expect(notices(pd, EXPORTED)).toHaveCount(1);
  await expect(notices(tileOf(page, LOAD), EXPORTED)).toHaveCount(0);
  await expand(page, PD);
  const shown = notices(page, EXPORTED);
  await expect(shown).toHaveCount(1);
  await expect(button(shown, REVEAL)).toBeVisible();
});

test("a tile's export ends on its meter when another one took the view (AC-AXHK2K.4)", async ({
  page,
}) => {
  await metersRunning(page, DEMO_PD, DEMO_NOISY);
  await hold(page, "exportSession");
  await button(tileOf(page, PD), "Export").click();
  await expectHeld(page);

  await expand(page, NOISY);
  await releaseHeld(page);
  await backToGrid(page);

  await expect(notices(tileOf(page, PD), EXPORTED)).toHaveCount(1);
  await expect(notices(tileOf(page, NOISY), EXPORTED)).toHaveCount(0);
  await expect(notices(tileOf(page, LOAD), EXPORTED)).toHaveCount(0);
});

test("a full view export ends on its meter when the view changed (AC-AXHK2K.4)", async ({
  page,
}) => {
  await metersRunning(page, DEMO_PD);
  await hold(page, "exportSession");
  await expand(page, LOAD);
  await page.getByRole("button", { name: "Export", exact: true }).click();
  await expectHeld(page);

  await backToGrid(page);
  await expand(page, PD);
  await releaseHeld(page);
  await backToGrid(page);

  await expect(notices(tileOf(page, LOAD), EXPORTED)).toHaveCount(1);
  await expect(notices(tileOf(page, PD), EXPORTED)).toHaveCount(0);
});

for (const [what, command] of [
  ["Stop", "stopRecording"],
  ["Save dialog", "saveRecordingAs"],
] as const) {
  test(`a ${what} still open when the view changes saves on its meter (AC-AXHK2K.4)`, async ({
    page,
  }) => {
    await metersRunning(page, DEMO_PD);
    const load = tileOf(page, LOAD);
    await record(load);
    await hold(page, command);
    await button(load, "Stop").click();
    await expectHeld(page);

    await expand(page, PD);
    await releaseHeld(page);
    await backToGrid(page);

    const saved = notices(load, SAVED);
    await expect(saved).toHaveCount(1);
    await expect(button(saved, REVEAL)).toBeVisible();
    await expect(notices(tileOf(page, PD), SAVED)).toHaveCount(0);
  });
}

test("a parked meter the user disconnected leaves its notice with the meter shown (AC-AXHK2K.4)", async ({
  page,
}) => {
  const ended = /Recording stopped: .*File: powerz-\d{8}-\d{6}\.csv/;
  await metersRunning(page, DEMO_PD);
  const load = tileOf(page, LOAD);
  await record(load);
  await expand(page, PD);
  await backToGrid(page);

  await button(load, "Disconnect").click();

  await expect(grid(page)).toBeHidden();
  const moved = notices(page, ended);
  await expect(moved).toHaveCount(1);
  await expect(button(moved, REVEAL)).toBeVisible();
});
