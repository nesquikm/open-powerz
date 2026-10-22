/** Shapes the Tauri backend sends to the UI (mirrors `src-tauri/src/{batch,worker,devices,ui_error}.rs`). */

import { Channel, invoke, isTauri } from "@tauri-apps/api/core";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { MockBackend } from "./mockBackend";

/** The scripted demo backend, when the web build was made with `VITE_BACKEND=mock`. */
const mock: MockBackend | null = import.meta.env.VITE_BACKEND === "mock" ? new MockBackend() : null;

// The mock build exposes itself so the UI suite can read what it recorded (reveal requests).
if (mock && typeof window !== "undefined") {
  (window as unknown as Record<string, unknown>).__OPEN_POWERZ_MOCK__ = mock;
}

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
  | { state: "disconnected"; error: UiError | null }
  /** The meter went away on its own; the app waits for this serial to come back. */
  | { state: "waiting"; serial: string };

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

/** A stored channel of the session history. */
export type Series = "vbus" | "ibus" | "dp" | "dm" | "cc1" | "cc2";

/** A chart series the session history answers: the stored channels plus Power
 * (VBUS × IBUS of each record). */
export type HistorySeries = Series | "power";

/** The min and max of one series over one time bucket (mirrors `src-tauri/src/history.rs`). */
export interface Bucket {
  t_ms: number;
  min: number;
  max: number;
}

/** The buckets of one series. */
export interface SeriesBuckets {
  series: HistorySeries;
  buckets: Bucket[];
}

/** One {@link SeriesBuckets} per requested series, in the requested order. */
export type BucketsFor<S extends readonly HistorySeries[]> = { [K in keyof S]: SeriesBuckets };

/** Who sent a PD message (mirrors `src-tauri/src/pd_capture.rs`). */
export type PdDirection = "source_to_sink" | "sink_to_source" | "cable";

/** One row of the PD packet table: a message or an event (FR TGE83C). The message
 * fields are null on event rows. */
export interface PdRow {
  /** Record index from 0, in capture order. */
  index: number;
  /** Ms since the capture start. */
  elapsed_ms: number;
  /** The record's time on the measurement samples' `t_ms` base (FR TGE83D), for the
   * chart. Optional: rows of older fixtures lack it. */
  session_ms?: number;
  kind: "message" | "event";
  sop: string | null;
  name: string;
  message_id: number | null;
  direction: PdDirection | null;
  objects: number | null;
  spec_revision: string | null;
  vbus_v: number;
  ibus_a: number;
  summary: string;
}

/** One node of a PD record's field tree, with the `[start, end)` bytes of the record's
 * raw bytes it covers. */
export interface PdField {
  name: string;
  meaning: string;
  raw: number;
  bytes: [number, number];
  children: PdField[];
}

/** The raw bytes and field tree of one PD record. */
export interface PdDetails {
  index: number;
  raw: number[];
  fields: PdField[];
}

/** One PD status block (FR TGE83D): VBUS, IBUS, CC1 and CC2 at `session_ms`, on the
 * measurement samples' `t_ms` base. */
export interface PdStatusSample {
  session_ms: number;
  vbus_v: number;
  /** Signed. */
  ibus_a: number;
  cc1_v: number;
  cc2_v: number;
}

/** Every event the backend sends over the session channel. */
export type BackendEvent =
  | { type: "batch"; data: Batch }
  | { type: "gap"; data: Gap }
  | { type: "status"; data: Status }
  | { type: "error"; data: UiError }
  | { type: "recording"; data: RecordingEvent }
  | { type: "pd_state"; data: { capturing: boolean } }
  | { type: "pd_rows"; data: { rows: PdRow[] } }
  | { type: "pd_status"; data: { samples: PdStatusSample[] } };

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

/** An open recording as a re-attached page picks it up (mirrors `devices.rs`). */
export interface RecordingSnapshot {
  rows: number;
  elapsed_ms: number;
}

/** The running session as a new page picks it up after a reload (mirrors `devices.rs`). */
export interface AttachedSession {
  device_id: string;
  model: string;
  firmware_version: string;
  serial: string | null;
  rate_sps: number | null;
  rate_locked: boolean;
  session_id: number;
  recording: RecordingSnapshot | null;
}

/** Asks the backend for the running session (after a reload); its later events go to
 * `onEvent`. Resolves to `null` when no session is running. */
export async function attach(
  onEvent: (event: BackendEvent) => void,
): Promise<AttachedSession | null> {
  if (mock) return mock.attach(onEvent);
  const channel = new Channel<BackendEvent>();
  channel.onmessage = onEvent;
  return invoke<AttachedSession | null>("attach", { onEvent: channel });
}

/** Ends the current session. */
export async function disconnect(): Promise<void> {
  if (mock) return mock.disconnect();
  await invoke("disconnect");
}

/** Stops waiting for an unplugged meter, after the user picked another device. */
export async function cancelWait(): Promise<void> {
  if (mock) return mock.cancelWait();
  await invoke("cancel_wait");
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

/** `t0Ms..=t1Ms` widened to the whole ms around it, which the backend takes: a view
 * narrower than a millisecond's precision (a deep zoom, AC-9CFMD3.4) widens rather than
 * failing to parse. */
function wholeMs(t0Ms: number, t1Ms: number): { t0Ms: number; t1Ms: number } {
  return { t0Ms: Math.max(0, Math.floor(t0Ms)), t1Ms: Math.max(0, Math.ceil(t1Ms)) };
}

/** Min / max buckets of the open session's history over `t0Ms..=t1Ms`, at most
 * `buckets` per series. */
export async function historyRange<const S extends readonly HistorySeries[]>(
  series: S,
  t0Ms: number,
  t1Ms: number,
  buckets: number,
): Promise<BucketsFor<S>> {
  if (mock) return mock.historyRange(series, t0Ms, t1Ms, buckets);
  return invoke<BucketsFor<S>>("history_range", { series, ...wholeMs(t0Ms, t1Ms), buckets });
}

/** The gaps between measuring runs of the open session's history that overlap
 * `t0Ms..=t1Ms`, oldest first. */
export async function historyGaps(t0Ms: number, t1Ms: number): Promise<HistoryGap[]> {
  if (mock) return mock.historyGaps(t0Ms, t1Ms);
  return invoke<HistoryGap[]>("history_gaps", wholeMs(t0Ms, t1Ms));
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

/** Starts the PD capture on the open session; rows and the capture state arrive on the
 * session channel (`pd_rows`, `pd_state`). */
export async function pdStart(): Promise<void> {
  if (mock) return mock.pdStart();
  await invoke("pd_start");
}

/** Stops the PD capture; the captured records stay. */
export async function pdStop(): Promise<void> {
  if (mock) return mock.pdStop();
  await invoke("pd_stop");
}

/** The raw bytes and field tree of captured PD record `index`. */
export async function pdDetails(index: number): Promise<PdDetails> {
  if (mock) return mock.pdDetails(index);
  return invoke<PdDetails>("pd_details", { index });
}

/** Drops every captured PD record; resolves to the index of the next record. Rows
 * numbered below it were sent before the clear and are stale. */
export async function pdClear(): Promise<number> {
  if (mock) return mock.pdClear();
  return invoke<number>("pd_clear");
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

/** Shows `path` selected in the system file manager (Finder, Explorer, …). */
export async function revealPath(path: string): Promise<void> {
  if (mock) return mock.revealPath(path);
  await revealItemInDir(path);
}
