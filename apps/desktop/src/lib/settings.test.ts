/**
 * FR V9SK55 (AC .1): the Advanced firmware switch is off by default and persisted like
 * the theme; missing, corrupt or unreadable storage leaves it off.
 */

import { describe, expect, it } from "vitest";
import {
  ADVANCED_FIRMWARE_STORAGE_KEY,
  DEFAULT_RECORDER_SETTINGS,
  HIDE_SERIAL_STORAGE_KEY,
  RECORDER_SETTINGS_STORAGE_KEY,
  loadAdvancedFirmware,
  loadHideSerial,
  loadRecorderSettings,
  recorderSettingsError,
  saveAdvancedFirmware,
  saveHideSerial,
  saveRecorderSettings,
  type RecorderSettings,
} from "./settings";

function memoryStorage(initial: Record<string, string> = {}) {
  const data = new Map(Object.entries(initial));
  return {
    data,
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => void data.set(key, value),
  };
}

const throwing = {
  getItem: (): string | null => {
    throw new Error("denied");
  },
  setItem: (): void => {
    throw new Error("denied");
  },
};

describe("Advanced firmware setting", () => {
  it("uses its own open-powerz storage key", () => {
    expect(ADVANCED_FIRMWARE_STORAGE_KEY).toBe("open-powerz.advanced-firmware");
  });

  it("is off when nothing was saved, or without storage", () => {
    expect(loadAdvancedFirmware(memoryStorage())).toBe(false);
    expect(loadAdvancedFirmware(undefined)).toBe(false);
  });

  it("is off for a corrupt value or unreadable storage", () => {
    for (const v of ["", "1", "yes", "TRUE", "on"]) {
      expect(loadAdvancedFirmware(memoryStorage({ [ADVANCED_FIRMWARE_STORAGE_KEY]: v }))).toBe(
        false,
      );
    }
    expect(loadAdvancedFirmware(throwing)).toBe(false);
  });

  it("round-trips on and off", () => {
    const s = memoryStorage();
    saveAdvancedFirmware(true, s);
    expect(s.data.get(ADVANCED_FIRMWARE_STORAGE_KEY)).toBe("true");
    expect(loadAdvancedFirmware(s)).toBe(true);
    saveAdvancedFirmware(false, s);
    expect(loadAdvancedFirmware(s)).toBe(false);
  });

  it("ignores a storage that refuses the write", () => {
    expect(() => saveAdvancedFirmware(true, throwing)).not.toThrow();
    expect(loadAdvancedFirmware(throwing)).toBe(false);
  });
});

describe("Hide serial numbers setting (AC-AXHK2Q.1)", () => {
  it("uses its own open-powerz storage key", () => {
    expect(HIDE_SERIAL_STORAGE_KEY).toBe("open-powerz.hide-serial");
  });

  it("is off by default, for a corrupt value and for unreadable storage", () => {
    expect(loadHideSerial(memoryStorage())).toBe(false);
    expect(loadHideSerial(undefined)).toBe(false);
    expect(loadHideSerial(throwing)).toBe(false);
    for (const v of ["", "1", "yes", "TRUE", "on"]) {
      expect(loadHideSerial(memoryStorage({ [HIDE_SERIAL_STORAGE_KEY]: v }))).toBe(false);
    }
  });

  it("round-trips on and off, across a new load of the same storage", () => {
    const s = memoryStorage();
    saveHideSerial(true, s);
    expect(s.data.get(HIDE_SERIAL_STORAGE_KEY)).toBe("true");
    expect(loadHideSerial(s)).toBe(true);
    saveHideSerial(false, s);
    expect(s.data.get(HIDE_SERIAL_STORAGE_KEY)).toBe("false");
    expect(loadHideSerial(s)).toBe(false);
  });

  it("keeps working when storage refuses the write", () => {
    saveHideSerial(true, throwing);
    expect(loadHideSerial(throwing)).toBe(false);
  });
});

describe("Recorder settings (FR AXHK2R)", () => {
  const custom: RecorderSettings = {
    prefix: "bench",
    folder: "/data/runs",
    autoStop: true,
    belowA: 0.05,
    forSeconds: 30,
    maxRows: 5_000,
  };

  it("uses its own open-powerz storage key", () => {
    expect(RECORDER_SETTINGS_STORAGE_KEY).toMatch(/^open-powerz\./);
    expect(RECORDER_SETTINGS_STORAGE_KEY).not.toBe(ADVANCED_FIRMWARE_STORAGE_KEY);
    expect(RECORDER_SETTINGS_STORAGE_KEY).not.toBe(HIDE_SERIAL_STORAGE_KEY);
  });

  it("defaults to the powerz name, no folder, no auto-stop and 10,000,000 rows (AC-AXHK2R.5)", () => {
    expect(DEFAULT_RECORDER_SETTINGS.prefix).toBe("");
    expect(DEFAULT_RECORDER_SETTINGS.folder).toBeFalsy();
    expect(DEFAULT_RECORDER_SETTINGS.autoStop).toBe(false);
    expect(DEFAULT_RECORDER_SETTINGS.maxRows).toBe(10_000_000);
    expect(recorderSettingsError(DEFAULT_RECORDER_SETTINGS)).toBeNull();
  });

  it("persists every setting (AC-AXHK2R.8)", () => {
    const storage = memoryStorage();
    saveRecorderSettings(custom, storage);
    expect(storage.data.has(RECORDER_SETTINGS_STORAGE_KEY)).toBe(true);
    expect(loadRecorderSettings(storage)).toEqual(custom);
  });

  it("falls back to the defaults with nothing saved or no storage", () => {
    expect(loadRecorderSettings(memoryStorage())).toEqual(DEFAULT_RECORDER_SETTINGS);
    expect(loadRecorderSettings(undefined)).toEqual(DEFAULT_RECORDER_SETTINGS);
    expect(loadRecorderSettings(throwing)).toEqual(DEFAULT_RECORDER_SETTINGS);
  });

  it("falls back to the defaults for a value that is not settings", () => {
    for (const v of ["", "{", "null", "5", "[]", '"bench"']) {
      const storage = memoryStorage({ [RECORDER_SETTINGS_STORAGE_KEY]: v });
      expect(loadRecorderSettings(storage)).toEqual(DEFAULT_RECORDER_SETTINGS);
    }
  });

  it("never loads a value that could not be saved", () => {
    const bad = [
      { ...custom, belowA: 0 },
      { ...custom, prefix: "a/b" },
      { ...custom, maxRows: 0 },
    ];
    for (const value of bad) {
      const storage = memoryStorage({
        [RECORDER_SETTINGS_STORAGE_KEY]: JSON.stringify(value),
      });
      expect(recorderSettingsError(loadRecorderSettings(storage))).toBeNull();
    }
  });

  it("survives storage that cannot be written", () => {
    saveRecorderSettings(custom, throwing);
    saveRecorderSettings(custom, undefined);
    expect(loadRecorderSettings(throwing)).toEqual(DEFAULT_RECORDER_SETTINGS);
  });

  it("accepts good settings", () => {
    expect(recorderSettingsError(custom)).toBeNull();
    expect(recorderSettingsError({ ...custom, prefix: "" })).toBeNull();
    expect(recorderSettingsError({ ...custom, prefix: "bench 2_a.b-c" })).toBeNull();
  });

  it("refuses 0 A, an empty field and a bad wait while auto-stop is on (AC-AXHK2R.3)", () => {
    for (const belowA of [0, -0.1, Number.NaN, Number.POSITIVE_INFINITY, null]) {
      const value = { ...custom, belowA } as RecorderSettings;
      expect(recorderSettingsError(value), `belowA ${belowA}`).toEqual(expect.any(String));
    }
    for (const forSeconds of [0, -1, Number.NaN, null]) {
      const value = { ...custom, forSeconds } as RecorderSettings;
      expect(recorderSettingsError(value), `forSeconds ${forSeconds}`).toEqual(expect.any(String));
    }
  });

  it("refuses a row limit that is no whole number above 0 (AC-AXHK2R.5)", () => {
    for (const maxRows of [0, -5, 1.5, Number.NaN, null]) {
      const value = { ...custom, maxRows } as RecorderSettings;
      expect(recorderSettingsError(value), `maxRows ${maxRows}`).toEqual(expect.any(String));
    }
  });

  it("refuses a prefix that makes a hidden file (review)", () => {
    expect(recorderSettingsError({ ...custom, prefix: ".bench" })).toEqual(expect.any(String));
  });

  it("refuses path separators and what Windows forbids in the prefix (AC-AXHK2R.6)", () => {
    for (const prefix of ["a/b", "a\\b", "a:b", "a*b", "a?b", 'a"b', "a<b", "a>b", "a|b"]) {
      expect(recorderSettingsError({ ...custom, prefix }), prefix).toEqual(expect.any(String));
    }
  });
});
