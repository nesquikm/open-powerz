/**
 * The Startup logo view's state (FR MEHQC0).
 */

import type {
  BuiltinLogoName,
  LogoPreview,
  LogoProgress,
  LogoRead,
  LogoSettingsCheck,
  LogoWritten,
  MeterSettings,
  SettingChanges,
  UiError,
} from "./backend";
import { hidOnlyBasicText } from "./web/features";

/** The word the Upload confirmation asks for. */
export const UPLOAD_WORD = "upload";
/** The word the Remove confirmation asks for. */
export const REMOVE_WORD = "remove";

/** The built-in open-powerz logos the view offers (FR 70X5YX), in listing order. */
export const BUILTIN_LOGOS: readonly { name: BuiltinLogoName; label: string }[] = [
  { name: "surf", label: "Mascot" },
  { name: "surf-wordmark", label: "Mascot with lettering" },
];

/** Least gap between two settings checks while the meter's settings wait for a menu
 * save (FR QG6RNY). */
export const SETTINGS_CHECK_MS = 2000;

/** The `startup_duration` code of each offered startup-screen duration. */
const DURATION_CODES = { 1: 1, 3: 2, 5: 3 } as const;

/** A logo write. */
export type LogoAction = "upload" | "remove";

/** Where the view is. */
export type LogoPhase =
  "idle" | "previewed" | "confirming" | "writing" | "saving" | "done" | "failed";

/** A failed write: the step that failed, the chunk index when a chunk failed, and what
 * to do next. */
export interface LogoFailure {
  step: string;
  chunk: number | null;
  advice: string;
}

/** What the view needs from the backend, for the meter it shows. */
export interface LogoDeps {
  choose(): Promise<string | null>;
  preview(path: string, alpha: boolean): Promise<LogoPreview>;
  /** `onProgress` hears the read's chunks done of the total, where the backend reports
   * them (the browser, FR CJ3QCD). */
  read(full: boolean, onProgress?: (p: LogoProgress) => void): Promise<LogoRead>;
  upload(path: string, alpha: boolean, onProgress: (p: LogoProgress) => void): Promise<LogoWritten>;
  /** The preview of a built-in logo, always transparent (FR 70X5YX); without it no
   * built-in can be picked. */
  previewBuiltin?(name: BuiltinLogoName): Promise<LogoPreview>;
  /** Uploads a built-in logo, always transparent (FR 70X5YX). */
  uploadBuiltin?(
    name: BuiltinLogoName,
    onProgress: (p: LogoProgress) => void,
  ): Promise<LogoWritten>;
  remove(onProgress: (p: LogoProgress) => void): Promise<LogoWritten>;
  writeSettings(changes: SettingChanges): Promise<MeterSettings>;
  /** One read-only check of the settings after a write that needs a menu save. */
  checkSettings(): Promise<LogoSettingsCheck>;
  /** Reads the meter's settings again once they are saved. */
  refreshSettings(): Promise<void>;
}

/** The step a failed settings check names. */
const SETTINGS_CHECK_STEP = "Settings check";
/** What to do when the meter's calibration bytes changed after a logo write. */
const CALIBRATION_ADVICE =
  "The meter's calibration bytes changed; do not write anything and report this.";

/** The live settings check while a write waits for the menu save: checks so far, when
 * the last one answered (`Date.now()` ms) and its error, if it failed. */
export interface SaveCheck {
  checks: number;
  lastAt: number | null;
  error: UiError | null;
}

/** The meter as the view sees it: its model and its settings as read, each null until
 * read, and whether its connection gives basic readings only (HID, FR CJ3QCC). */
export interface LogoMeter {
  model: string | null;
  settings: MeterSettings | null;
  basicReadingsOnly?: boolean;
}

/** The word a confirmation of `action` asks for. */
export function confirmWord(action: LogoAction): string {
  return action === "upload" ? UPLOAD_WORD : REMOVE_WORD;
}

/** The failed step and chunk of a failed write, with advice. */
export function logoFailure(error: UiError, action: LogoAction): LogoFailure {
  const msg = error.message;
  const chunkMatch = /\bchunk (\d+)/i.exec(msg);
  const chunk = chunkMatch ? Number(chunkMatch[1]) : null;
  let step: string;
  if (chunk !== null) step = "Writing a chunk";
  else if (/startup duration|\bflag\b/i.test(msg)) step = "Setting the startup duration";
  else if (/read.?back|read back/i.test(msg)) step = "Read-back check";
  else if (/\bclear\b/i.test(msg)) step = "Clearing the logo";
  else step = action === "upload" ? "Uploading the logo" : "Removing the logo";
  return {
    step,
    chunk,
    advice: "Run Upload or Remove again to leave the meter in a known state.",
  };
}

export class StartupLogoState {
  phase = $state<LogoPhase>("idle");
  path = $state<string | null>(null);
  /** The built-in logo picked in place of a file (FR 70X5YX). */
  builtin = $state<BuiltinLogoName | null>(null);
  alpha = $state(false);
  preview = $state<LogoPreview | null>(null);
  confirming = $state<LogoAction | null>(null);
  action = $state<LogoAction | null>(null);
  progress = $state<LogoProgress | null>(null);
  written = $state<LogoWritten | null>(null);
  failure = $state<LogoFailure | null>(null);
  error = $state<UiError | null>(null);
  area = $state<LogoRead | null>(null);
  reading = $state(false);
  /** The running read's chunks done of the total, while it reports them (FR CJ3QCD). */
  readProgress = $state<LogoProgress | null>(null);
  /** The settings check of the `saving` phase (FR QG6RNY). */
  saveCheck = $state<SaveCheck>({ checks: 0, lastAt: null, error: null });
  /** The next settings check, while checking. */
  #checkTimer: ReturnType<typeof setTimeout> | null = null;
  /** A settings check is in flight. */
  #checkInFlight = false;
  /** stopChecking was called and startChecking not since. */
  #stopped = false;

  /** A duration write in flight. */
  durationWriting = $state(false);
  /** Why the last duration write failed; also shown as `error`. */
  durationError = $state<UiError | null>(null);
  /** The settings a duration write read back, while the meter's settings are still the
   * block they replaced. */
  #afterWrite = $state<{ before: MeterSettings | null; after: MeterSettings } | null>(null);
  /** The duration the last logo write's saved settings read, while the meter's settings
   * are still the ones read before it (FR QG6RNY). */
  #writtenDuration = $state<{ before: MeterSettings | null; value: string } | null>(null);

  #deps: LogoDeps;
  #meter: () => LogoMeter;

  constructor(deps: LogoDeps, meter: () => LogoMeter) {
    this.#deps = deps;
    this.#meter = meter;
  }

  /** The meter's settings, or what the last duration write read back until they are
   * read again. */
  get #settings(): MeterSettings | null {
    const now = this.#meter().settings;
    const w = this.#afterWrite;
    return w !== null && w.before === now ? w.after : now;
  }

  /** A write is running: it cannot be stopped, and every control that sends to the
   * meter is refused until it ends. */
  get busy(): boolean {
    return this.phase === "writing" || this.phase === "saving";
  }
  /** Why neither write is allowed on this meter yet: the model and the settings must be
   * read, and the logo is a KM003C feature. */
  get #meterDisabledReason(): string | null {
    const meter = this.#meter();
    if (meter.basicReadingsOnly === true) return hidOnlyBasicText("The startup logo");
    if (meter.model === null) return "The meter's model has not been read yet.";
    // HID sessions report the USB product name ("POWER-Z KM003C"); match the model token.
    if (!meter.model.split(/\s+/).includes("KM003C"))
      return `The startup logo needs a KM003C; this meter is a ${meter.model}.`;
    if (meter.settings === null) return "The meter's settings have not been read yet.";
    return null;
  }
  get uploadDisabledReason(): string | null {
    if (this.busy) return "A logo write is running.";
    const meter = this.#meterDisabledReason;
    if (meter !== null) return meter;
    if (this.preview === null) return "Choose an image first.";
    return null;
  }
  get removeDisabledReason(): string | null {
    if (this.busy) return "A logo write is running.";
    return this.#meterDisabledReason;
  }
  /** The startup-screen duration as read, e.g. `off` or `3 s`; null until read. */
  get startupDuration(): string | null {
    const w = this.written;
    if (w !== null && this.phase === "saving") return w.duration_set;
    const d = this.#writtenDuration;
    if (d !== null && d.before === this.#meter().settings) return d.value;
    return this.#settings?.fields.find((f) => f.key === "startup_logo")?.value ?? null;
  }
  /** What the Upload confirmation says about the startup duration (AC-QG6RNY.1). */
  get uploadConfirmText(): string {
    return `The startup duration goes from ${this.startupDuration ?? "unknown"} to 3 s.`;
  }
  /** What the Remove confirmation says about the startup duration (AC-QG6RNY.1). */
  get removeConfirmText(): string {
    return "The startup screen is turned off (startup duration off).";
  }
  /** The startup screen is off, so an uploaded logo will not show. */
  get durationOff(): boolean {
    return this.startupDuration === "off";
  }

  async choose(): Promise<void> {
    if (this.busy) return;
    const path = await this.#deps.choose();
    if (path === null) return;
    this.path = path;
    this.builtin = null;
    await this.#loadPreview();
  }

  /** Picks built-in logo `name` in place of a file and previews it (FR 70X5YX); a
   * built-in is always transparent. */
  async useBuiltin(name: BuiltinLogoName): Promise<void> {
    const previewBuiltin = this.#deps.previewBuiltin;
    if (this.busy || previewBuiltin === undefined) return;
    this.builtin = name;
    this.path = null;
    this.alpha = true;
    await this.#show(() => previewBuiltin(name));
  }

  /** Why the opaque mode is off: a built-in logo is always transparent (AC-70X5YX.6). */
  get opaqueDisabledReason(): string | null {
    return this.builtin === null ? null : "A built-in logo always uses the transparent mode.";
  }

  async setAlpha(alpha: boolean): Promise<void> {
    if (this.busy || (!alpha && this.opaqueDisabledReason !== null)) return;
    this.alpha = alpha;
    if (this.path !== null) await this.#loadPreview();
  }

  /** Previews the chosen file in the chosen mode; a refused file shows its error. */
  async #loadPreview(): Promise<void> {
    const path = this.path;
    if (path === null) return;
    await this.#show(() => this.#deps.preview(path, this.alpha));
  }

  /** Shows the preview `load` resolves to; a refused one shows its error. */
  async #show(load: () => Promise<LogoPreview>): Promise<void> {
    this.error = null;
    try {
      this.preview = await load();
      this.phase = "previewed";
    } catch (e) {
      this.preview = null;
      this.error = e as UiError;
      this.phase = "idle";
    }
  }
  /** Opens the Upload confirmation when Upload is allowed. */
  askUpload(): void {
    if (this.busy || this.uploadDisabledReason !== null) return;
    this.#ask("upload");
  }

  /** Opens the Remove confirmation when Remove is allowed. */
  askRemove(): void {
    if (this.busy || this.removeDisabledReason !== null) return;
    this.#ask("remove");
  }

  #ask(action: LogoAction): void {
    this.confirming = action;
    this.phase = "confirming";
  }

  /** Closes the confirmation, sending nothing. */
  cancelConfirm(): void {
    this.confirming = null;
    this.phase = this.preview === null ? "idle" : "previewed";
  }

  /** Starts the confirmed write once `typed` is the confirmation's word; anything else
   * sends nothing. Resolves once the write has started, not when it ends. */
  async confirm(typed: string): Promise<void> {
    const action = this.confirming;
    if (this.busy || action === null || typed !== confirmWord(action)) return;
    if (action === "upload" && this.path === null && this.builtin === null) return;
    this.confirming = null;
    void this.#write(action);
  }

  /** Runs one logo write to its end. */
  async #write(action: LogoAction): Promise<void> {
    this.action = action;
    this.phase = "writing";
    this.progress = null;
    this.written = null;
    this.#writtenDuration = null;
    this.failure = null;
    this.error = null;
    const onProgress = (p: LogoProgress) => (this.progress = p);
    try {
      this.written = await this.#send(action, onProgress);
      // The logo area changed: the earlier read no longer describes it (FR CJ3QCD).
      this.area = null;
      this.#afterWritten(this.written);
    } catch (e) {
      this.error = e as UiError;
      this.failure = logoFailure(this.error, action);
      this.phase = "failed";
    }
  }
  /** Sends `action`: an upload of the chosen file or built-in logo, or Remove. */
  #send(action: LogoAction, onProgress: (p: LogoProgress) => void): Promise<LogoWritten> {
    if (action === "upload" && this.path !== null)
      return this.#deps.upload(this.path, this.alpha, onProgress);
    if (action === "upload" && this.builtin !== null && this.#deps.uploadBuiltin)
      return this.#deps.uploadBuiltin(this.builtin, onProgress);
    if (action === "remove") return this.#deps.remove(onProgress);
    // An upload with no source it can send must never fall through to a Remove.
    const error: UiError = { kind: "usage", message: "No logo to upload.", hint: null };
    return Promise.reject(error);
  }

  /** Where a finished write goes by its fresh settings read (FR QG6RNY). */
  #afterWritten(written: LogoWritten): void {
    if (written.settings === "saved") {
      this.#keepDuration(written.startup_duration);
      this.phase = "done";
    } else if (written.settings === "calibration_changed") {
      this.#calibrationChanged();
    } else {
      this.saveCheck = { checks: 0, lastAt: null, error: null };
      this.phase = "saving";
      this.startChecking();
    }
  }

  #keepDuration(value: string | null): void {
    this.#writtenDuration = value === null ? null : { before: this.#meter().settings, value };
  }

  #calibrationChanged(): void {
    this.failure = { step: SETTINGS_CHECK_STEP, chunk: null, advice: CALIBRATION_ADVICE };
    this.phase = "failed";
  }

  /** Starts (or resumes) the settings check while the phase is `saving`; one check at
   * a time, at least {@link SETTINGS_CHECK_MS} apart. */
  startChecking(): void {
    if (this.phase !== "saving") return;
    this.#stopped = false;
    if (this.#checkTimer !== null || this.#checkInFlight) return;
    this.#checkTimer = setTimeout(() => void this.#check(), SETTINGS_CHECK_MS);
  }

  /** A new connection (FR CJ3QCC): the previous session's error and logo read no
   * longer describe this meter. */
  resetSession(): void {
    this.error = null;
    this.durationError = null;
    this.area = null;
  }

  /** Cancels the next settings check; the phase stays `saving`. */
  stopChecking(): void {
    this.#stopped = true;
    if (this.#checkTimer !== null) clearTimeout(this.#checkTimer);
    this.#checkTimer = null;
  }

  async #check(): Promise<void> {
    this.#checkTimer = null;
    if (this.phase !== "saving") return;
    this.#checkInFlight = true;
    let result: LogoSettingsCheck | null = null;
    let error: UiError | null = null;
    try {
      result = await this.#deps.checkSettings();
    } catch (e) {
      error = e as UiError;
    } finally {
      this.#checkInFlight = false;
    }
    this.saveCheck = { checks: this.saveCheck.checks + 1, lastAt: Date.now(), error };
    if (this.phase !== "saving") return;
    if (result?.settings === "saved") {
      if (this.written !== null) {
        this.written = { ...this.written, startup_duration: result.startup_duration };
      }
      this.#keepDuration(result.startup_duration);
      this.phase = "done";
      void this.#deps.refreshSettings().catch(() => {});
      return;
    }
    if (result?.settings === "calibration_changed") {
      this.#calibrationChanged();
      return;
    }
    // Stopped while the check was in flight: stay stopped until startChecking.
    if (this.#stopped) return;
    this.startChecking();
  }

  /** Reads what the meter's logo area holds now; `full` also fetches the stored logo's
   * pixels so the view can show it. */
  async read(full = false): Promise<void> {
    if (this.busy || this.reading) return;
    this.reading = true;
    this.readProgress = null;
    this.error = null;
    try {
      // Only the stored logo's read is long enough to report its chunks (FR CJ3QCD).
      this.area = full
        ? await this.#deps.read(full, (p) => (this.readProgress = p))
        : await this.#deps.read(full);
    } catch (e) {
      this.error = e as UiError;
    } finally {
      this.reading = false;
      this.readProgress = null;
    }
  }
  /** Turns the startup screen on for `seconds` through the settings write
   * (`startup_duration` 1, 2 or 3), then shows the settings as read back. */
  async setDuration(seconds: 1 | 3 | 5): Promise<void> {
    if (this.busy || this.durationWriting) return;
    const code = DURATION_CODES[seconds];
    const before = this.#meter().settings;
    this.durationWriting = true;
    this.error = null;
    this.durationError = null;
    try {
      const after = await this.#deps.writeSettings({ startup_duration: code });
      this.#afterWrite = { before, after };
      this.#writtenDuration = null;
    } catch (e) {
      this.error = e as UiError;
      this.durationError = this.error;
    } finally {
      this.durationWriting = false;
    }
  }
}
