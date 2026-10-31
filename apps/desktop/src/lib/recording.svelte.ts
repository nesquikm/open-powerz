import {
  toUiError,
  type ExportProgress,
  type ExportResult,
  type RecordingEvent,
  type RecordingSnapshot,
  type RecordingSummary,
  type UiError,
} from "./backend";
import type { ConnectionState } from "./format";

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

/** The file name the save dialog proposes: `powerz-YYYYMMDD-HHMMSS.csv` in local time. */
export function defaultRecordingName(now: Date): string {
  const date = `${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(now.getDate())}`;
  const time = `${pad(now.getHours())}${pad(now.getMinutes())}${pad(now.getSeconds())}`;
  return `powerz-${date}-${time}.csv`;
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

function gapsText(gaps: number): string {
  if (gaps === 0) return "no gaps";
  return `${gaps} ${gaps === 1 ? "gap" : "gaps"}`;
}

/** What the top bar shows while recording: elapsed time and rows written. */
export function recordingIndicator(elapsedMs: number, rows: number): string {
  return `REC ${formatRecordingElapsed(elapsedMs)}${recordingRowsSuffix(rows)}`;
}

/** The rows part of the indicator, ` · 1,234 rows` (the top bar hides it visually when narrow). */
export function recordingRowsSuffix(rows: number): string {
  return ` · ${rowsText(rows)}`;
}

/** The notice shown once a recording has ended; names the file when `path` is known. */
export function recordingSummaryText(
  summary: RecordingSummary,
  path: string | null = null,
): string {
  const counts = `${rowsText(summary.rows)} written, ${gapsText(summary.gaps.length)}.`;
  const text = summary.disconnected
    ? `Recording stopped: the meter was unplugged. ${counts}`
    : `Recording saved: ${counts}`;
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

/** What `Recording.toggle` needs from the app. */
export interface RecordingDeps {
  /** A stream is running that can be recorded. */
  canRecord: boolean;
  /** Starts recording to a default file named after the proposal; answers its path. */
  startRecording: (suggestedName: string) => Promise<string | null>;
  stopRecording: () => Promise<RecordingSummary>;
  /** Opens the save dialog for the finished file and moves it to the choice; `null` when
   * cancelled. Without it a stop keeps the default file (a stop nobody asked to save). */
  saveRecordingAs?: (path: string) => Promise<string | null>;
  now: () => Date;
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
  /** Summary of the last recording, until the next one starts. */
  notice = $state<string | null>(null);
  /** File the last recording was saved to, while its notice shows. */
  savedPath = $state<string | null>(null);
  error = $state<UiError | null>(null);

  /** Elapsed time and rows while recording, `null` otherwise. */
  get indicator(): string | null {
    return this.phase === "recording" ? recordingIndicator(this.elapsedMs, this.rows) : null;
  }

  /** Start when idle, stop when recording; ignored while a start or stop is pending. */
  async toggle(deps: RecordingDeps): Promise<void> {
    if (this.phase === "recording") {
      await this.stop(deps);
      return;
    }
    if (this.phase !== "idle" || !deps.canRecord) return;
    this.phase = "starting";
    this.notice = null;
    this.savedPath = null;
    this.error = null;
    try {
      const path = await deps.startRecording(defaultRecordingName(deps.now()));
      // The session may have ended while the start was on its way.
      if (this.phase !== "starting") return;
      if (path === null) {
        this.phase = "idle";
        return;
      }
      this.path = path;
      this.rows = 0;
      this.elapsedMs = 0;
      this.phase = "recording";
    } catch (err) {
      if (this.phase !== "starting") return;
      this.error = toUiError(err);
      this.phase = "idle";
    }
  }

  /** Stops a running recording; with none it does nothing. False when the stop failed,
   * with the reason in {@link error}. */
  async stop(deps: RecordingDeps): Promise<boolean> {
    if (this.phase !== "recording") return true;
    this.phase = "stopping";
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
    this.phase = "recording";
  }

  handleEvent(event: RecordingEvent): void {
    switch (event.state) {
      case "progress":
        this.rows = event.rows;
        this.elapsedMs = event.elapsed_ms;
        break;
      case "stopped":
        this.#saved(event);
        this.#end();
        break;
      case "failed":
        this.error = event.error;
        this.notice = `Recording stopped; ${rowsText(event.rows)} kept.`;
        // The rows before the failure are in the file: keep its reveal and copy actions.
        if (event.rows > 0) this.savedPath = this.path;
        this.#end();
        break;
      case "started":
        break;
    }
  }

  /** The session ended without telling the recording (e.g. the worker
   * failed): stop showing it as running. */
  sessionEnded(): void {
    if (this.phase === "idle") return;
    this.notice = "Recording ended with the session.";
    if (this.rows > 0) this.savedPath = this.path;
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
    };
    this.#end();
    return snapshot;
  }

  /** Hides the notice and the error. */
  dismiss(): void {
    this.notice = null;
    this.savedPath = null;
    this.error = null;
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
      return path;
    }
  }

  /** The recording's file is complete: the summary names it. */
  #saved(summary: RecordingSummary, path = this.path): void {
    this.savedPath = path;
    this.notice = recordingSummaryText(summary, path);
  }

  #end(): void {
    this.phase = "idle";
    this.path = null;
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
