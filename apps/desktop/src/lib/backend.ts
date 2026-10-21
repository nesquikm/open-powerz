/** Shapes the Tauri backend sends to the UI (mirrors `src-tauri/src/{batch,worker,devices,ui_error}.rs`). */

import { Channel, invoke, isTauri } from "@tauri-apps/api/core";
import { MockBackend } from "./mockBackend";

/** The scripted demo backend, when the web build was made with `VITE_BACKEND=mock`. */
const mock: MockBackend | null = import.meta.env.VITE_BACKEND === "mock" ? new MockBackend() : null;

/** Whether a backend is there to talk to: the Tauri app, or the mock build. */
export function backendAvailable(): boolean {
  return mock !== null || isTauri();
}

/** One measurement. */
export interface Sample {
  t_ms: number;
  vbus_v: number;
  ibus_a: number;
  cc1_v: number;
  cc2_v: number;
  dp_v: number;
  dm_v: number;
}

/** A batch of samples plus the latest temperature (null when not reported). */
export interface Batch {
  samples: Sample[];
  temp_c: number | null;
  /** Statistics since the session start or the last reset, as of this batch. */
  stats: StatsSnapshot;
}

export type ErrorKind =
  | "permission_denied"
  | "device_busy"
  | "timeout"
  | "protocol"
  | "disconnected"
  | "usage"
  | "internal";

/** A plain-language error. */
export interface UiError {
  kind: ErrorKind;
  message: string;
  hint: string | null;
}

const ERROR_KINDS: readonly ErrorKind[] = [
  "permission_denied",
  "device_busy",
  "timeout",
  "protocol",
  "disconnected",
  "usage",
  "internal",
];

function isUiError(value: unknown): value is UiError {
  if (typeof value !== "object" || value === null) return false;
  const v = value as Record<string, unknown>;
  return (
    typeof v.kind === "string" &&
    (ERROR_KINDS as readonly string[]).includes(v.kind) &&
    typeof v.message === "string" &&
    (v.hint === null || v.hint === undefined || typeof v.hint === "string")
  );
}

/** What the banner says about a failure that did not come from the backend. */
export const INTERNAL_MESSAGE = "Something went wrong inside the app.";

/** Turns a rejected invoke payload (a backend UiError) or any other thrown value into a
 * UiError. Anything else is a bug: its raw text goes to the console, never to the banner. */
export function toUiError(err: unknown): UiError {
  if (isUiError(err)) return { kind: err.kind, message: err.message, hint: err.hint ?? null };
  console.error("open-powerz: unexpected error", err);
  return { kind: "internal", message: INTERNAL_MESSAGE, hint: null };
}

/** A meter (or demo device) the UI can connect to. */
export interface DeviceListing {
  id: string;
  label: string;
  model: string;
  serial: string | null;
  bus: string | null;
  address: number | null;
  demo_scenario: string | null;
}

export interface ConnectedInfo {
  model: string;
  firmware_version: string;
  serial: string;
  rate_sps: number | null;
  rate_locked: boolean;
  session_id: number;
}

export type Status =
  | ({ state: "connected" } & ConnectedInfo)
  | { state: "rate"; rate_sps: number; rate_locked: boolean }
  | { state: "disconnected"; error: UiError | null };

export interface Gap {
  kind: "stream" | "outage";
  from_ms: number;
  to_ms: number;
}

/** A stretch of session time the history holds no samples for: between the last
 * sample of one measuring run and the first of the next (an unplug, a disconnect, a
 * clock jump). */
export interface HistoryGap {
  from_ms: number;
  to_ms: number;
}

/** What a finished recording wrote. */
export interface RecordingSummary {
  rows: number;
  gaps: Gap[];
  /** The recording ended because the meter went away. */
  disconnected: boolean;
}

/** Recording state changes (mirrors `src-tauri/src/recording.rs`). */
export type RecordingEvent =
  | { state: "started" }
  | { state: "progress"; rows: number; elapsed_ms: number }
  | ({ state: "stopped" } & RecordingSummary)
  | { state: "failed"; rows: number; error: UiError };

/** How far an export has got. */
export interface ExportProgress {
  done: number;
  total: number;
}

/** Where an export went and how many rows it wrote. */
export interface ExportResult {
  path: string;
  rows: number;
}

/** Min, max and mean of one quantity. */
export interface MinMaxAvg {
  min: number;
  max: number;
  avg: number;
}

/** Running statistics since the session start or the last reset (mirrors `src-tauri/src/stats.rs`). */
export interface StatsSnapshot {
  elapsed_ms: number;
  samples: number;
  charge_ah: number;
  energy_wh: number;
  vbus: MinMaxAvg | null;
  ibus: MinMaxAvg | null;
  power: MinMaxAvg | null;
}

/** A chart series in the session history. */
export type Series = "vbus" | "ibus" | "dp" | "dm" | "cc1" | "cc2";

/** The min and max of one series over one time bucket (mirrors `src-tauri/src/history.rs`). */
export interface Bucket {
  t_ms: number;
  min: number;
  max: number;
}

/** The buckets of one series. */
export interface SeriesBuckets {
  series: Series;
  buckets: Bucket[];
}

/** One {@link SeriesBuckets} per requested series, in the requested order. */
export type BucketsFor<S extends readonly Series[]> = { [K in keyof S]: SeriesBuckets };

/** Every event the backend sends over the session channel. */
export type BackendEvent =
  | { type: "batch"; data: Batch }
  | { type: "gap"; data: Gap }
  | { type: "status"; data: Status }
  | { type: "error"; data: UiError }
  | { type: "recording"; data: RecordingEvent };

/** Lists the meters and demo devices available right now. */
export function listDevices(): Promise<DeviceListing[]> {
  if (mock) return mock.listDevices();
  return invoke<DeviceListing[]>("list_devices");
}

/** Subscribes to hotplug: `onChange` receives the full device list on every change.
 * Resolves to an unsubscribe function; the backend's watcher is replaced by the next
 * subscription and stops by itself once the window is gone. */
export async function watchDevices(
  onChange: (devices: DeviceListing[]) => void,
): Promise<() => void> {
  if (mock) return mock.watchDevices(onChange);
  let deliver = onChange;
  const channel = new Channel<DeviceListing[]>();
  channel.onmessage = (devices) => deliver(devices);
  await invoke("watch_devices", { onChange: channel });
  return () => {
    deliver = () => {};
  };
}

/** Opens a session on `deviceId`; every session event goes to `onEvent`. */
export async function connect(
  deviceId: string,
  rateSps: number,
  onEvent: (event: BackendEvent) => void,
): Promise<void> {
  if (mock) return mock.connect(deviceId, rateSps, onEvent);
  const channel = new Channel<BackendEvent>();
  channel.onmessage = onEvent;
  await invoke("connect", { deviceId, rateSps, onEvent: channel });
}

/** Ends the current session. */
export async function disconnect(): Promise<void> {
  if (mock) return mock.disconnect();
  await invoke("disconnect");
}

/** Changes the queue rate of the current session. */
export async function setRate(rateSps: number): Promise<void> {
  if (mock) return mock.setRate(rateSps);
  await invoke("set_rate", { rateSps });
}

/** Opens the save dialog proposing `suggestedName` and starts recording to the chosen
 * file. Resolves to the path, or `null` when the dialog was cancelled. */
export async function startRecording(suggestedName: string): Promise<string | null> {
  if (mock) return mock.startRecording(suggestedName);
  return invoke<string | null>("start_recording", { suggestedName });
}

/** Stops the recording; resolves to its row count and gaps. */
export async function stopRecording(): Promise<RecordingSummary> {
  if (mock) return mock.stopRecording();
  return invoke<RecordingSummary>("stop_recording");
}

/** Opens the save dialog proposing `suggestedName` and exports the session so far,
 * reporting progress to `onProgress`. Resolves to `null` when the dialog was cancelled. */
export async function exportSession(
  suggestedName: string,
  onProgress: (progress: ExportProgress) => void,
): Promise<ExportResult | null> {
  if (mock) return mock.exportSession(suggestedName, onProgress);
  const channel = new Channel<ExportProgress>();
  channel.onmessage = onProgress;
  return invoke<ExportResult | null>("export_session", { suggestedName, onProgress: channel });
}

/** Statistics of the open session since its start or the last reset. */
export async function stats(): Promise<StatsSnapshot> {
  if (mock) return mock.stats();
  return invoke<StatsSnapshot>("stats");
}

/** Min / max buckets of the open session's history over `t0Ms..=t1Ms`, at most
 * `buckets` per series. */
export async function historyRange<const S extends readonly Series[]>(
  series: S,
  t0Ms: number,
  t1Ms: number,
  buckets: number,
): Promise<BucketsFor<S>> {
  if (mock) return mock.historyRange(series, t0Ms, t1Ms, buckets);
  return invoke<BucketsFor<S>>("history_range", { series, t0Ms, t1Ms, buckets });
}

/** The gaps between measuring runs of the open session's history that overlap
 * `t0Ms..=t1Ms`, oldest first. */
export async function historyGaps(t0Ms: number, t1Ms: number): Promise<HistoryGap[]> {
  if (mock) return mock.historyGaps(t0Ms, t1Ms);
  return invoke<HistoryGap[]>("history_gaps", { t0Ms, t1Ms });
}

/** Zeroes the session statistics; they count again from the next sample. */
export async function resetStats(): Promise<void> {
  if (mock) return mock.resetStats();
  await invoke("reset_stats");
}

/** Discards the session history; the stream (and any recording) goes on. */
export async function clearHistory(): Promise<void> {
  if (mock) return mock.clearHistory();
  await invoke("clear_history");
}

/** App and protocol-library versions for the About dialog. */
export interface AppInfo {
  appVersion: string;
  protocolVersion: string;
}

/** Reads the app and protocol-library versions from the backend. */
export async function appInfo(): Promise<AppInfo> {
  if (mock) return mock.appInfo();
  const info = await invoke<{ app_version: string; protocol_version: string }>("app_info");
  return { appVersion: info.app_version, protocolVersion: info.protocol_version };
}
