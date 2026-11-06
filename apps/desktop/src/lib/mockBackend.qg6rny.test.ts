/** The demo backend after a logo write (FR QG6RNY, AC .1 to .4, .6, .8): the write
 * result carries the duration before and the one set, the settings stay unsaved until
 * the mock's own menu save `MOCK_MENU_SAVE_MS` after the write, and while unsaved the
 * settings read and write are refused with the menu-save steps. `?menusave=never` never
 * saves, `?menusave=calibration` makes the first check after the save time report changed
 * calibration, `?menusave=saved` has the write's own fresh read already saved. All on
 * fake timers; nothing reaches a meter. */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MENU_SAVE_STEPS, MOCK_LOGO_FILE, MOCK_MENU_SAVE_MS, MockBackend } from "./mockBackend";

const DEMO = "demo:load-5v-1a";
const NOT_SAVED = "The meter has not saved its settings since the last change.";

/** A connected demo backend that read its settings, its options from page URL `search`. */
async function ready(search = "") {
  vi.stubGlobal("location", { search });
  const b = new MockBackend(null);
  await b.connect(DEMO, 50, () => {});
  await b.meterSettings();
  return b;
}

/** Runs `p` to its end on the fake clock, advancing at most `ms`. */
async function settle<T>(p: Promise<T>, ms = 5_000): Promise<T> {
  const out = p.then(
    (v) => ({ ok: true as const, v }),
    (e: unknown) => ({ ok: false as const, e }),
  );
  await vi.advanceTimersByTimeAsync(ms);
  const r = await out;
  if (r.ok) return r.v;
  throw r.e;
}

function startup(s: { fields: { key: string; value: string }[] }): string | undefined {
  return s.fields.find((f) => f.key === "startup_logo")?.value;
}

/** Uploads without letting the mock's menu save time pass: returns as soon as the write
 * resolves. */
async function upload(b: MockBackend) {
  const p = b.logoUpload(DEMO, MOCK_LOGO_FILE, false, () => {});
  let done = false;
  void p.then(
    () => (done = true),
    () => (done = true),
  );
  for (let i = 0; i < 400 && !done; i++) await vi.advanceTimersByTimeAsync(10);
  return p;
}

beforeEach(() => vi.useFakeTimers());
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("the steps (FR QG6RNY)", () => {
  it("qg6rny: MENU_SAVE_STEPS is the library's text", () => {
    expect(MENU_SAVE_STEPS).toBe(
      "On the meter: Settings → startup logo duration → pick a value → save. Don't unplug the meter first.",
    );
    expect(MOCK_MENU_SAVE_MS).toBe(3000);
  });
});

describe("the write result (AC-QG6RNY.1, .2)", () => {
  it("qg6rny: an upload sets 3 s and leaves the settings unsaved", async () => {
    const b = await ready();
    const w = await upload(b);
    expect(w).toEqual({
      duration_before: "off",
      duration_set: "3 s",
      settings: "needs_menu_save",
      startup_duration: null,
    });
    await b.disconnect();
  });

  it("qg6rny: a Remove turns the startup screen off, the duration before as it was", async () => {
    const b = await ready();
    await upload(b);
    await vi.advanceTimersByTimeAsync(MOCK_MENU_SAVE_MS + 100);
    await expect(b.logoSettingsCheck(DEMO)).resolves.toEqual({
      settings: "saved",
      startup_duration: "3 s",
    });
    const w = await settle(
      b.logoRemove(DEMO, () => {}),
      1_000,
    );
    expect(w).toEqual({
      duration_before: "3 s",
      duration_set: "off",
      settings: "needs_menu_save",
      startup_duration: null,
    });
    await b.disconnect();
  });
});

describe("the settings while unsaved (AC-QG6RNY.6)", () => {
  it("qg6rny: the settings read is refused with the steps", async () => {
    const b = await ready("?menusave=never");
    await upload(b);
    await expect(b.meterSettings()).rejects.toEqual({
      kind: "settings_not_saved",
      message: NOT_SAVED,
      hint: MENU_SAVE_STEPS,
    });
    await b.disconnect();
  });

  it("qg6rny: a settings write is refused with the steps and nothing is written", async () => {
    const b = await ready("?menusave=never");
    await upload(b);
    const before = b.meterSettingsWrites();
    await expect(b.meterSettingsWrite({ brightness: 80 })).rejects.toEqual({
      kind: "settings_not_saved",
      message: NOT_SAVED,
      hint: MENU_SAVE_STEPS,
    });
    await expect(b.meterSettingsWrite({ startup_duration: 2 })).rejects.toMatchObject({
      kind: "settings_not_saved",
    });
    expect(b.meterSettingsWrites()).toBe(before);
    await b.disconnect();
  });
});

describe("the check and the mock's menu save (AC-QG6RNY.2, .3, .8)", () => {
  it("qg6rny: not saved until MOCK_MENU_SAVE_MS after the write, then saved with the row", async () => {
    const b = await ready();
    await upload(b);
    await expect(b.logoSettingsCheck(DEMO)).resolves.toEqual({
      settings: "needs_menu_save",
      startup_duration: null,
    });
    await vi.advanceTimersByTimeAsync(MOCK_MENU_SAVE_MS + 100);
    await expect(b.logoSettingsCheck(DEMO)).resolves.toEqual({
      settings: "saved",
      startup_duration: "3 s",
    });
    // Saved: the settings read works again and shows the duration set.
    expect(startup(await b.meterSettings())).toBe("3 s");
    await b.disconnect();
  });

  it("qg6rny: ?menusave=never keeps the settings unsaved", async () => {
    const b = await ready("?menusave=never");
    await upload(b);
    await vi.advanceTimersByTimeAsync(30_000);
    await expect(b.logoSettingsCheck(DEMO)).resolves.toEqual({
      settings: "needs_menu_save",
      startup_duration: null,
    });
    await b.disconnect();
  });

  it("qg6rny: ?menusave=calibration reports changed calibration after the save time", async () => {
    const b = await ready("?menusave=calibration");
    await upload(b);
    await expect(b.logoSettingsCheck(DEMO)).resolves.toMatchObject({
      settings: "needs_menu_save",
    });
    await vi.advanceTimersByTimeAsync(MOCK_MENU_SAVE_MS + 100);
    await expect(b.logoSettingsCheck(DEMO)).resolves.toMatchObject({
      settings: "calibration_changed",
      startup_duration: null,
    });
    await b.disconnect();
  });
});

describe("a fresh read already saved (AC-QG6RNY.4)", () => {
  it("qg6rny: ?menusave=saved has the write report saved with its duration", async () => {
    const b = await ready("?menusave=saved");
    const w = await upload(b);
    expect(w).toEqual({
      duration_before: "off",
      duration_set: "3 s",
      settings: "saved",
      startup_duration: "3 s",
    });
    expect(startup(await b.meterSettings())).toBe("3 s");
    await b.disconnect();
  });
});
