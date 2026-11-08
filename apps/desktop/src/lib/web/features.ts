/**
 * FR QFTCX0: the capability map components read to hide or disable the controls a
 * build cannot offer. The desktop (Tauri and mock) builds have every feature; the web
 * build leaves out every meter write (AC-QFTCX0.6) and every read outside the safe
 * read set (AC-QFTCX0.7). Pure data: no WebUSB code lives here.
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
});

/** The browser build: no meter writes, and the reads that need more than the safe read
 * set stay in the desktop app. */
export const WEB_FEATURES: Readonly<Features> = Object.freeze({
  firmwareUpdate: false,
  logoUpload: false,
  settingsWrite: false,
  recordDelete: false,
  trigger: false,
  settingsRead: false,
  recordsList: false,
  recordDownload: false,
  fileReveal: false,
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
