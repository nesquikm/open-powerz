/**
 * FR QFTCX0: the capability map components read to hide or disable the controls a
 * build cannot offer. The desktop (Tauri and mock) builds have every feature. The web
 * build has them too (FR milestone M_ZFZQE7) except firmware update, file reveal and the
 * recorder's folder, which stay desktop only (an interrupted update in a tab could brick
 * the meter; a page cannot open the file manager or save into a folder unasked). Pure
 * data: no WebUSB code lives here.
 */

import type { UiError } from "../backend";

/** The features a build can switch off. */
export interface Features {
  firmwareUpdate: boolean;
  logoUpload: boolean;
  settingsWrite: boolean;
  recordDelete: boolean;
  trigger: boolean;
  settingsRead: boolean;
  recordsList: boolean;
  recordDownload: boolean;
  /** Reveal a saved file in the file manager and copy its path (AC-R13Y5W.1). */
  fileReveal: boolean;
  /** A folder the recorder saves into without asking (FR AXHK2R); the browser saves a
   * recording as a download (FR EAKFEC, AC .7). */
  recordingFolder: boolean;
}

/** The Tauri and mock builds: everything on. */
export const DESKTOP_FEATURES: Readonly<Features> = Object.freeze({
  firmwareUpdate: true,
  logoUpload: true,
  settingsWrite: true,
  recordDelete: true,
  trigger: true,
  settingsRead: true,
  recordsList: true,
  recordDownload: true,
  fileReveal: true,
  recordingFolder: true,
});

/** The browser build: the safe reads (settings, records list, record download) are on
 * (AC-W8SHX8.7), and so are the settings write over Web Serial (AC-W8SHX9.7), the
 * record delete (AC-W8SHXA.6), the startup logo (AC-W8SHXB.7) and protocol triggering
 * over Web Serial (AC-W8SHXC.7); firmware update, file reveal and the recorder's folder
 * stay in the desktop app (a recording is a download in the browser). */
export const WEB_FEATURES: Readonly<Features> = Object.freeze({
  firmwareUpdate: false,
  logoUpload: true,
  settingsWrite: true,
  recordDelete: true,
  trigger: true,
  settingsRead: true,
  recordsList: true,
  recordDownload: true,
  fileReveal: false,
  recordingFolder: false,
});

/** What a control the web build leaves out says instead. */
export const DESKTOP_ONLY_TEXT = "Desktop app only";

/** Where the desktop app is downloaded. */
export const RELEASES_URL = "https://github.com/nesquikm/open-powerz/releases";

/** The error a command the web build leaves to the desktop app rejects with
 * (AC-QFTCX0.6): a usage error naming where the desktop app is. */
export function desktopOnlyError(): UiError {
  return {
    kind: "usage",
    message: `${DESKTOP_ONLY_TEXT}: get the desktop app from ${RELEASES_URL}.`,
    hint: null,
  };
}

/** What a WebHID session shows (AC-QFTCWZ.3): HID carries ADC snapshots only, so no PD
 * capture and no queue rates. */
export const HID_BASIC_NOTE = "HID: basic readings only";

/** The refusal a HID session gives for `what` (AC-09R2FK.2): HID carries ADC snapshots
 * only, so PD capture and rate changes are not available. */
export function hidOnlyBasicText(what: string): string {
  return `${what} is not available over HID: it gives basic readings only.`;
}
