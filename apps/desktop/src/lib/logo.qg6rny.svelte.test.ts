/** The Startup logo view's state after a logo write (FR QG6RNY, AC .1 to .5): the
 * confirmations word what the write does to the startup duration, a write whose settings
 * need a menu save enters `saving` and checks the settings read-only at least
 * `SETTINGS_CHECK_MS` apart, never two at once, until saved (then the settings are read
 * again) or calibration changed (failed); the check stops and starts with the view and
 * the meter, and no other logo write starts while it runs. Fake backends and fake
 * timers; nothing reaches a meter. */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  LogoPreview,
  LogoProgress,
  LogoRead,
  LogoSettingsCheck,
  LogoWritten,
  MeterSettings,
  SettingChanges,
  UiError,
} from "./backend";
import {
  REMOVE_WORD,
  SETTINGS_CHECK_MS,
  StartupLogoState,
  UPLOAD_WORD,
  type LogoMeter,
} from "./logo.svelte";

function settings(startup: string): MeterSettings {
  return {
    fields: [{ key: "startup_logo", label: "Startup logo", value: startup, tag: "confirmed" }],
  };
}

const PREVIEW: LogoPreview = {
  width: 100,
  height: 60,
  mode: "opaque",
  blob_size: 12_016,
  padded_size: 12_032,
  chunks: 47,
  small_side_warning: false,
  rgba: "AAAA",
};

const NEEDS_SAVE: LogoWritten = {
  duration_before: "off",
  duration_set: "3 s",
  settings: "needs_menu_save",
  startup_duration: null,
};

const NOT_SAVED: LogoSettingsCheck = { settings: "needs_menu_save", startup_duration: null };

function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  promise.catch(() => {});
  return { promise, resolve, reject };
}

function harness(
  written: LogoWritten = NEEDS_SAVE,
  meter: LogoMeter = { model: "KM003C", settings: settings("off") },
) {
  const checkTimes: number[] = [];
  const deps = {
    choose: vi.fn(async (): Promise<string | null> => "/pics/logo.png"),
    preview: vi.fn(async (): Promise<LogoPreview> => PREVIEW),
    read: vi.fn(async (): Promise<LogoRead> => ({
      status: "empty",
      width: null,
      height: null,
      mode: null,
      image: null,
    })),
    upload: vi.fn(async (path: string, alpha: boolean, cb: (p: LogoProgress) => void) => {
      cb({ step: "chunks", done: path.length > 0 && !alpha ? 1 : 0, total: 1 });
      return written;
    }),
    remove: vi.fn(async (cb: (p: LogoProgress) => void) => {
      cb({ step: "chunks", done: 1, total: 1 });
      return { ...written, duration_set: "off" };
    }),
    writeSettings: vi.fn(async (c: SettingChanges): Promise<MeterSettings> =>
      settings(Object.keys(c).length > 0 ? "3 s" : "off"),
    ),
    checkSettings: vi.fn(async (): Promise<LogoSettingsCheck> => {
      checkTimes.push(Date.now());
      return NOT_SAVED;
    }),
    refreshSettings: vi.fn(async (): Promise<void> => {}),
  };
  const current = { ...meter };
  const state = new StartupLogoState(deps, () => current);
  return { deps, state, current, checkTimes };
}

/** Chooses the picture, confirms Upload and lets the write resolve. */
async function uploaded(h: ReturnType<typeof harness>) {
  await h.state.choose();
  h.state.askUpload();
  await h.state.confirm(UPLOAD_WORD);
  await vi.advanceTimersByTimeAsync(0);
}

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

describe("the confirmations (AC-QG6RNY.1)", () => {
  it("qg6rny: Upload says the duration goes from its value now to 3 s", () => {
    expect(harness().state.uploadConfirmText).toBe("The startup duration goes from off to 3 s.");
    const on = harness(NEEDS_SAVE, { model: "KM003C", settings: settings("5 s") });
    expect(on.state.uploadConfirmText).toBe("The startup duration goes from 5 s to 3 s.");
    const unread = harness(NEEDS_SAVE, { model: "KM003C", settings: null });
    expect(unread.state.uploadConfirmText).toBe("The startup duration goes from unknown to 3 s.");
  });

  it("qg6rny: Remove says the startup screen is turned off", () => {
    const h = harness(NEEDS_SAVE, { model: "KM003C", settings: settings("3 s") });
    expect(h.state.removeConfirmText).toBe(
      "The startup screen is turned off (startup duration off).",
    );
  });
});

describe("a write that needs a menu save (AC-QG6RNY.2)", () => {
  it("qg6rny: enters saving, busy, showing the set duration", async () => {
    const h = harness();
    await uploaded(h);
    expect(h.state.phase).toBe("saving");
    expect(h.state.busy).toBe(true);
    expect(h.state.written).toEqual(NEEDS_SAVE);
    expect(h.state.startupDuration).toBe("3 s");
  });

  it("qg6rny: checks the settings read-only, at least SETTINGS_CHECK_MS apart", async () => {
    expect(SETTINGS_CHECK_MS).toBe(2000);
    const h = harness();
    await uploaded(h);
    await vi.advanceTimersByTimeAsync(10_500);
    expect(h.deps.checkSettings.mock.calls.length).toBeGreaterThanOrEqual(4);
    expect(h.deps.checkSettings.mock.calls.length).toBeLessThanOrEqual(6);
    for (let i = 1; i < h.checkTimes.length; i++) {
      expect(h.checkTimes[i]! - h.checkTimes[i - 1]!).toBeGreaterThanOrEqual(SETTINGS_CHECK_MS);
    }
    // Read-only: the checks never write settings or the logo.
    expect(h.deps.writeSettings).not.toHaveBeenCalled();
    expect(h.deps.upload).toHaveBeenCalledTimes(1);
    expect(h.state.saveCheck.checks).toBe(h.deps.checkSettings.mock.calls.length);
    expect(h.state.saveCheck.lastAt).not.toBeNull();
    expect(h.state.phase).toBe("saving");
  });

  it("qg6rny: never two checks in flight: the next waits for the last answer", async () => {
    const h = harness();
    const slow = deferred<LogoSettingsCheck>();
    h.deps.checkSettings.mockImplementationOnce(() => {
      h.checkTimes.push(Date.now());
      return slow.promise;
    });
    await uploaded(h);
    await vi.advanceTimersByTimeAsync(SETTINGS_CHECK_MS + 10);
    expect(h.deps.checkSettings).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(20_000);
    expect(h.deps.checkSettings).toHaveBeenCalledTimes(1);
    slow.resolve(NOT_SAVED);
    await vi.advanceTimersByTimeAsync(SETTINGS_CHECK_MS - 10);
    expect(h.deps.checkSettings).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(20);
    expect(h.deps.checkSettings).toHaveBeenCalledTimes(2);
  });

  it("qg6rny: a failed check is shown and checking goes on", async () => {
    const h = harness();
    const err: UiError = {
      kind: "timeout",
      message: "The meter did not answer in time.",
      hint: null,
    };
    h.deps.checkSettings.mockRejectedValueOnce(err);
    await uploaded(h);
    await vi.advanceTimersByTimeAsync(SETTINGS_CHECK_MS + 10);
    expect(h.state.saveCheck.error).toEqual(err);
    expect(h.state.phase).toBe("saving");
    await vi.advanceTimersByTimeAsync(SETTINGS_CHECK_MS + 10);
    expect(h.deps.checkSettings).toHaveBeenCalledTimes(2);
    expect(h.state.saveCheck.error).toBeNull();
  });
});

describe("the check's end (AC-QG6RNY.3)", () => {
  it("qg6rny: saved ends done with the duration read and reads the settings again once", async () => {
    const h = harness();
    h.deps.checkSettings
      .mockResolvedValueOnce(NOT_SAVED)
      .mockResolvedValueOnce({ settings: "saved", startup_duration: "5 s" });
    await uploaded(h);
    await vi.advanceTimersByTimeAsync(2 * SETTINGS_CHECK_MS + 50);
    expect(h.state.phase).toBe("done");
    expect(h.state.busy).toBe(false);
    expect(h.state.written?.startup_duration).toBe("5 s");
    expect(h.state.startupDuration).toBe("5 s");
    expect(h.deps.refreshSettings).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(20_000);
    expect(h.deps.checkSettings).toHaveBeenCalledTimes(2);
    expect(h.deps.refreshSettings).toHaveBeenCalledTimes(1);
  });

  it("qg6rny: changed calibration fails as an error and stops checking", async () => {
    const h = harness();
    h.deps.checkSettings.mockResolvedValueOnce({
      settings: "calibration_changed",
      startup_duration: null,
    });
    await uploaded(h);
    await vi.advanceTimersByTimeAsync(SETTINGS_CHECK_MS + 50);
    expect(h.state.phase).toBe("failed");
    expect(h.state.failure?.step).toBe("Settings check");
    expect(h.state.failure?.advice).toBe(
      "The meter's calibration bytes changed; do not write anything and report this.",
    );
    expect(h.state.busy).toBe(false);
    await vi.advanceTimersByTimeAsync(20_000);
    expect(h.deps.checkSettings).toHaveBeenCalledTimes(1);
    expect(h.deps.refreshSettings).not.toHaveBeenCalled();
  });

  it("qg6rny: a write whose fresh read found changed calibration fails at once", async () => {
    const h = harness({ ...NEEDS_SAVE, settings: "calibration_changed" });
    await uploaded(h);
    expect(h.state.phase).toBe("failed");
    expect(h.state.failure?.step).toBe("Settings check");
    await vi.advanceTimersByTimeAsync(20_000);
    expect(h.deps.checkSettings).not.toHaveBeenCalled();
  });
});

describe("a fresh read already saved (AC-QG6RNY.4)", () => {
  it("qg6rny: no saving phase, the duration from that read", async () => {
    const h = harness({ ...NEEDS_SAVE, settings: "saved", startup_duration: "3 s" });
    await uploaded(h);
    expect(h.state.phase).toBe("done");
    expect(h.state.startupDuration).toBe("3 s");
    expect(h.state.durationOff).toBe(false);
    await vi.advanceTimersByTimeAsync(20_000);
    expect(h.deps.checkSettings).not.toHaveBeenCalled();
  });
});

describe("stopping and the lock (AC-QG6RNY.5)", () => {
  it("qg6rny: stopChecking cancels the next check, startChecking resumes it", async () => {
    const h = harness();
    await uploaded(h);
    await vi.advanceTimersByTimeAsync(SETTINGS_CHECK_MS + 10);
    expect(h.deps.checkSettings).toHaveBeenCalledTimes(1);
    h.state.stopChecking();
    await vi.advanceTimersByTimeAsync(20_000);
    expect(h.deps.checkSettings).toHaveBeenCalledTimes(1);
    expect(h.state.phase).toBe("saving");
    h.state.startChecking();
    await vi.advanceTimersByTimeAsync(SETTINGS_CHECK_MS + 10);
    expect(h.deps.checkSettings).toHaveBeenCalledTimes(2);
    h.state.stopChecking();
  });

  it("qg6rny: startChecking does nothing outside the saving phase", async () => {
    const h = harness();
    h.state.startChecking();
    await vi.advanceTimersByTimeAsync(20_000);
    expect(h.deps.checkSettings).not.toHaveBeenCalled();
  });

  it("qg6rny: starting twice still checks one at a time", async () => {
    const h = harness();
    await uploaded(h);
    h.state.startChecking();
    h.state.startChecking();
    await vi.advanceTimersByTimeAsync(SETTINGS_CHECK_MS * 3 + 50);
    expect(h.deps.checkSettings.mock.calls.length).toBeLessThanOrEqual(3);
    for (let i = 1; i < h.checkTimes.length; i++) {
      expect(h.checkTimes[i]! - h.checkTimes[i - 1]!).toBeGreaterThanOrEqual(SETTINGS_CHECK_MS);
    }
    h.state.stopChecking();
  });

  it("qg6rny: no other logo write or duration write starts while saving", async () => {
    const h = harness();
    await uploaded(h);
    expect(h.state.phase).toBe("saving");
    expect(h.state.uploadDisabledReason).not.toBeNull();
    expect(h.state.removeDisabledReason).not.toBeNull();
    h.state.askUpload();
    h.state.askRemove();
    expect(h.state.confirming).toBeNull();
    await h.state.confirm(REMOVE_WORD);
    await h.state.setDuration(3);
    expect(h.deps.upload).toHaveBeenCalledTimes(1);
    expect(h.deps.remove).not.toHaveBeenCalled();
    expect(h.deps.writeSettings).not.toHaveBeenCalled();
    h.state.stopChecking();
  });
});
