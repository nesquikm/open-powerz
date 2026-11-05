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

/** localStorage key of the Show (i) icons switch (FR CW66T1, AC .8). */
export const SHOW_INFO_ICONS_STORAGE_KEY = "open-powerz.show-info-icons";

/** On by default: only a saved `"false"` turns the (i) buttons off. */
export function loadShowInfoIcons(storage: ThemeStorage | undefined): boolean {
  try {
    return storage?.getItem(SHOW_INFO_ICONS_STORAGE_KEY) !== "false";
  } catch {
    return true;
  }
}

export function saveShowInfoIcons(on: boolean, storage: ThemeStorage | undefined): void {
  saveSwitch(SHOW_INFO_ICONS_STORAGE_KEY, on, storage);
}

/** The row limit of a recording unless the user sets another, like the backend's. */
export const DEFAULT_MAX_ROWS = 10_000_000;

/** localStorage key of the recorder's settings (FR AXHK2R). */
export const RECORDER_SETTINGS_STORAGE_KEY = "open-powerz.recorder";

/** What the recorder's options dialog sets. `null` is a field left empty. */
export interface RecorderSettings {
  /** Replaces `powerz` in the file name; empty means `powerz`. */
  prefix: string;
  /** Where files go; `null` keeps the app's folder and the save dialog. */
  folder: string | null;
  /** End a recording when the current stays low. */
  autoStop: boolean;
  /** The current to stay below, A. */
  belowA: number | null;
  /** How long it must stay below, seconds. */
  forSeconds: number | null;
  /** End a recording at this many rows. */
  maxRows: number | null;
}

export const DEFAULT_RECORDER_SETTINGS: RecorderSettings = {
  prefix: "",
  folder: null,
  autoStop: false,
  belowA: 0.05,
  forSeconds: 60,
  maxRows: DEFAULT_MAX_ROWS,
};

/** The auto-stop time the backend takes, ms; an empty field is 0. */
export function autoStopMs(forSeconds: number | null): number {
  return Math.round((forSeconds ?? 0) * 1000);
}

/** A character a file name cannot hold: a path separator, what Windows forbids, or a
 * control character. */
function badNameCharacter(prefix: string): boolean {
  return /[/\\:*?"<>|]/.test(prefix) || [...prefix].some((c) => c < " ");
}

/** Why `s` cannot be saved, in plain words; `null` when it can. The two auto-stop fields
 * only count while auto-stop is on. */
export function recorderSettingsError(s: RecorderSettings): string | null {
  if (badNameCharacter(s.prefix)) {
    return 'The prefix cannot hold / \\ : * ? " < > | or control characters.';
  }
  if (s.prefix.startsWith(".")) return "The prefix cannot start with a dot.";
  if (s.autoStop) {
    if (s.belowA === null || !Number.isFinite(s.belowA) || s.belowA <= 0) {
      return "Enter a current above 0 A for auto-stop.";
    }
    const forMs = autoStopMs(s.forSeconds);
    if (!Number.isSafeInteger(forMs) || forMs < 1) {
      return "Enter a time above 0 seconds for auto-stop.";
    }
  }
  if (s.maxRows === null || !Number.isSafeInteger(s.maxRows) || s.maxRows < 1) {
    return "Enter a row limit of at least 1 row.";
  }
  return null;
}

/** The fields of a saved value that have the right type; the rest are dropped. */
function savedFields(saved: Record<string, unknown>): Partial<RecorderSettings> {
  const { prefix, folder, autoStop, belowA, forSeconds, maxRows } = saved;
  const numberOrNull = (v: unknown): v is number | null => typeof v === "number" || v === null;
  return {
    ...(typeof prefix === "string" ? { prefix } : {}),
    ...(typeof folder === "string" || folder === null ? { folder } : {}),
    ...(typeof autoStop === "boolean" ? { autoStop } : {}),
    ...(numberOrNull(belowA) ? { belowA } : {}),
    ...(numberOrNull(forSeconds) ? { forSeconds } : {}),
    ...(numberOrNull(maxRows) ? { maxRows } : {}),
  };
}

/** The saved settings; the defaults for nothing saved, or for anything that could not
 * have been saved. */
export function loadRecorderSettings(storage: ThemeStorage | undefined): RecorderSettings {
  try {
    const saved: unknown = JSON.parse(storage?.getItem(RECORDER_SETTINGS_STORAGE_KEY) ?? "null");
    if (typeof saved !== "object" || saved === null || Array.isArray(saved)) {
      return DEFAULT_RECORDER_SETTINGS;
    }
    const settings = {
      ...DEFAULT_RECORDER_SETTINGS,
      ...savedFields(saved as Record<string, unknown>),
    };
    return recorderSettingsError(settings) === null ? settings : DEFAULT_RECORDER_SETTINGS;
  } catch {
    return DEFAULT_RECORDER_SETTINGS;
  }
}

/** Persist the settings; storage failures are ignored, as for the switches above. */
export function saveRecorderSettings(
  settings: RecorderSettings,
  storage: ThemeStorage | undefined,
): void {
  try {
    storage?.setItem(RECORDER_SETTINGS_STORAGE_KEY, JSON.stringify(settings));
  } catch {
    // storage unavailable (private mode, quota, denied): keep the in-memory value
  }
}
