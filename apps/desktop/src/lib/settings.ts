import type { ThemeStorage } from "./theme";

/** localStorage key of the Advanced firmware switch (FR V9SK55, AC .1). */
export const ADVANCED_FIRMWARE_STORAGE_KEY = "open-powerz.advanced-firmware";

/** Whether the Advanced firmware switch is on; anything but a saved `"true"` is off. */
export function loadAdvancedFirmware(storage: ThemeStorage | undefined): boolean {
  try {
    return storage?.getItem(ADVANCED_FIRMWARE_STORAGE_KEY) === "true";
  } catch {
    return false;
  }
}

/** Persist the switch; storage failures are ignored (it just won't survive a restart). */
export function saveAdvancedFirmware(on: boolean, storage: ThemeStorage | undefined): void {
  try {
    storage?.setItem(ADVANCED_FIRMWARE_STORAGE_KEY, on ? "true" : "false");
  } catch {
    // storage unavailable (private mode, quota, denied): keep the in-memory value
  }
}
