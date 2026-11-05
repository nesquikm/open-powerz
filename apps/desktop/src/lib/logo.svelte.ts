/**
 * The Startup logo view's state (FR MEHQC0).
 */

import type {
  LogoPreview,
  LogoProgress,
  LogoRead,
  LogoWritten,
  MeterSettings,
  SettingChanges,
  UiError,
} from "./backend";

/** The word the Upload confirmation asks for. */
export const UPLOAD_WORD = "upload";
/** The word the Remove confirmation asks for. */
export const REMOVE_WORD = "remove";

/** The `startup_duration` code of each offered startup-screen duration. */
const DURATION_CODES = { 1: 1, 3: 2, 5: 3 } as const;

/** A logo write. */
export type LogoAction = "upload" | "remove";

/** Where the view is. */
export type LogoPhase = "idle" | "previewed" | "confirming" | "writing" | "done" | "failed";

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
  read(full: boolean): Promise<LogoRead>;
  upload(path: string, alpha: boolean, onProgress: (p: LogoProgress) => void): Promise<LogoWritten>;
  remove(onProgress: (p: LogoProgress) => void): Promise<LogoWritten>;
  writeSettings(changes: SettingChanges): Promise<MeterSettings>;
}

/** The meter as the view sees it: its model and its settings as read, each null until
 * read. */
export interface LogoMeter {
  model: string | null;
  settings: MeterSettings | null;
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
  else if (/\bflag\b/i.test(msg)) step = "Setting the logo flag";
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

  /** A duration write in flight. */
  durationWriting = $state(false);
  /** Why the last duration write failed; also shown as `error`. */
  durationError = $state<UiError | null>(null);
  /** The settings a duration write read back, while the meter's settings are still the
   * block they replaced. */
  #afterWrite = $state<{ before: MeterSettings | null; after: MeterSettings } | null>(null);

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
    return this.phase === "writing";
  }
  /** Why neither write is allowed on this meter yet: the model and the settings must be
   * read, and the logo is a KM003C feature. */
  get #meterDisabledReason(): string | null {
    const meter = this.#meter();
    if (meter.model === null) return "The meter's model has not been read yet.";
    if (meter.model !== "KM003C")
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
    return this.#settings?.fields.find((f) => f.key === "startup_logo")?.value ?? null;
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
    await this.#loadPreview();
  }

  async setAlpha(alpha: boolean): Promise<void> {
    if (this.busy) return;
    this.alpha = alpha;
    if (this.path !== null) await this.#loadPreview();
  }

  /** Previews the chosen file in the chosen mode; a refused file shows its error. */
  async #loadPreview(): Promise<void> {
    const path = this.path;
    if (path === null) return;
    this.error = null;
    try {
      this.preview = await this.#deps.preview(path, this.alpha);
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
    if (action === "upload" && this.path === null) return;
    this.confirming = null;
    void this.#write(action);
  }

  /** Runs one logo write to its end. */
  async #write(action: LogoAction): Promise<void> {
    this.action = action;
    this.phase = "writing";
    this.progress = null;
    this.written = null;
    this.failure = null;
    this.error = null;
    const onProgress = (p: LogoProgress) => (this.progress = p);
    try {
      this.written =
        action === "upload" && this.path !== null
          ? await this.#deps.upload(this.path, this.alpha, onProgress)
          : await this.#deps.remove(onProgress);
      this.phase = "done";
    } catch (e) {
      this.error = e as UiError;
      this.failure = logoFailure(this.error, action);
      this.phase = "failed";
    }
  }
  /** Reads what the meter's logo area holds now; `full` also fetches the stored logo's
   * pixels so the view can show it. */
  async read(full = false): Promise<void> {
    if (this.busy || this.reading) return;
    this.reading = true;
    this.error = null;
    try {
      this.area = await this.#deps.read(full);
    } catch (e) {
      this.error = e as UiError;
    } finally {
      this.reading = false;
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
    } catch (e) {
      this.error = e as UiError;
      this.durationError = this.error;
    } finally {
      this.durationWriting = false;
    }
  }
}
