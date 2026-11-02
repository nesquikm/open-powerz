import {
  toUiError,
  type ExportProgress,
  type ExportResult,
  type RecordingEnd,
  type RecordingEvent,
  type RecordingOptions,
  type RecordingSnapshot,
  type RecordingSummary,
  type UiError,
} from "./backend";
import type { ConnectionState } from "./format";
import type { MeterNotice } from "./meterNotices.svelte";
import { autoStopMs, DEFAULT_MAX_ROWS, type RecorderSettings } from "./settings";

/** Sample rates the picker offers, in SPS. */
export const RATE_OPTIONS = [2, 10, 50, 1000] as const;

export interface RatePickerState {
  disabled: boolean;
  /** Text explaining why the picker is disabled, or `null` when it is enabled. */
  label: string | null;
}

/** Why the rate cannot change while a recording is open. */
const RATE_LOCKED_WHILE_RECORDING = "Stop the recording to change the rate";

/**
 * How the rate picker looks for a session state: disabled with an explanation
 * when the meter's rate is locked (no authentication), enabled otherwise.
 */
export function ratePicker(
  state: ConnectionState,
  rateSps: number | null,
  recording = false,
): RatePickerState {
  if (state === "rate_locked") {
    return {
      disabled: true,
      // Until enough records arrive to measure it, say so rather than show a number.
      label:
        rateSps === null
          ? "Rate set on the meter (measuring…) — authentication unavailable"
          : `Rate set on the meter: ${rateSps} SPS — authentication unavailable`,
    };
  }
  // The backend refuses a rate change mid-recording: one file, one rate.
  if (recording) return { disabled: true, label: RATE_LOCKED_WHILE_RECORDING };
  return { disabled: false, label: null };
}

/** What a rate key does: change the rate, or refuse while a recording is open. */
export type RateChange = { kind: "set"; sps: number } | { kind: "refused"; notice: string };

/** A rate key (1–4) follows the picker's lock: no rate change mid-recording. */
export function rateChange(sps: number, { recording }: { recording: boolean }): RateChange {
  if (recording) return { kind: "refused", notice: `${RATE_LOCKED_WHILE_RECORDING}.` };
  return { kind: "set", sps };
}

const pad = (n: number): string => String(n).padStart(2, "0");

/** The file name the save dialog proposes: `powerz-YYYYMMDD-HHMMSS.csv` in local time, or
 * a recording's prefix in place of `powerz`. */
export function defaultRecordingName(now: Date, prefix = ""): string {
  const date = `${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(now.getDate())}`;
  const time = `${pad(now.getHours())}${pad(now.getMinutes())}${pad(now.getSeconds())}`;
  return `${prefix || "powerz"}-${date}-${time}.csv`;
}

/** Elapsed recording time as `h:mm:ss` (hours unpadded). */
export function formatRecordingElapsed(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  return `${Math.floor(s / 3600)}:${pad(Math.floor(s / 60) % 60)}:${pad(s % 60)}`;
}

/** `1 row`, `1,234 rows`. */
export function rowsText(rows: number): string {
  return `${rows.toLocaleString("en-US")} ${rows === 1 ? "row" : "rows"}`;
}

const countText = (n: number, unit: string): string => `${n} ${unit}${n === 1 ? "" : "s"}`;

function gapsText(gaps: number): string {
  return gaps === 0 ? "no gaps" : countText(gaps, "gap");
}

/** A long span in the largest unit that reads well: `30 seconds`, `10 minutes`, `3.3 hours`,
 * `3 days`. Each unit stops where its rounded count would reach the next one. */
function spanText(seconds: number): string {
  if (Math.round(seconds) < 60) return countText(Math.round(seconds), "second");
  if (Math.round(seconds / 60) < 60) return countText(Math.round(seconds / 60), "minute");
  const hours = Math.round(seconds / 360) / 10;
  if (hours < 48) return countText(hours, "hour");
  return countText(Math.round(seconds / 86_400), "day");
}

/** How long a chart memory of `capacity` samples lasts at a rate, or that the rate is still
 * unknown (`null` or 0). */
export function memoryDepthText(capacity: number, rateSps: number | null): string {
  if (rateSps === null || rateSps <= 0) return "Chart memory: measuring the rate…";
  return `Chart memory holds about ${spanText(capacity / rateSps)} at this rate`;
}

/** The word before the time in the recording indicator. */
export function recordingWord(paused: boolean): "REC" | "PAUSED" {
  return paused ? "PAUSED" : "REC";
}

/** What the top bar shows while recording: elapsed time and rows written. */
export function recordingIndicator(elapsedMs: number, rows: number, paused = false): string {
  return `${recordingWord(paused)} ${formatRecordingElapsed(elapsedMs)}${recordingRowsSuffix(rows)}`;
}

/** The rows part of the indicator, ` · 1,234 rows` (the top bar hides it visually when narrow). */
export function recordingRowsSuffix(rows: number): string {
  return ` · ${rowsText(rows)}`;
}

/** How the notice opens when a rule ended the recording. */
const RULE_ENDS: Partial<Record<RecordingEnd, string>> = {
  low_current: "Recording ended on low current:",
  row_limit: "Recording ended at the row limit:",
};

/** The notice shown once a recording has ended; names the file when `path` is known. */
export function recordingSummaryText(
  summary: RecordingSummary,
  path: string | null = null,
): string {
  const counts = `${rowsText(summary.rows)} written, ${gapsText(summary.gaps.length)}.`;
  const text = summary.disconnected
    ? `Recording stopped: the meter was unplugged. ${counts}`
    : `${RULE_ENDS[summary.ended] ?? "Recording saved:"} ${counts}`;
  return path === null ? text : `${text} File: ${path}`;
}

/** The notice shown once an export has finished: rows written and the file. */
export function exportNoticeText(result: ExportResult): string {
  return `Exported ${rowsText(result.rows)} to ${result.path}`;
}

/** The file-manager action's label for a platform string (`navigator.platform`). */
export function revealActionLabel(platform: string): string {
  return /mac/i.test(platform) ? "Reveal in Finder" : "Show in folder";
}

/** What the recorder's settings ask of a recording; what they leave at the default is
 * not sent. */
export function recordingOptions(settings: RecorderSettings): RecordingOptions {
  const { folder, autoStop, belowA, forSeconds, maxRows } = settings;
  return {
    ...(folder ? { folder } : {}),
    ...(autoStop ? { below_a: belowA, for_ms: autoStopMs(forSeconds) } : {}),
    ...(maxRows !== DEFAULT_MAX_ROWS ? { max_rows: maxRows } : {}),
  };
}

/** What `Recording.toggle` needs from the app. */
export interface RecordingDeps {
  /** A stream is running that can be recorded. */
  canRecord: boolean;
  /** Starts recording to a default file named after the proposal; answers its path. */
  startRecording: (suggestedName: string) => Promise<string | null>;
  /** Replaces `powerz` in the proposed name. */
  prefix?: string;
  stopRecording: () => Promise<RecordingSummary>;
  /** Opens the save dialog for the finished file and moves it to the choice; `null` when
   * cancelled. Without it a stop keeps the default file (a stop nobody asked to save). */
  saveRecordingAs?: (path: string) => Promise<string | null>;
  now: () => Date;
}

/** What `Recording.togglePause` needs from the app. */
export interface PauseDeps {
  pauseRecording: () => Promise<void>;
  resumeRecording: () => Promise<void>;
}

export type RecordingPhase = "idle" | "starting" | "recording" | "stopping";

/** How a Record button looks: it reads Stop while a recording runs or ends, and takes no
 * press while a start or a stop is on its way, or with nothing to record. */
export function recordButton(
  phase: RecordingPhase,
  canRecord: boolean,
): { label: "Record" | "Stop"; disabled: boolean } {
  return {
    label: phase === "recording" || phase === "stopping" ? "Stop" : "Record",
    disabled: phase === "starting" || phase === "stopping" || (phase === "idle" && !canRecord),
  };
}

/** Reactive state of the CSV recording (Record button / Space). */
export class Recording {
  phase = $state<RecordingPhase>("idle");
  /** File being recorded to. */
  path = $state<string | null>(null);
  rows = $state(0);
  elapsedMs = $state(0);
  /** A pause holds the rows back. */
  paused = $state(false);
  /** Summary of the last recording, until the next one starts. */
  notice = $state<string | null>(null);
  /** File the last recording was saved to, while its notice shows. */
  savedPath = $state<string | null>(null);
  error = $state<UiError | null>(null);
  /** Told what the recording has to say each time that changes, null for nothing. */
  #onNotice: ((notice: MeterNotice | null) => void) | undefined;
  /** A pause or resume is on its way to the backend. */
  #pauseInFlight = false;
  /** How a recording ended before its start answered, to name its file when that comes. */
  #endedStarting: RecordingSummary | null = null;

  constructor(onNotice?: (notice: MeterNotice | null) => void) {
    this.#onNotice = onNotice;
  }

  /** Passes on the notice and the error as they stand now. */
  #publish(): void {
    const { notice, error } = this;
    this.#onNotice?.(
      notice === null && error === null
        ? null
        : {
            tone: error ? "error" : "info",
            message: error?.message ?? null,
            text: [notice, error?.hint].filter(Boolean).join(" "),
            path: this.savedPath,
          },
    );
  }

  /** Elapsed time and rows while recording, `null` otherwise. */
  get indicator(): string | null {
    return this.phase === "recording"
      ? recordingIndicator(this.elapsedMs, this.rows, this.paused)
      : null;
  }

  /** Pause a running recording, or resume a paused one; with none it does nothing. A
   * refused request leaves the state as it was, with the reason in {@link error}. */
  async togglePause(deps: PauseDeps): Promise<void> {
    if (this.phase !== "recording" || this.#pauseInFlight) return;
    const paused = !this.paused;
    this.#pauseInFlight = true;
    this.error = null;
    this.#publish();
    try {
      await (paused ? deps.pauseRecording() : deps.resumeRecording());
      // The recording may have ended while the request was on its way.
      if (this.phase === "recording") this.paused = paused;
    } catch (err) {
      // An end that outran the refusal has said what happened.
      if (this.phase !== "recording") return;
      this.error = toUiError(err);
      this.#publish();
    } finally {
      this.#pauseInFlight = false;
    }
  }

  /** Start when idle, stop when recording; ignored while a start or stop is pending. */
  async toggle(deps: RecordingDeps): Promise<void> {
    if (this.phase === "recording") {
      await this.stop(deps);
      return;
    }
    if (this.phase !== "idle" || !deps.canRecord) return;
    this.phase = "starting";
    this.#endedStarting = null;
    this.notice = null;
    this.savedPath = null;
    this.error = null;
    this.#publish();
    try {
      const path = await deps.startRecording(defaultRecordingName(deps.now(), deps.prefix));
      // The session may have ended while the start was on its way.
      if (this.phase !== "starting") {
        if (path !== null && this.#endedStarting !== null) {
          this.#saved(this.#endedStarting, path);
        }
        return;
      }
      if (path === null) {
        this.phase = "idle";
        return;
      }
      this.path = path;
      this.rows = 0;
      this.elapsedMs = 0;
      this.paused = false;
      this.phase = "recording";
    } catch (err) {
      if (this.phase !== "starting") return;
      this.error = toUiError(err);
      this.phase = "idle";
      this.#publish();
    }
  }

  /** Stops a running recording; with none it does nothing. False when the stop failed,
   * with the reason in {@link error} and the recording still running, and while a start
   * or stop is in flight (that one goes on, this call changes nothing, {@link error}
   * included). Also false when a failure ended the recording during the stop: the file
   * is not complete, and {@link error} and the notice come from that failure. */
  async stop(deps: RecordingDeps): Promise<boolean> {
    if (this.phase === "starting" || this.phase === "stopping") return false;
    if (this.phase !== "recording") return true;
    this.phase = "stopping";
    this.error = null;
    this.#publish();
    let stopped = true;
    try {
      const summary = await deps.stopRecording();
      if (this.phase === "stopping") {
        const path = this.path;
        const file = await this.#chooseFile(deps);
        // An unplug during the dialog named the old place; the file has moved since.
        if (this.phase === "stopping" || this.savedPath === path) this.#saved(summary, file);
      }
    } catch (err) {
      // An unplug can end the recording (with its own notice) while the stop
      // is on its way; the stop's "not connected" is then old news.
      if (this.phase === "stopping") {
        this.error = toUiError(err);
        this.phase = "recording";
        stopped = false;
        this.#publish();
      } else if (this.error !== null) {
        // ...unless what ended it was a failure, whose event outran this rejection.
        stopped = false;
      }
    }
    // An event may already have ended it, and a new recording may have begun.
    if (this.phase === "stopping") this.#end();
    return stopped;
  }

  /** Picks up a recording the backend kept running across a reload (FR VEDDFD):
   * shown as recording, its timer and rows continuing from the backend's. */
  adopt(snapshot: RecordingSnapshot): void {
    this.notice = null;
    this.savedPath = null;
    this.error = null;
    this.path = snapshot.path ?? null;
    this.rows = snapshot.rows;
    this.elapsedMs = snapshot.elapsed_ms;
    this.paused = snapshot.paused ?? false;
    this.phase = "recording";
  }

  handleEvent(event: RecordingEvent): void {
    switch (event.state) {
      case "progress":
        this.rows = event.rows;
        this.elapsedMs = event.elapsed_ms;
        // A progress sent before a pause must not undo the press on its way.
        if (this.phase === "recording" && !this.#pauseInFlight) this.paused = event.paused;
        break;
      case "stopped":
        // A press that lost the race to this end was refused: this says what happened.
        this.error = null;
        // A rule can end a recording before its start has answered with the path.
        if (this.phase === "starting") this.#endedStarting = event;
        this.#saved(event);
        this.#end();
        break;
      case "failed":
        this.error = event.error;
        this.notice = `Recording stopped; ${rowsText(event.rows)} kept.`;
        // The rows before the failure are in the file: keep its reveal and copy actions.
        if (event.rows > 0) this.savedPath = this.path;
        this.#publish();
        this.#end();
        break;
      case "started":
        // A start that timed out can still be applied (AC-AXHK2K.9).
        if (this.phase === "idle") {
          this.adopt({ rows: 0, elapsed_ms: 0 });
          this.#publish();
        }
        break;
    }
  }

  /** The session ended without telling the recording (e.g. the worker
   * failed): stop showing it as running. */
  sessionEnded(): void {
    if (this.phase === "idle") return;
    this.notice = "Recording ended with the session.";
    if (this.rows > 0) this.savedPath = this.path;
    this.#publish();
    this.#end();
  }

  /** Lets go of a running recording without ending it (FR 1HHT03): its meter leaves the
   * full view and records on. Returns where it stood, to {@link adopt} when the meter
   * comes back; null (and nothing changes) when no recording runs. */
  detach(): RecordingSnapshot | null {
    if (this.phase !== "recording") return null;
    const snapshot = {
      rows: this.rows,
      elapsed_ms: this.elapsedMs,
      path: this.path ?? undefined,
      ...(this.paused ? { paused: true } : {}),
    };
    this.#end();
    return snapshot;
  }

  /** Hides the notice and the error. */
  dismiss(): void {
    this.notice = null;
    this.savedPath = null;
    this.error = null;
    this.#publish();
  }

  /** Where the finished file ends up: the place asked for, or the default path
   * when the dialog is cancelled or the move fails (with the reason in {@link error}). */
  async #chooseFile(deps: RecordingDeps): Promise<string | null> {
    const path = this.path;
    if (path === null || deps.saveRecordingAs === undefined) return path;
    try {
      return (await deps.saveRecordingAs(path)) ?? path;
    } catch (err) {
      this.error = toUiError(err);
      this.#publish();
      return path;
    }
  }

  /** The recording's file is complete: the summary names it. */
  #saved(summary: RecordingSummary, path = this.path): void {
    this.savedPath = path;
    this.notice = recordingSummaryText(summary, path);
    this.#publish();
  }

  #end(): void {
    this.phase = "idle";
    this.path = null;
    this.paused = false;
  }
}

/** What `ExportJob.run` needs from the app. */
export interface ExportDeps {
  exportSession: (
    suggestedName: string,
    onProgress: (progress: ExportProgress) => void,
  ) => Promise<ExportResult | null>;
  now: () => Date;
}

/** Reactive state of a session export. */
export class ExportJob {
  running = $state(false);
  done = $state(0);
  total = $state(0);
  /** Rows the last export wrote, `null` when none finished. */
  rows = $state<number | null>(null);
  path = $state<string | null>(null);
  error = $state<UiError | null>(null);

  /** Share of the export done, 0–1. */
  get fraction(): number {
    return this.total > 0 ? this.done / this.total : 0;
  }

  /** Exports the session; a second call while one runs is ignored. */
  async run(deps: ExportDeps): Promise<void> {
    if (this.running) return;
    this.running = true;
    this.error = null;
    this.rows = null;
    this.path = null;
    this.done = 0;
    this.total = 0;
    try {
      const result = await deps.exportSession(defaultRecordingName(deps.now()), (p) => {
        this.done = p.done;
        this.total = p.total;
      });
      if (result !== null) {
        this.rows = result.rows;
        this.path = result.path;
      }
    } catch (err) {
      this.error = toUiError(err);
    } finally {
      this.running = false;
    }
  }

  /** Hides the last export's notice. */
  dismissNotice(): void {
    this.rows = null;
    this.path = null;
  }
}
