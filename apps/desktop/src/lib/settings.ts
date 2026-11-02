import type { ThemeStorage } from "./theme";

/** Whether the switch saved under `key` is on; anything but a saved `"true"` is off. */
function loadSwitch(key: string, storage: ThemeStorage | undefined): boolean {
  try {
    return storage?.getItem(key) === "true";
  } catch {
    return false;
  }
}

/** Persist a switch; storage failures are ignored (it just won't survive a restart). */
function saveSwitch(key: string, on: boolean, storage: ThemeStorage | undefined): void {
  try {
    storage?.setItem(key, on ? "true" : "false");
  } catch {
    // storage unavailable (private mode, quota, denied): keep the in-memory value
  }
}

/** localStorage key of the Advanced firmware switch (FR V9SK55, AC .1). */
export const ADVANCED_FIRMWARE_STORAGE_KEY = "open-powerz.advanced-firmware";

export function loadAdvancedFirmware(storage: ThemeStorage | undefined): boolean {
  return loadSwitch(ADVANCED_FIRMWARE_STORAGE_KEY, storage);
}

export function saveAdvancedFirmware(on: boolean, storage: ThemeStorage | undefined): void {
  saveSwitch(ADVANCED_FIRMWARE_STORAGE_KEY, on, storage);
}

/** localStorage key of the Hide serial numbers switch (FR AXHK2Q, AC .1). */
export const HIDE_SERIAL_STORAGE_KEY = "open-powerz.hide-serial";

export function loadHideSerial(storage: ThemeStorage | undefined): boolean {
  return loadSwitch(HIDE_SERIAL_STORAGE_KEY, storage);
}

export function saveHideSerial(on: boolean, storage: ThemeStorage | undefined): void {
  saveSwitch(HIDE_SERIAL_STORAGE_KEY, on, storage);
}
