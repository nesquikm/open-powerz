/**
 * FR V9SK55 (AC .1): the Advanced firmware switch is off by default and persisted like
 * the theme; missing, corrupt or unreadable storage leaves it off.
 */

import { describe, expect, it } from "vitest";
import {
  ADVANCED_FIRMWARE_STORAGE_KEY,
  loadAdvancedFirmware,
  saveAdvancedFirmware,
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
