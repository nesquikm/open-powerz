/** Shapes the Tauri backend sends to the UI (mirrors `src-tauri/src/{batch,worker,devices,ui_error}.rs`). */

import { Channel, invoke, isTauri } from "@tauri-apps/api/core";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { MockBackend } from "./mockBackend";
import { version as packageVersion } from "../../package.json";
import { ERROR_KINDS } from "./errorKinds";
import { DESKTOP_FEATURES, WEB_FEATURES, desktopOnlyError, type Features } from "./web/features";

/** The scripted demo backend, when the web build was made with `VITE_BACKEND=mock`. */
const mock: MockBackend | null = import.meta.env.VITE_BACKEND === "mock" ? new MockBackend() : null;

/** The browser backend over WebUSB / WebHID, when the build was made with
 * `VITE_BACKEND=web` (FR QFTCX0). A separate chunk, so the other builds carry none of
 * it, but preloaded at start-up so a click reaches the browser's picker synchronously. */
const web: Promise<WebBackendApi> | null =
  import.meta.env.VITE_BACKEND === "web"
    ? import("./web/webBackend").then((m) => m.createWebBackend())
    : null;

/** The web backend once it has loaded (the click path needs it without an await). */
let webReady: WebBackendApi | null = null;
web?.then(
  (b) => (webReady = b),
  () => {},
);

/** A command the web build leaves to the desktop app (AC-QFTCX0.6): rejects with a usage
 * error and sends nothing anywhere. */
function desktopOnly<T>(): Promise<T> {
  return Promise.reject(desktopOnlyError());
}

/** What this build offers: components hide or disable the controls it leaves out. */
export const features: Readonly<Features> = web ? WEB_FEATURES : DESKTOP_FEATURES;

/** Asks the browser for a meter to use (its device picker; FR QFTCX0). Resolves to the
 * granted meter's listing, or `null` when none was picked or the build has no picker. */
export function requestMeter(): Promise<DeviceListing | null> {
  // Loaded: straight to navigator.usb.requestDevice, still inside the user gesture.
  if (webReady) return webReady.requestMeter();
  if (web) return web.then((b) => b.requestMeter());
  return Promise.resolve(null);
}

/** Asks the browser for a meter over WebHID (AC-QFTCWZ.3), for a meter whose USB
 * interface another driver holds. Resolves `null` when none was picked or the build has
 * no browser backend. */
export function requestHidMeter(): Promise<DeviceListing | null> {
  // Loaded: straight to navigator.hid.requestDevice, still inside the user gesture.
  if (webReady) return webReady.requestHidMeter();
  if (web) return web.then((b) => b.requestHidMeter());
  return Promise.resolve(null);
}

// The mock build exposes itself so the UI suite can read what it recorded (reveal requests).
if (mock && typeof window !== "undefined") {
  (window as unknown as Record<string, unknown>).__OPEN_POWERZ_MOCK__ = mock;
}

/** Whether a backend is there to talk to: the Tauri app, or the mock build. */
export function backendAvailable(): boolean {
  return mock !== null || web !== null || isTauri();
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
  | "out_of_sync"
  | "flash_needs_consent"
  /** The user stopped the firmware update; not a failure (AC-AXHK2M.13). */
  | "cancelled"
  /** The meter has not saved its settings since the last change (FR QG6RNY). */
  | "settings_not_saved"
  | "internal";

/** A session a firmware update would release, as the prompt names it (FR AXHK2F); the
 * page sends back the ones the user agreed to release (mirrors `devices.rs`). */
export interface FlashHold {
  id: string;
  label: string;
  recording: boolean;
  /** Which recording, sent only while there is one: a consent covers that recording and
   * no later one on the same session (AC-AXHK2F.5). */
  recording_id?: number;
}

/** A plain-language error. */
export interface UiError {
  kind: ErrorKind;
  message: string;
  hint: string | null;
  /** The sessions a `flash_needs_consent` refusal names; sent only then. */
  sessions?: FlashHold[];
}

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
  if (isUiError(err)) {
    const { kind, message, hint, sessions } = err;
    return { kind, message, hint: hint ?? null, ...(sessions ? { sessions } : {}) };
  }
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
  /** What the meter runs, from its vendor interface's USB string (FR V9SK52). */
  mode: DeviceMode;
}

/** Application firmware, bootloader, or not known (mirrors `firmware::DeviceMode`). */
export type DeviceMode = "application" | "bootloader" | "unknown";

/** Firmware warning codes (mirrors `version::FirmwareWarning::code`). */
export type FirmwareWarningCode = "too_old" | "outdated";

export interface ConnectedInfo {
  model: string;
  firmware_version: string;
  /** Bootloader version, empty when the meter did not report one (FR V9SK52). */
  bootloader_version: string;
  mode: DeviceMode;
  /** Firmware warnings for this meter, none when it is up to date. */
  warnings: FirmwareWarningCode[];
  /** The HEAD check: false when current, null when HEAD was not read or not recognised (FR V9SK52). */
  update_required: boolean | null;
  /** Why parts of the firmware status could not be read, empty when all of it was. */
  firmware_problems: string[];
  serial: string;
  rate_sps: number | null;
  rate_locked: boolean;
  /** A WebHID session of the web build (AC-QFTCWZ.3): ADC snapshots only, no PD capture
   * or queue rates. Absent elsewhere. */
  basic_readings_only?: boolean | undefined;
  session_id: number;
  /** Oldest supported firmware, from the protocol library. */
  min_supported_firmware: string;
  /** Newest firmware known, from the protocol library. */
  known_latest_firmware: string;
}

export type Status =
  | ({ state: "connected" } & ConnectedInfo)
  | { state: "rate"; rate_sps: number; rate_locked: boolean }
  | { state: "disconnected"; error: UiError | null }
  /** The meter went away on its own; the app waits for this serial to come back. */
  | { state: "waiting"; serial: string };

export interface Gap {
  kind: "stream" | "outage" | "pause";
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

/** Why a recording ended (mirrors `EndReason`). */
export type RecordingEnd = "stopped" | "disconnected" | "low_current" | "row_limit";

/** What a finished recording wrote. */
export interface RecordingSummary {
  rows: number;
  gaps: Gap[];
  /** The recording ended because the meter went away. */
  disconnected: boolean;
  ended: RecordingEnd;
}

/** Recording state changes (mirrors `src-tauri/src/recording.rs`). */
export type RecordingEvent =
  | { state: "started" }
  | { state: "progress"; rows: number; elapsed_ms: number; paused: boolean }
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
  /** The folded oldest part of the history the export left out (FR AXHK2W,
   * AC-AXHK2W.14); absent or `null` when nothing was. */
  folded?: ExportFolded | null;
}

/** The oldest part of a session an export left out because the history folded it. */
export interface ExportFolded {
  /** Folded samples left out. */
  samples: number;
  /** Session time they span, ms. */
  ms: number;
  /** Wall-clock time of the file's first row, ms. */
  start_unix_ms: number;
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

/** Why the app stopped a PD capture the user did not stop (FR JQ971R). */
export type PdStopReason = "trigger";

/** Every event the backend sends over the session channel. */
export type BackendEvent =
  | { type: "batch"; data: Batch }
  | { type: "gap"; data: Gap }
  | { type: "status"; data: Status }
  | { type: "error"; data: UiError }
  | { type: "recording"; data: RecordingEvent }
  | { type: "pd_state"; data: { capturing: boolean; reason?: PdStopReason } }
  | { type: "pd_rows"; data: { rows: PdRow[] } }
  | {
      type: "pd_status";
      /** `dropped`: status blocks the backend dropped past its limit since the last
       * clear (FR AXHK2W, AC .2). */
      data: { samples: PdStatusSample[]; dropped?: number };
    };

/** Lists the meters and demo devices available right now. */
export function listDevices(): Promise<DeviceListing[]> {
  if (web) return web.then((b) => b.listDevices());
  if (mock) return mock.listDevices();
  return invoke<DeviceListing[]>("list_devices");
}

/** Subscribes to hotplug: `onChange` receives the full device list on every change.
 * Resolves to an unsubscribe function; the backend's watcher is replaced by the next
 * subscription and stops by itself once the window is gone. */
export async function watchDevices(
  onChange: (devices: DeviceListing[]) => void,
): Promise<() => void> {
  if (web) return web.then((b) => b.watchDevices(onChange));
  if (mock) return mock.watchDevices(onChange);
  let deliver = onChange;
  const channel = new Channel<DeviceListing[]>();
  channel.onmessage = (devices) => deliver(devices);
  await invoke("watch_devices", { onChange: channel });
  return () => {
    deliver = () => {};
  };
}

/** Where the live measurements come from (mirrors `worker::MeasurementSource`, FR
 * 1HHT02): the ADC queue over the vendor bulk pipe, or the ADCNEW stream the meter
 * pushes on its CDC serial port (no sign-in, no temperature). */
export type MeasurementSource = "queue" | "cdc";

/** The sources the connect controls offer, the default first. */
export const MEASUREMENT_SOURCES: readonly { value: MeasurementSource; label: string }[] = [
  { value: "queue", label: "USB queue" },
  { value: "cdc", label: "Serial stream" },
];

/** Opens a session on `deviceId` streaming from `source` (the queue by default);
 * every session event goes to `onEvent`. */
export async function connect(
  deviceId: string,
  rateSps: number,
  onEvent: (event: BackendEvent) => void,
  source: MeasurementSource = "queue",
): Promise<void> {
  if (web) return web.then((b) => b.connect(deviceId, rateSps, onEvent, source));
  if (mock) return mock.connect(deviceId, rateSps, onEvent, source);
  const channel = new Channel<BackendEvent>();
  channel.onmessage = onEvent;
  // The queue is the backend's default, so it is left out of the arguments.
  const picked = source === "queue" ? {} : { source };
  await invoke("connect", { deviceId, rateSps, ...picked, onEvent: channel });
}

/** An open recording as a re-attached page picks it up (mirrors `devices.rs`). */
export interface RecordingSnapshot {
  rows: number;
  elapsed_ms: number;
  /** Its file. */
  path?: string | null | undefined;
  /** A pause holds its rows back. */
  paused?: boolean | undefined;
}

/** The running session as a new page picks it up after a reload (mirrors `devices.rs`). */
export interface AttachedSession {
  device_id: string;
  model: string;
  firmware_version: string;
  serial: string | null;
  rate_sps: number | null;
  rate_locked: boolean;
  /** A WebHID session of the web build (AC-QFTCWZ.3). */
  basic_readings_only?: boolean | undefined;
  session_id: number;
  recording: RecordingSnapshot | null;
  /** The PD capture runs (AC-AXHK2K.7). */
  pd_capturing?: boolean | undefined;
  /** Status blocks the PD capture dropped past its limit since the last clear
   * (FR AXHK2W, AC .3). */
  pd_status_dropped?: number | undefined;
}

/** Asks the backend for the running session (after a reload); its later events go to
 * `onEvent`. Resolves to `null` when no session is running. Without `deviceId` it is the
 * current meter's session; with one, that meter's (FR 1HHT03). */
export async function attach(
  onEvent: (event: BackendEvent) => void,
  deviceId?: string,
): Promise<AttachedSession | null> {
  if (web) return web.then((b) => b.attach(onEvent, deviceId));
  if (mock) return mock.attach(onEvent, deviceId);
  const channel = new Channel<BackendEvent>();
  channel.onmessage = onEvent;
  const named = deviceId === undefined ? {} : { deviceId };
  return invoke<AttachedSession | null>("attach", { ...named, onEvent: channel });
}

/** The device ids of the meters whose session runs, as their attach snapshots name them,
 * in the order they were connected (FR 1HHT03): what a reloaded page picks up with
 * {@link attach}. */
export async function connectedMeters(): Promise<string[]> {
  if (web) return web.then((b) => b.connectedMeters());
  if (mock) return mock.connectedIds();
  return invoke<string[]>("running_meters");
}

/** Ends the current session. */
export async function disconnect(): Promise<void> {
  if (web) return web.then((b) => b.disconnect());
  if (mock) return mock.disconnect();
  await invoke("disconnect");
}

/** Stops waiting for an unplugged meter, after the user picked another device. */
export async function cancelWait(): Promise<void> {
  if (web) return web.then((b) => b.cancelWait());
  if (mock) return mock.cancelWait();
  await invoke("cancel_wait");
}

/** Changes the queue rate of the current session. */
export async function setRate(rateSps: number): Promise<void> {
  if (web) return web.then((b) => b.setRate(rateSps));
  if (mock) return mock.setRate(rateSps);
  await invoke("set_rate", { rateSps });
}

/** What a recording starts with (FR AXHK2R); the backend checks them too. */
export interface RecordingOptions {
  folder?: string | null | undefined;
  below_a?: number | null | undefined;
  for_ms?: number | null | undefined;
  max_rows?: number | null | undefined;
}

/** Starts recording at once to a default file named after `suggestedName`. Resolves to
 * its path; {@link saveRecordingAs} moves the finished file. */
export async function startRecording(
  suggestedName: string,
  options?: RecordingOptions,
): Promise<string | null> {
  if (web) return web.then((b) => b.startRecording(suggestedName, options));
  if (mock) return mock.startRecording(suggestedName, options);
  return invoke<string | null>("start_recording", { suggestedName, options });
}

/** Asks which folder recordings go to; `null` when the dialog was cancelled. */
export async function chooseRecordingFolder(): Promise<string | null> {
  if (web) return desktopOnly();
  if (mock) return mock.chooseRecordingFolder();
  return invoke<string | null>("choose_recording_folder");
}

/** Stops the recording; resolves to its row count and gaps. */
export async function stopRecording(): Promise<RecordingSummary> {
  if (web) return web.then((b) => b.stopRecording());
  if (mock) return mock.stopRecording();
  return invoke<RecordingSummary>("stop_recording");
}

/** Holds the recording's rows back while the stream goes on. */
export async function pauseRecording(): Promise<void> {
  if (web) return web.then((b) => b.pauseRecording());
  if (mock) return mock.pauseRecording();
  await invoke("recording_pause");
}

/** Writes the recording's rows again, in the same file. */
export async function resumeRecording(): Promise<void> {
  if (web) return web.then((b) => b.resumeRecording());
  if (mock) return mock.resumeRecording();
  await invoke("recording_resume");
}

/** Opens the save dialog for the finished recording at `path` and moves it to the chosen
 * file. Resolves to the new path, or `null` when the dialog was cancelled. */
export async function saveRecordingAs(path: string): Promise<string | null> {
  if (web) return web.then((b) => b.saveRecordingAs(path));
  if (mock) return mock.saveRecordingAs(path);
  return invoke<string | null>("save_recording_as", { path });
}

/** Opens the save dialog proposing `suggestedName` and exports the session so far,
 * reporting progress to `onProgress`. Resolves to `null` when the dialog was cancelled. */
export async function exportSession(
  suggestedName: string,
  onProgress: (progress: ExportProgress) => void,
): Promise<ExportResult | null> {
  if (web) return web.then((b) => b.exportSession(suggestedName, onProgress));
  if (mock) return mock.exportSession(suggestedName, onProgress);
  const channel = new Channel<ExportProgress>();
  channel.onmessage = onProgress;
  return invoke<ExportResult | null>("export_session", { suggestedName, onProgress: channel });
}

/** Opens the save dialog proposing `suggestedName` and exports the session so far to a
 * vendor-layout `.db` file (FR AXHK2S). Resolves like {@link exportSession}. */
export async function exportSessionDb(
  suggestedName: string,
  onProgress: (progress: ExportProgress) => void,
): Promise<ExportResult | null> {
  if (web) return web.then((b) => b.exportSessionDb(suggestedName, onProgress));
  if (mock) return mock.exportSessionDb(suggestedName, onProgress);
  const channel = new Channel<ExportProgress>();
  channel.onmessage = onProgress;
  return invoke<ExportResult | null>("export_session_db", { suggestedName, onProgress: channel });
}

/** Statistics of the open session since its start or the last reset. */
export async function stats(): Promise<StatsSnapshot> {
  if (web) return web.then((b) => b.stats());
  if (mock) return mock.stats();
  return invoke<StatsSnapshot>("stats");
}

/** `t0Ms..=t1Ms` widened to the whole ms around it, which the backend takes: a view
 * narrower than a millisecond's precision (a deep zoom, AC-9CFMD3.4) widens rather than
 * failing to parse. */
function wholeMs(t0Ms: number, t1Ms: number): { t0Ms: number; t1Ms: number } {
  return { t0Ms: Math.max(0, Math.floor(t0Ms)), t1Ms: Math.max(0, Math.ceil(t1Ms)) };
}

/** What the view shows of an opened recording file (mirrors `src-tauri/src/recording_file.rs`). */
export interface FileSummary {
  /** The file's name, without its folder. */
  name: string;
  rows: number;
  /** Damaged rows skipped. */
  skipped: number;
  /** Readable rows past the row limit, or past the time a record holds. */
  left_out: number;
  stats: StatsSnapshot;
}

/** How far an open has got, in a unit of the reader's own. */
export interface OpenProgress {
  done: number;
  /** 0 when the end is not known. */
  total: number;
}

/** Opens the open dialog and reads the chosen CSV or vendor `.db` recording (FR AXHK2S),
 * off the UI thread; needs no meter and sends nothing to one. Resolves to its summary, or
 * `null` when the dialog was dismissed or Cancel ended the open. */
export async function openRecordingFile(
  onProgress: (progress: OpenProgress) => void,
): Promise<FileSummary | null> {
  if (web) return web.then((b) => b.openRecordingFile(onProgress));
  if (mock) return mock.openRecordingFile(onProgress);
  const channel = new Channel<OpenProgress>();
  channel.onmessage = onProgress;
  return invoke<FileSummary | null>("recording_file_open", { onProgress: channel });
}

/** Ends the open under way: it resolves to `null`. */
export async function cancelRecordingFile(): Promise<void> {
  if (web) return web.then((b) => b.cancelRecordingFile());
  if (mock) return mock.cancelRecordingFile();
  await invoke("recording_file_cancel");
}

/** Closes the open recording file. */
export async function closeRecordingFile(): Promise<void> {
  if (web) return web.then((b) => b.closeRecordingFile());
  if (mock) return mock.closeRecordingFile();
  await invoke("recording_file_close");
}

/** Min / max buckets of the open recording file over `t0Ms..=t1Ms`, at most `buckets` per
 * series. */
export async function recordingFileRange<const S extends readonly HistorySeries[]>(
  series: S,
  t0Ms: number,
  t1Ms: number,
  buckets: number,
): Promise<BucketsFor<S>> {
  if (web) return web.then((b) => b.recordingFileRange(series, t0Ms, t1Ms, buckets));
  if (mock) return mock.recordingFileRange(series, t0Ms, t1Ms, buckets);
  return invoke<BucketsFor<S>>("recording_file_range", {
    series,
    ...wholeMs(t0Ms, t1Ms),
    buckets,
  });
}

/** Min / max buckets of the open session's history over `t0Ms..=t1Ms`, at most
 * `buckets` per series. */
export async function historyRange<const S extends readonly HistorySeries[]>(
  series: S,
  t0Ms: number,
  t1Ms: number,
  buckets: number,
): Promise<BucketsFor<S>> {
  if (web) return web.then((b) => b.historyRange(series, t0Ms, t1Ms, buckets));
  if (mock) return mock.historyRange(series, t0Ms, t1Ms, buckets);
  return invoke<BucketsFor<S>>("history_range", { series, ...wholeMs(t0Ms, t1Ms), buckets });
}

/** The gaps between measuring runs of the open session's history that overlap
 * `t0Ms..=t1Ms`, oldest first. */
export async function historyGaps(t0Ms: number, t1Ms: number): Promise<HistoryGap[]> {
  if (web) return web.then((b) => b.historyGaps(t0Ms, t1Ms));
  if (mock) return mock.historyGaps(t0Ms, t1Ms);
  return invoke<HistoryGap[]>("history_gaps", wholeMs(t0Ms, t1Ms));
}

/** Where the open session's history keeps full resolution from, ms; the data before it is
 * folded into coarser blocks. `null` while nothing is folded (AC-AXHK2W.11). */
export async function historyFoldBoundary(): Promise<number | null> {
  if (web) return web.then((b) => b.historyFoldBoundary());
  if (mock) return mock.historyFoldBoundary();
  return invoke<number | null>("history_fold_boundary");
}

/** What the ruler shows between its cursors, from the raw samples with
 * `t0 <= t <= t1` (FR AXHK2T). */
export interface RangeStats {
  samples: number;
  charge_ah: number;
  energy_wh: number;
  vbus_avg_v: number;
  ibus_avg_a: number;
  power_avg_w: number;
}

/** The ruler's stats over the open session's history between `t0Ms` and `t1Ms`; `null`
 * when no sample is in range (AC-AXHK2T.2). */
export async function rangeStats(t0Ms: number, t1Ms: number): Promise<RangeStats | null> {
  if (web) return web.then((b) => b.rangeStats(t0Ms, t1Ms));
  if (mock) return mock.rangeStats(t0Ms, t1Ms);
  return invoke<RangeStats | null>("range_stats", wholeMs(t0Ms, t1Ms));
}

/** The ruler's stats over the open recording file between `t0Ms` and `t1Ms`. */
export async function recordingFileRangeStats(
  t0Ms: number,
  t1Ms: number,
): Promise<RangeStats | null> {
  if (web) return web.then((b) => b.recordingFileRangeStats(t0Ms, t1Ms));
  if (mock) return mock.recordingFileRangeStats(t0Ms, t1Ms);
  return invoke<RangeStats | null>("recording_file_range_stats", wholeMs(t0Ms, t1Ms));
}

/** What segment marks follow (FR AXHK2T). */
export type RunMarkKind = "charge" | "energy";

/** What E.D. is a share of: the whole session's energy (summed over its runs, never
 * across a run break) or the current run's (AC-AXHK2T.5). */
export type EdScope = "session" | "run";

/** What `run_progress` is asked for (mirrors `RunProgressQuery` in
 * `src-tauri/src/history.rs`). */
export interface RunProgressQuery {
  /** The running total the marks follow. */
  kind: RunMarkKind;
  /** Mark every `step_pct` percent; 0% and 100% are never marked, and a step outside
   * `0 < step < 100` marks nothing. */
  step_pct: number;
  /** Evenly spaced E.D. points to sample, about the plot width (at most 8192); 0 for none. */
  ed_points: number;
  /** The E.D. window, ms, clamped to the scope's samples (either order); all of them when
   * left out. */
  ed_t0_ms?: number | null;
  ed_t1_ms?: number | null;
  /** What E.D. is a share of; `"session"` when left out. Marks always follow the run. */
  ed_scope?: EdScope;
}

/** One segment mark: where the run's running total first reached `percent` of its final
 * value, ms (between records, so not whole). */
export interface RunMark {
  t_ms: number;
  percent: number;
}

/** Segment marks of the current run (since the last run break) and E.D. of the session
 * (or the run, per `ed_scope`), integrated over the raw samples (AC-AXHK2T.4, .5). */
export interface RunProgress {
  /** The first sample's time of the session (or file), ms. */
  session_t0_ms: number;
  run_t0_ms: number;
  run_t1_ms: number;
  samples: number;
  charge_ah: number;
  energy_wh: number;
  /** The followed total is above zero; when not (idle, discharging, one sample) `marks`
   * is empty and the UI can say why. */
  marks_total_positive: boolean;
  marks: RunMark[];
  /** The E.D. total: the session's (or the run's) energy, Wh. */
  ed_total_wh: number;
  /** `ed_total_wh` is above zero; when not, `ed_t_ms` and `ed_pct` are empty. */
  energy_total_positive: boolean;
  ed_t_ms: number[];
  /** Running energy at each `ed_t_ms` as a percentage of `ed_total_wh`. */
  ed_pct: number[];
}

/** `q` as the backend takes it: whole, non-negative counts and milliseconds. */
function wireQuery(q: RunProgressQuery): Required<RunProgressQuery> {
  const ms = (t: number | null | undefined, round: (x: number) => number): number | null =>
    t === null || t === undefined || !Number.isFinite(t) ? null : Math.max(0, round(t));
  const lo = ms(q.ed_t0_ms, Math.floor);
  const hi = ms(q.ed_t1_ms, Math.ceil);
  return {
    kind: q.kind,
    step_pct: q.step_pct,
    ed_points: Number.isFinite(q.ed_points) ? Math.max(0, Math.floor(q.ed_points)) : 0,
    ed_t0_ms: lo,
    ed_t1_ms: hi,
    ed_scope: q.ed_scope === "run" ? "run" : "session",
  };
}

/** Segment marks and E.D. of the open session's current run; `null` with an empty
 * history (AC-AXHK2T.4, .5). */
export async function runProgress(query: RunProgressQuery): Promise<RunProgress | null> {
  if (web) return web.then((b) => b.runProgress(query));
  if (mock) return mock.runProgress(query);
  return invoke<RunProgress | null>("run_progress", { query: wireQuery(query) });
}

/** Segment marks and E.D. of the open recording file's rows. */
export async function recordingFileRunProgress(
  query: RunProgressQuery,
): Promise<RunProgress | null> {
  if (web) return web.then((b) => b.recordingFileRunProgress(query));
  if (mock) return mock.recordingFileRunProgress(query);
  return invoke<RunProgress | null>("recording_file_run_progress", { query: wireQuery(query) });
}

/** Zeroes the session statistics; they count again from the next sample. */
export async function resetStats(): Promise<void> {
  if (web) return web.then((b) => b.resetStats());
  if (mock) return mock.resetStats();
  await invoke("reset_stats");
}

/** Discards the session history; the stream (and any recording) goes on. */
export async function clearHistory(): Promise<void> {
  if (web) return web.then((b) => b.clearHistory());
  if (mock) return mock.clearHistory();
  await invoke("clear_history");
}

/** Starts the PD capture on the open session; rows and the capture state arrive on the
 * session channel (`pd_rows`, `pd_state`). */
export async function pdStart(): Promise<void> {
  if (web) return web.then((b) => b.pdStart());
  if (mock) return mock.pdStart();
  await invoke("pd_start");
}

/** Stops the PD capture; the captured records stay. */
export async function pdStop(): Promise<void> {
  if (web) return web.then((b) => b.pdStop());
  if (mock) return mock.pdStop();
  await invoke("pd_stop");
}

/** The raw bytes and field tree of PD record `index`: of the capture opened from a file
 * when `opened`, of the live capture otherwise. */
export async function pdDetails(index: number, opened = false): Promise<PdDetails> {
  if (web) return web.then((b) => b.pdDetails(index, opened));
  if (mock) return mock.pdDetails(index, opened);
  return invoke<PdDetails>("pd_details", opened ? { index, opened } : { index });
}

/** Confidence of a meter setting: checked on the meter's screen, or from static
 * analysis only (FR 1HHT01). */
export type SettingTag = "confirmed" | "inferred";

/** One read-only row of the Meter settings panel. */
export interface MeterSettingField {
  key: string;
  label: string;
  value: string;
  tag: SettingTag;
}

/** The connected meter's settings, in display order (FR 1HHT01). */
export interface MeterSettings {
  fields: MeterSettingField[];
  /** What a write changed beyond the fields, in plain words; a warning, not a failure. */
  warnings?: string[];
}

/** Reads the connected meter's settings (GET_DATA 0x008 only); rejects with the
 * backend's error, e.g. without a meter or on a CRC mismatch. */
export async function meterSettings(): Promise<MeterSettings> {
  if (web) return web.then((b) => b.meterSettings());
  if (mock) return mock.meterSettings();
  return invoke<MeterSettings>("meter_settings");
}

/** A settings field the panel may change (FR JKWT3W); calibration, auto-rotate and
 * the startup logo never are. */
export type EditableSettingKey =
  "language" | "brightness" | "dim_timeout" | "theme" | "rotation" | "sample_rate";

/** A settings change: the new wire code of each changed field (FR JKWT3W); the Startup
 * logo view also sets `startup_duration` (1..3 for 1, 3 or 5 s, FR MEHQC0, AC .5). */
export type SettingChanges = Partial<Record<EditableSettingKey | "startup_duration", number>>;

/** Writes `changes` to the connected meter (one settings write between two reads) and
 * resolves to the settings as read back; rejects with the backend's error, e.g. when
 * the re-read does not match. Nothing is retried. */
export async function meterSettingsWrite(changes: SettingChanges): Promise<MeterSettings> {
  if (web) return web.then((b) => b.meterSettingsWrite(changes));
  if (mock) return mock.meterSettingsWrite(changes);
  return invoke<MeterSettings>("meter_settings_write", { changes });
}

/** One offline record on the meter, with its decoded list columns (FR 1HHT00). */
export interface OfflineRecordEntry {
  /** 1-based position in the meter's list. */
  index: number;
  name: string;
  /** Seconds between samples. */
  interval_s: number;
  points: number;
  duration_s: number;
  /** Charge in Ah as stored: signed, negative for one current direction. */
  charge_ah: number;
  /** Energy in Wh as stored: signed like the charge. */
  energy_wh: number;
  /** Offset of the samples in the record area. */
  offset: number;
}

/** The meter's offline-record list (FR 1HHT00). */
export interface RecordList {
  records: OfflineRecordEntry[];
}

/** One sample of a downloaded offline record. */
export interface OfflineRecordSample {
  /** Seconds from the record start. */
  t_s: number;
  vbus_v: number;
  ibus_a: number;
  charge_ah: number;
  energy_wh: number;
}

/** A downloaded offline record (FR 1HHT00). */
export interface OfflineRecord {
  index: number;
  name: string;
  interval_s: number;
  samples: OfflineRecordSample[];
}

/** Lists the connected meter's offline records (GET_DATA 0x200 only); rejects with
 * the backend's error, e.g. without a meter. */
export async function recordsList(): Promise<RecordList> {
  if (web) return web.then((b) => b.recordsList());
  if (mock) return mock.recordsList();
  return invoke<RecordList>("records_list");
}

/** Downloads offline record `index` (1-based): its own window, read in chunks of at most
 * 256 bytes. */
export async function recordDownload(index: number): Promise<OfflineRecord> {
  if (web) return web.then((b) => b.recordDownload(index));
  if (mock) return mock.recordDownload(index);
  return invoke<OfflineRecord>("record_download", { index });
}

/** The result of an offline-record delete (FR JKWT3V): the list as read again and
 * where the backup went. */
export interface RecordDeleteResult {
  /** The meter's records after the delete. */
  records: OfflineRecordEntry[];
  /** Folder holding the CSV backup. */
  backup_dir: string;
  /** The backup CSV files, one per deleted record. */
  backups: string[];
}

/** Deletes offline record `target` (1-based), or every record with `"all"`, after a
 * backup; `confirm` is the typed record name (or `all`), which the backend checks too. */
export async function recordsDelete(
  target: number | "all",
  confirm: string,
): Promise<RecordDeleteResult> {
  if (web) return web.then((b) => b.recordsDelete(target, confirm));
  if (mock) return mock.recordsDelete(target, confirm);
  return invoke<RecordDeleteResult>("records_delete", {
    index: target === "all" ? null : target,
    confirm,
  });
}

/** A PD capture opened from a pdcap file (FR TGE83E): its rows and status samples,
 * numbered from 0. */
export interface PdOpened {
  path: string;
  /** The rows the capture keeps (at most its record limit). */
  rows: PdRow[];
  status: PdStatusSample[];
  /** Records the capture dropped past its limit (the oldest ones): not in `rows`. */
  dropped: number;
  /** The 1-based line where reading stopped when the file is truncated or corrupt
   * (AC-TGE83E.3); every valid line before it was loaded. `null` for a whole file. */
  damaged_line: number | null;
  /** Vendor log rows skipped because their record bytes are oversized (0 for a pdcap
   * file). */
  oversized: number;
  /** Vendor log rows skipped because they hold no whole record (0 for a pdcap file). */
  skipped: number;
  /** The vendor log has more rows than the import takes: only the first ones were
   * imported (false for a pdcap file). */
  truncated: boolean;
}

/** Opens the open dialog and loads the chosen pdcap file (FR TGE83E); needs no meter.
 * Resolves to the capture, or `null` when the dialog was cancelled. */
export async function pdOpen(): Promise<PdOpened | null> {
  if (web) return desktopOnly();
  if (mock) return mock.pdOpen();
  return invoke<PdOpened | null>("pd_open");
}

/** Releases the capture opened from a file, which the page no longer shows
 * (AC-AXHK2K.5). The slot is the app's, not a meter's: it needs no meter. */
export async function pdReleaseOpened(): Promise<void> {
  if (web) return;
  if (mock) return mock.pdReleaseOpened();
  await invoke("pd_release_opened");
}

/** Opens the open dialog and imports the chosen vendor PD log database (`.sqlite`)
 * read-only (FR TGE83E, AC .6); needs no meter. Resolves like {@link pdOpen}: the
 * imported capture, or `null` when the dialog was cancelled. */
export async function pdImport(): Promise<PdOpened | null> {
  if (web) return desktopOnly();
  if (mock) return mock.pdImport();
  return invoke<PdOpened | null>("pd_import");
}

/** A local firmware container's metadata (FR V9SK53, AC .3). The image itself is never
 * decrypted; `crc` is 8 uppercase hex digits and `size` the declared image length in bytes. */
export interface FirmwareFileInfo {
  vendor: string;
  container_version: string;
  model: string;
  version: string;
  date: string;
  crc: string;
  size: number;
  /** Payload length the file stores, bytes; may exceed `size` (display only). */
  stored_size: number;
  release_notes_en: string;
}

/** Reads the firmware container at `path` and resolves to its metadata; needs no meter.
 * A corrupt file or one that is not a firmware container rejects with a usage error. */
export async function firmwareInspect(path: string): Promise<FirmwareFileInfo> {
  if (web) return desktopOnly();
  if (mock) return mock.firmwareInspect(path);
  return invoke<FirmwareFileInfo>("firmware_inspect", { path });
}

/** Opens the open dialog filtered to firmware containers (`.mencrypt`) (FR V9SK55,
 * AC .2). Resolves to the chosen path, or `null` when the dialog was cancelled. */
export async function firmwareChoose(): Promise<string | null> {
  if (web) return desktopOnly();
  if (mock) return mock.firmwareChoose();
  return invoke<string | null>("firmware_choose");
}

/** The startup logo a picture encodes to (FR MEHQC0, AC .1): the exact pixels and
 * numbers that would be written. */
export interface LogoPreview {
  width: number;
  height: number;
  mode: "opaque" | "alpha";
  /** Unpadded blob size, bytes. */
  blob_size: number;
  /** Padded (written) size, bytes. */
  padded_size: number;
  /** 256-byte chunks written. */
  chunks: number;
  /** Either side is below 48. */
  small_side_warning: boolean;
  /** The blob decoded back to RGBA8888 (`width * height * 4` bytes), base64. */
  rgba: string;
}

/** One batched progress message of a logo write (FR MEHQC0, AC .3). */
export interface LogoProgress {
  step: "chunks" | "read_back";
  done: number;
  total: number;
}

/** The meter's settings after a logo write: saved, waiting for a menu save on the meter,
 * or with other calibration bytes (FR QG6RNY). */
export type LogoSettingsState = "saved" | "needs_menu_save" | "calibration_changed";

/** What a logo upload or Remove left on the meter (FR MEHQC0, FR QG6RNY): the startup
 * durations before and set, the fresh settings read and, when saved, its duration. */
export interface LogoWritten {
  duration_before: string;
  duration_set: string;
  settings: LogoSettingsState;
  startup_duration: string | null;
}

/** One check of the settings after a logo write (FR QG6RNY). */
export interface LogoSettingsCheck {
  settings: LogoSettingsState;
  startup_duration: string | null;
}

/** What the meter's logo area holds (FR MEHQC0, AC .7). */
export interface LogoRead {
  status: "empty" | "logo" | "unrecognised";
  width: number | null;
  height: number | null;
  mode: "opaque" | "alpha" | null;
  /** The stored logo as RGBA8888, base64, when a full read was asked for. */
  image: string | null;
}

/** Opens the open dialog filtered to logo pictures (PNG, JPEG, BMP, raw `.bin`) (FR
 * MEHQC0, AC .1). Resolves to the chosen path, or `null` when the dialog was cancelled. */
export async function logoChoose(): Promise<string | null> {
  if (web) return web.then((b) => b.logoChoose());
  if (mock) return mock.logoChoose();
  return invoke<string | null>("logo_choose");
}

/** The startup logo the picture at `path` encodes to, opaque or (`alpha`) transparent
 * (FR MEHQC0, AC .1); needs no meter. A file that is no picture rejects with a usage
 * error. */
export async function logoPreview(path: string, alpha: boolean): Promise<LogoPreview> {
  if (web) return web.then((b) => b.logoPreview(path, alpha));
  if (mock) return mock.logoPreview(path, alpha);
  return invoke<LogoPreview>("logo_preview", { path, alpha });
}

/** A built-in open-powerz startup logo (FR 70X5YX): the mascot, or the mascot with the
 * lettering. */
export type BuiltinLogoName = "surf" | "surf-wordmark";

/** The startup logo built-in logo `name` encodes to, always transparent (FR 70X5YX);
 * needs no meter. */
export async function logoPreviewBuiltin(name: BuiltinLogoName): Promise<LogoPreview> {
  if (web) return web.then((b) => b.logoPreviewBuiltin(name));
  if (mock) return mock.logoPreviewBuiltin(name);
  return invoke<LogoPreview>("logo_preview", { builtin: name, alpha: true });
}

/** What meter `deviceId`'s logo area holds (FR MEHQC0, AC .7); with `full`, the stored
 * image too. Only memory reads go out. In the browser, `onProgress` hears the read's
 * memory reads done of the total (FR CJ3QCD). */
export async function logoRead(
  deviceId: string,
  full: boolean,
  onProgress?: (progress: LogoProgress) => void,
): Promise<LogoRead> {
  if (web) return web.then((b) => b.logoRead(deviceId, full, onProgress));
  if (mock) return mock.logoRead(deviceId, full);
  return invoke<LogoRead>("logo_read", { deviceId, full });
}

/** Uploads the picture at `path` (opaque or `alpha`) to meter `deviceId` as its startup
 * logo (FR MEHQC0, AC .3): chunks, the startup-duration write, then a full read-back.
 * Batched progress (at most 20 a second) goes to `onProgress`; rejects with the
 * backend's error, naming the chunk a failed write stopped at. Nothing is retried. */
export async function logoUpload(
  deviceId: string,
  path: string,
  alpha: boolean,
  onProgress: (progress: LogoProgress) => void,
): Promise<LogoWritten> {
  if (web) return web.then((b) => b.logoUpload(deviceId, path, alpha, onProgress));
  if (mock) return mock.logoUpload(deviceId, path, alpha, onProgress);
  const channel = new Channel<LogoProgress>();
  channel.onmessage = onProgress;
  return invoke<LogoWritten>("logo_upload", { deviceId, path, alpha, onProgress: channel });
}

/** Uploads built-in logo `name` (always transparent) as meter `deviceId`'s startup logo
 * (FR 70X5YX), like {@link logoUpload} does a picture. */
export async function logoUploadBuiltin(
  deviceId: string,
  name: BuiltinLogoName,
  onProgress: (progress: LogoProgress) => void,
): Promise<LogoWritten> {
  if (web) return web.then((b) => b.logoUploadBuiltin(deviceId, name, onProgress));
  if (mock) return mock.logoUploadBuiltin(deviceId, name, onProgress);
  const channel = new Channel<LogoProgress>();
  channel.onmessage = onProgress;
  return invoke<LogoWritten>("logo_upload", {
    deviceId,
    builtin: name,
    alpha: true,
    onProgress: channel,
  });
}

/** Removes meter `deviceId`'s startup logo, so the built-in one shows again (FR MEHQC0,
 * AC .4); progress goes to `onProgress` like an upload's. */
export async function logoRemove(
  deviceId: string,
  onProgress: (progress: LogoProgress) => void,
): Promise<LogoWritten> {
  if (web) return web.then((b) => b.logoRemove(deviceId, onProgress));
  if (mock) return mock.logoRemove(deviceId, onProgress);
  const channel = new Channel<LogoProgress>();
  channel.onmessage = onProgress;
  return invoke<LogoWritten>("logo_remove", { deviceId, onProgress: channel });
}

/** One read-only check of meter `deviceId`'s settings after a logo write whose settings
 * wait for a menu save on the meter (FR QG6RNY); rejects with a usage error when no
 * logo write is waiting. */
export async function logoSettingsCheck(deviceId: string): Promise<LogoSettingsCheck> {
  if (web) return web.then((b) => b.logoSettingsCheck(deviceId));
  if (mock) return mock.logoSettingsCheck(deviceId);
  return invoke<LogoSettingsCheck>("logo_settings_check", { deviceId });
}

/** One stage of a firmware update as the backend reports it (FR V9SK55, AC .4); the
 * write stage carries the bytes written so far and the image length. */
export type FlashStage =
  | { stage: "switching" }
  | { stage: "erasing" }
  | { stage: "writing"; done: number; total: number }
  | { stage: "verifying" }
  | { stage: "finished" }
  | { stage: "returning" };

/** A jump was sent and the meter needs a replug if it does not restart by itself;
 * `reply` is its answer to the jump. */
export type FlashReplug = { stage: "replug"; to: "bootloader" | "application"; reply: string };

/** What the backend reports during an update: a stage, or a replug prompt. */
export type FlashEvent = FlashStage | FlashReplug;

/** How a firmware update ended (FR V9SK55, AC .5): the version read back from the meter,
 * or an image accepted while the meter stays in its bootloader until it is replugged. */
export type FlashDone =
  { outcome: "verified"; version: string } | { outcome: "power_cycle_needed" };

/** Updates the meter `deviceId` with the firmware file at `path` (FR V9SK55, AC .4): the
 * backend holds the meter for the update, re-checks that `confirmation` is the file's
 * version before anything is sent, and reports each stage (and each replug prompt) to
 * `onStage`. Resolves to the
 * outcome, or rejects with the error and its recovery steps. `agreed` names the sessions
 * the user agreed to release; a session it does not cover gets a `flash_needs_consent`
 * refusal, with nothing released or sent (FR AXHK2F). */
export async function firmwareFlash(
  deviceId: string,
  path: string,
  confirmation: string,
  onStage: (stage: FlashEvent) => void,
  agreed: FlashHold[] = [],
): Promise<FlashDone> {
  if (web) return web.then((b) => b.firmwareFlash(deviceId, path, confirmation, onStage, agreed));
  if (mock) return mock.firmwareFlash(deviceId, path, confirmation, onStage, agreed);
  const channel = new Channel<FlashEvent>();
  channel.onmessage = onStage;
  return invoke<FlashDone>("firmware_flash", {
    deviceId,
    path,
    confirmation,
    agreed,
    onProgress: channel,
  });
}

/** Asks the running update to stop (AC-4PJRVJ.1); rejects once its erase has begun. */
export async function firmwareCancel(): Promise<void> {
  if (web) return;
  if (mock) return mock.firmwareCancel();
  return invoke<void>("firmware_cancel");
}

/** How an update ended, as the backend keeps it for a reloaded page (AC-AXHK2M.2). */
export type FlashEnd = { result: FlashDone; error: null } | { result: null; error: UiError };

/** A firmware update as the backend reports it (AC-4PJRVJ.3): running, or ended with an
 * `outcome` not read yet (AC-AXHK2M.2). `stage` is null before its first stage, `replug`
 * is the prompt it waits on. */
export interface FlashStatus {
  device_id: string;
  stage: FlashStage | null;
  replug?: Omit<FlashReplug, "stage"> | null;
  outcome?: FlashEnd | null;
}

/** The update the backend is running, else the ended one it kept, which this read takes;
 * null when neither: a reloaded page picks it up. */
export async function firmwareFlashStatus(): Promise<FlashStatus | null> {
  if (web) return null;
  if (mock) return mock.firmwareFlashStatus();
  return invoke<FlashStatus | null>("firmware_flash_status");
}

/** What a PD clear answers. */
export interface PdCleared {
  /** Index of the next record: rows numbered below it were sent before the clear and
   * are stale. */
  next_index: number;
  /** Session time (ms) of the latest status sample the backend had at the clear, 0 if
   * none: status samples carry no index, so those at or below it are stale. */
  status_floor_ms: number;
}

/** Drops every captured PD record; resolves to {@link PdCleared}. */
export async function pdClear(): Promise<PdCleared> {
  if (web) return web.then((b) => b.pdClear());
  if (mock) return mock.pdClear();
  return invoke<PdCleared>("pd_clear");
}

/** Opens the save dialog proposing `suggestedName` and saves the PD capture so far as a
 * pdcap file (FR TGE83E). Resolves to the path and the number of records (`rows`), or
 * `null` when the dialog was cancelled. */
export async function pdSave(suggestedName: string): Promise<ExportResult | null> {
  if (web) return web.then((b) => b.pdSave(suggestedName));
  if (mock) return mock.pdSave(suggestedName);
  return invoke<ExportResult | null>("pd_save", { suggestedName });
}

/** Opens the save dialog proposing `suggestedName` and writes the chart image `data`
 * (for a PDF, the PNG the backend wraps in one page; FR AXHK2V). Resolves to the saved
 * path, or `null` when the dialog was cancelled. */
export async function saveChartImage(
  suggestedName: string,
  format: "png" | "svg" | "pdf",
  data: Uint8Array,
): Promise<string | null> {
  if (web) return web.then((b) => b.saveChartImage(suggestedName, format, data));
  if (mock) return mock.saveChartImage(suggestedName, format, data);
  return invoke<string | null>("save_chart_image", {
    suggestedName,
    format,
    data: Array.from(data),
  });
}

/** App and protocol-library versions for the About dialog. */
export interface AppInfo {
  appVersion: string;
  protocolVersion: string;
}

/** Reads the app and protocol-library versions from the backend. */
export async function appInfo(): Promise<AppInfo> {
  // The protocol library shares the workspace version with the app.
  if (web) return { appVersion: packageVersion, protocolVersion: packageVersion };
  if (mock) return mock.appInfo();
  const info = await invoke<{ app_version: string; protocol_version: string }>("app_info");
  return { appVersion: info.app_version, protocolVersion: info.protocol_version };
}

/** Shows `path` selected in the system file manager (Finder, Explorer, …). */
export async function revealPath(path: string): Promise<void> {
  if (web) return desktopOnly();
  if (mock) return mock.revealPath(path);
  await revealItemInDir(path);
}

// ---- Trigger panel (FR DGSQ6S; mirrors `src-tauri/src/trigger.rs`) ----

/** A protocol the trigger view has a section for. */
export type TriggerProtocol =
  "pd" | "qc" | "fcp" | "afc" | "sfcp" | "scp" | "vfcp" | "tfcp" | "ufcs";

/** Every protocol, PD first: one section each. */
export const TRIGGER_PROTOCOLS: readonly TriggerProtocol[] = [
  "pd",
  "qc",
  "fcp",
  "afc",
  "sfcp",
  "scp",
  "vfcp",
  "tfcp",
  "ufcs",
];

/** What a detection says about one protocol. */
export type SupportName = "supported" | "not-supported" | "unknown";

/** One line of the meter's automatic list. */
export interface DetectEntry {
  name: string;
  support: SupportName;
  vbus_mv: number | null;
  ibus_ma: number | null;
}

/** What a detection found. */
export interface Detection {
  bc12: SupportName;
  apple: SupportName;
  entries: DetectEntry[];
  complete: boolean;
  /** The D+/D- charging-mode hint line, absent without a measurement. */
  hint?: string | null;
}

export type OfferKind = "fixed" | "battery" | "variable" | "pps" | "spr-avs" | "epr-avs";

/** One PD offer: a button (fixed) or a slider range (PPS / AVS). */
export interface TriggerOffer {
  position: number;
  kind: OfferKind;
  min_voltage_mv: number;
  max_voltage_mv: number;
  max_current_ma: number | null;
  max_power_mw: number | null;
  epr: boolean;
}

/** The limits of one non-PD capability. */
export interface TriggerLimits {
  index: number | null;
  min_voltage_mv: number;
  max_voltage_mv: number;
  min_current_ma: number;
  max_current_ma: number;
  max_power_mw: number | null;
}

/** The meter's PD emulation settings, by name. */
export interface PdEmulation {
  pd_type: string;
  emarker: string;
  sink: string;
}

/** One protocol's section. */
export interface ProtocolSection {
  protocol: TriggerProtocol;
  supported: boolean;
  offers: TriggerOffer[];
  limits: TriggerLimits[];
  settings: PdEmulation | null;
}

/** One request. */
export type TriggerRequest =
  | { protocol: "pd"; position: number; voltage_mv: number | null; current_ma: number | null }
  | { protocol: "qc" | "fcp" | "afc" | "sfcp"; volts: number }
  | { protocol: "qc3"; voltage_mv: number }
  | { protocol: "scp" | "vfcp" | "tfcp"; voltage_mv: number; current_ma: number }
  | { protocol: "ufcs"; index: number; voltage_mv: number; current_ma: number };

/** The library risk class of a request. `above_5v_rememberable` is `above_5v` whose
 * dialog offers "don't ask again"; `armed` is covered by one and needs no dialog. */
export type RiskClass = "read_only" | "at_most_5v" | "above_5v" | "above_5v_rememberable" | "armed";

/** What the charger answered. */
export interface TriggerReply {
  command: string;
  requested_mv: number | null;
  requested_ma: number | null;
  delivered_mv: number | null;
  delivered_ma: number | null;
  reply: string | null;
}

/** Opens the trigger session on the connected meter. */
export async function triggerOpen(): Promise<void> {
  if (web) return web.then((b) => b.triggerOpen());
  if (mock) return mock.triggerOpen();
  await invoke("trigger_open");
}

/** BC 1.2, Apple and the automatic list. */
export async function triggerDetect(): Promise<Detection> {
  if (web) return web.then((b) => b.triggerDetect());
  if (mock) return mock.triggerDetect();
  return invoke<Detection>("trigger_detect");
}

/** One protocol's section (offers, limits, PD emulation settings). */
export async function triggerList(protocol: TriggerProtocol): Promise<ProtocolSection> {
  if (web) return web.then((b) => b.triggerList(protocol));
  if (mock) return mock.triggerList(protocol);
  return invoke<ProtocolSection>("trigger_list", { protocol });
}

/** The risk class of `request`: `above_5v` and `above_5v_rememberable` need the user's
 * confirmation. */
export async function triggerRisk(request: TriggerRequest): Promise<RiskClass> {
  if (web) return web.then((b) => b.triggerRisk(request));
  if (mock) return mock.triggerRisk(request);
  return invoke<RiskClass>("trigger_risk", { request });
}

/** Sends `request`; the backend refuses an above-5 V one unless `confirmed` or armed.
 * `remember` with `confirmed` asks it to arm "don't ask again" (FR AXHK2N). */
export async function triggerRequest(
  request: TriggerRequest,
  confirmed: boolean,
  remember = false,
): Promise<TriggerReply> {
  if (web) return web.then((b) => b.triggerRequest(request, confirmed, remember));
  if (mock) return mock.triggerRequest(request, confirmed, remember);
  return invoke<TriggerReply>("trigger_request", requestArgs(request, confirmed, remember));
}

/** The `trigger_request` arguments; `remember` goes only when it is true. */
function requestArgs(
  request: TriggerRequest,
  confirmed: boolean,
  remember?: boolean,
): Record<string, unknown> {
  return remember ? { request, confirmed, remember } : { request, confirmed };
}

/** Whether "don't ask again" is armed; false with no trigger session. */
export async function triggerArmed(): Promise<boolean> {
  if (web) return web.then((b) => b.triggerArmed());
  if (mock) return mock.triggerArmed();
  return invoke<boolean>("trigger_armed");
}

/** Ends "don't ask again"; does nothing with no trigger session. */
export async function triggerDisarm(): Promise<void> {
  if (web) return web.then((b) => b.triggerDisarm());
  if (mock) return mock.triggerDisarm();
  await invoke("trigger_disarm");
}

/** Whether VBUS collapsed under a contract; false with no trigger session. */
export async function triggerChargerGone(): Promise<boolean> {
  if (web) return web.then((b) => b.triggerChargerGone());
  if (mock) return mock.triggerChargerGone();
  return invoke<boolean>("trigger_charger_gone");
}

/** Whether PD is entered in the trigger session, so Read cable goes out (AC-AXHK2P.13);
 * false with no trigger session. Nothing is sent to the meter. */
export async function triggerPdEntered(): Promise<boolean> {
  if (web) return web.then((b) => b.triggerPdEntered());
  if (mock) return mock.triggerPdEntered();
  return invoke<boolean>("trigger_pd_entered");
}

/** Back to 5 V; the session stays open. `leaving`: the trigger view was left, which
 * the log names as the reason "don't ask again" ended (FR AXHK2N). */
export async function triggerBackTo5v(leaving = false): Promise<void> {
  if (web) return web.then((b) => b.triggerBackTo5v(leaving));
  if (mock) return mock.triggerBackTo5v();
  if (leaving) await invoke("trigger_back_to_5v", { leaving: true });
  else await invoke("trigger_back_to_5v");
}

/** Back to 5 V where the protocol allows it, then closes the session. */
export async function triggerClose(): Promise<void> {
  if (web) return web.then((b) => b.triggerClose());
  if (mock) return mock.triggerClose();
  await invoke("trigger_close");
}

// ---- Custom PD messages and the raw console (FR DGSQ6T; mirrors `src-tauri/src/trigger.rs`) ----

/** Where a built PD message goes: SOP (the charger), SOP' or SOP'' (the cable plugs). */
export type MessageSop = "sop" | "sop_prime" | "sop_double_prime";

/** A PD message from the builder: no data objects is a control message. */
export interface PdMessageRequest {
  sop: MessageSop;
  /** The 5-bit Message Type. */
  msg_type: number;
  /** 32-bit data objects. */
  data_objects: number[];
}

/** One console transcript line. */
export interface ConsoleLine {
  /** Clock time, ms since the Unix epoch. */
  at_ms: number;
  direction: "tx" | "rx";
  /** The line sent or received (a block's tag for a block). */
  text: string;
  /** A parsed binary block, in words; null for a text line. */
  block: string | null;
}

/** A cable's e-marker details. */
export interface CableView {
  vid: number;
  pid: number | null;
  active: boolean;
  max_voltage_v: number | null;
  max_current_ma: number | null;
  epr_capable: boolean;
  /** USB speed in words (`USB 3.2 Gen 1`); null when the cable sent no cable VDO. */
  speed: string | null;
  /** The name from our own vendor table; null for a VID it does not list. */
  vendor: string | null;
}

/** What one Read cable press found. */
export type CableRead = { kind: "found"; cable: CableView } | { kind: "no_answer" };

/** What sending a built message did. */
export interface PdSendReply {
  command: string;
  lines: ConsoleLine[];
  cable: CableView | null;
}

/** The library risk class of a built message; GoodCRC is a usage error. */
export async function triggerMessageRisk(message: PdMessageRequest): Promise<RiskClass> {
  if (web) return web.then((b) => b.triggerMessageRisk(message));
  if (mock) return mock.triggerMessageRisk(message);
  return invoke<RiskClass>("trigger_message_risk", { message });
}

/** Sends a built message; above 5 V needs `confirmed`. */
export async function triggerMessageSend(
  message: PdMessageRequest,
  confirmed: boolean,
): Promise<PdSendReply> {
  if (web) return web.then((b) => b.triggerMessageSend(message, confirmed));
  if (mock) return mock.triggerMessageSend(message, confirmed);
  return invoke<PdSendReply>("trigger_message_send", { message, confirmed });
}

/** One SOP' Discover Identity inside the open PD session, no dialog (FR AXHK2P). Takes
 * no argument: nothing from the page selects the exemption. */
export async function triggerCableRead(): Promise<CableRead> {
  if (web) return web.then((b) => b.triggerCableRead());
  if (mock) return mock.triggerCableRead();
  return invoke<CableRead>("trigger_cable_read");
}

/** The cable identity a PD capture holds, null when it holds none; sends nothing. */
export async function pdCableSeen(): Promise<CableView | null> {
  if (web) return web.then((b) => b.pdCableSeen());
  if (mock) return mock.pdCableSeen();
  return invoke<CableView | null>("pd_cable_seen");
}

/** The library risk class of a raw console line. */
export async function triggerConsoleRisk(line: string): Promise<RiskClass> {
  if (web) return web.then((b) => b.triggerConsoleRisk(line));
  if (mock) return mock.triggerConsoleRisk(line);
  return invoke<RiskClass>("trigger_console_risk", { line });
}

/** Sends a raw console line; above 5 V (any unparsable line too) needs `confirmed`. */
export async function triggerConsoleSend(line: string, confirmed: boolean): Promise<ConsoleLine[]> {
  if (web) return web.then((b) => b.triggerConsoleSend(line, confirmed));
  if (mock) return mock.triggerConsoleSend(line, confirmed);
  return invoke<ConsoleLine[]>("trigger_console_send", { line, confirmed });
}

// ---- One meter's commands (FR 1HHT03) ----

/** The commands of one meter's tile: every one names that meter, so an action never
 * reaches another meter. */
export interface MeterBackend {
  readonly deviceId: string;
  disconnect(): Promise<void>;
  cancelWait(): Promise<void>;
  setRate(rateSps: number): Promise<void>;
  startRecording(suggestedName: string, options?: RecordingOptions): Promise<string | null>;
  stopRecording(): Promise<RecordingSummary>;
  pauseRecording(): Promise<void>;
  resumeRecording(): Promise<void>;
  saveRecordingAs(path: string): Promise<string | null>;
  exportSession(
    suggestedName: string,
    onProgress: (progress: ExportProgress) => void,
  ): Promise<ExportResult | null>;
  exportSessionDb(
    suggestedName: string,
    onProgress: (progress: ExportProgress) => void,
  ): Promise<ExportResult | null>;
  stats(): Promise<StatsSnapshot>;
  historyRange<const S extends readonly HistorySeries[]>(
    series: S,
    t0Ms: number,
    t1Ms: number,
    buckets: number,
  ): Promise<BucketsFor<S>>;
  historyGaps(t0Ms: number, t1Ms: number): Promise<HistoryGap[]>;
  historyFoldBoundary(): Promise<number | null>;
  rangeStats(t0Ms: number, t1Ms: number): Promise<RangeStats | null>;
  runProgress(query: RunProgressQuery): Promise<RunProgress | null>;
  resetStats(): Promise<void>;
  clearHistory(): Promise<void>;
  pdStart(): Promise<void>;
  pdStop(): Promise<void>;
  pdDetails(index: number, opened?: boolean): Promise<PdDetails>;
  pdClear(): Promise<PdCleared>;
  pdSave(suggestedName: string): Promise<ExportResult | null>;
  meterSettings(): Promise<MeterSettings>;
  meterSettingsWrite(changes: SettingChanges): Promise<MeterSettings>;
  recordsList(): Promise<RecordList>;
  recordDownload(index: number): Promise<OfflineRecord>;
  recordsDelete(target: number | "all", confirm: string): Promise<RecordDeleteResult>;
  triggerOpen(): Promise<void>;
  triggerDetect(): Promise<Detection>;
  triggerList(protocol: TriggerProtocol): Promise<ProtocolSection>;
  triggerRisk(request: TriggerRequest): Promise<RiskClass>;
  triggerRequest(
    request: TriggerRequest,
    confirmed: boolean,
    remember?: boolean,
  ): Promise<TriggerReply>;
  triggerArmed(): Promise<boolean>;
  triggerDisarm(): Promise<void>;
  triggerChargerGone(): Promise<boolean>;
  triggerPdEntered(): Promise<boolean>;
  triggerBackTo5v(leaving?: boolean): Promise<void>;
  triggerClose(): Promise<void>;
  triggerMessageRisk(message: PdMessageRequest): Promise<RiskClass>;
  triggerMessageSend(message: PdMessageRequest, confirmed: boolean): Promise<PdSendReply>;
  triggerCableRead(): Promise<CableRead>;
  pdCableSeen(): Promise<CableView | null>;
  triggerConsoleRisk(line: string): Promise<RiskClass>;
  triggerConsoleSend(line: string, confirmed: boolean): Promise<ConsoleLine[]>;
}

/** One meter's commands without its device id: {@link meterBackend}'s, or
 * {@link currentMeterCommands}. */
export type MeterCommands = Omit<MeterBackend, "deviceId">;

/** The commands without a device id, which act on the backend's current session: the meter
 * connected last (FR 1HHT03). */
export const currentMeterCommands: MeterCommands = {
  disconnect,
  cancelWait,
  setRate,
  startRecording,
  stopRecording,
  pauseRecording,
  resumeRecording,
  saveRecordingAs,
  exportSession,
  exportSessionDb,
  stats,
  historyRange,
  historyGaps,
  historyFoldBoundary,
  rangeStats,
  runProgress,
  resetStats,
  clearHistory,
  pdStart,
  pdStop,
  pdDetails,
  pdClear,
  pdSave,
  meterSettings,
  meterSettingsWrite,
  recordsList,
  recordDownload,
  recordsDelete,
  triggerOpen,
  triggerDetect,
  triggerList,
  triggerRisk,
  triggerRequest,
  triggerArmed,
  triggerDisarm,
  triggerChargerGone,
  triggerPdEntered,
  triggerBackTo5v,
  triggerClose,
  triggerMessageRisk,
  triggerMessageSend,
  triggerCableRead,
  pdCableSeen,
  triggerConsoleRisk,
  triggerConsoleSend,
};

/** The browser backend (`VITE_BACKEND=web`, FR QFTCX0) as the dispatch calls it: the
 * current-session commands, plus devices, the browser's picker and one meter's commands. */
export interface WebBackendApi extends MeterCommands {
  readonly features: Readonly<Features>;
  listDevices(): Promise<DeviceListing[]>;
  watchDevices(onChange: (devices: DeviceListing[]) => void): Promise<() => void>;
  requestMeter(): Promise<DeviceListing | null>;
  /** The browser's WebHID picker (AC-QFTCWZ.3); `null` when none was picked or there is
   * no WebHID. */
  requestHidMeter(): Promise<DeviceListing | null>;
  connect(
    deviceId: string,
    rateSps: number,
    onEvent: (event: BackendEvent) => void,
    source?: MeasurementSource,
  ): Promise<void>;
  attach(
    onEvent: (event: BackendEvent) => void,
    deviceId?: string,
  ): Promise<AttachedSession | null>;
  meter(deviceId: string): MeterCommands;
  /** The connected meters' ids, in the order they were connected. */
  connectedMeters(): Promise<string[]>;
  /** Downloads the chart image `data` as `suggestedName`; resolves to the file name. */
  saveChartImage(
    suggestedName: string,
    format: "png" | "svg" | "pdf",
    data: Uint8Array,
  ): Promise<string | null>;
  /** Open recording (FR AHTZW3): the browser's file picker for CSV and `.db` files;
   * `null` when it was dismissed or Cancel ended the open. */
  openRecordingFile(onProgress: (progress: OpenProgress) => void): Promise<FileSummary | null>;
  cancelRecordingFile(): Promise<void>;
  closeRecordingFile(): Promise<void>;
  recordingFileRange<const S extends readonly HistorySeries[]>(
    series: S,
    t0Ms: number,
    t1Ms: number,
    buckets: number,
  ): Promise<BucketsFor<S>>;
  recordingFileRangeStats(t0Ms: number, t1Ms: number): Promise<RangeStats | null>;
  recordingFileRunProgress(query: RunProgressQuery): Promise<RunProgress | null>;
  /** The startup logo (FR W8SHXB): the browser's file picker answers a token that
   * stands in for the desktop path. */
  logoChoose(): Promise<string | null>;
  logoPreview(path: string, alpha: boolean): Promise<LogoPreview>;
  logoPreviewBuiltin(name: BuiltinLogoName): Promise<LogoPreview>;
  logoRead(
    deviceId: string,
    full: boolean,
    onProgress?: (progress: LogoProgress) => void,
  ): Promise<LogoRead>;
  logoSettingsCheck(deviceId: string): Promise<LogoSettingsCheck>;
  /** Desktop-only meter writes (AC-QFTCX0.6): each rejects without sending anything. */
  firmwareFlash(
    deviceId: string,
    path: string,
    confirmation: string,
    onStage: (stage: FlashEvent) => void,
    agreed?: FlashHold[],
  ): Promise<FlashDone>;
  /** The startup logo writes (FR W8SHXB), run in the browser. */
  logoUpload(
    deviceId: string,
    path: string,
    alpha: boolean,
    onProgress: (progress: LogoProgress) => void,
  ): Promise<LogoWritten>;
  logoUploadBuiltin(
    deviceId: string,
    name: BuiltinLogoName,
    onProgress: (progress: LogoProgress) => void,
  ): Promise<LogoWritten>;
  logoRemove(deviceId: string, onProgress: (progress: LogoProgress) => void): Promise<LogoWritten>;
}

/** The commands of meter `deviceId` (FR 1HHT03): each invokes its Tauri command with
 * `deviceId`, or the mock's {@link MockBackend.meter}. */
export function meterBackend(deviceId: string): MeterBackend {
  if (web) {
    const commands = Object.fromEntries(
      Object.keys(currentMeterCommands).map((name) => [
        name,
        (...args: unknown[]) =>
          web.then((b) => {
            const command = b.meter(deviceId)[name as keyof MeterCommands] as (
              ...a: unknown[]
            ) => Promise<unknown>;
            return command(...args);
          }),
      ]),
    ) as unknown as MeterCommands;
    return { deviceId, ...commands };
  }
  if (mock) return { deviceId, ...mock.meter(deviceId) };
  const call = <T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> =>
    invoke<T>(cmd, { deviceId, ...args });
  return {
    deviceId,
    disconnect: async () => {
      await call("disconnect");
    },
    cancelWait: async () => {
      await call("cancel_wait");
    },
    setRate: async (rateSps) => {
      await call("set_rate", { rateSps });
    },
    startRecording: (suggestedName, options) => call("start_recording", { suggestedName, options }),
    stopRecording: () => call("stop_recording"),
    pauseRecording: () => call("recording_pause"),
    resumeRecording: () => call("recording_resume"),
    saveRecordingAs: (path) => call("save_recording_as", { path }),
    exportSession: (suggestedName, onProgress) => {
      const channel = new Channel<ExportProgress>();
      channel.onmessage = onProgress;
      return call("export_session", { suggestedName, onProgress: channel });
    },
    exportSessionDb: (suggestedName, onProgress) => {
      const channel = new Channel<ExportProgress>();
      channel.onmessage = onProgress;
      return call("export_session_db", { suggestedName, onProgress: channel });
    },
    stats: () => call("stats"),
    historyRange: (series, t0Ms, t1Ms, buckets) =>
      call("history_range", { series, ...wholeMs(t0Ms, t1Ms), buckets }),
    historyGaps: (t0Ms, t1Ms) => call("history_gaps", wholeMs(t0Ms, t1Ms)),
    historyFoldBoundary: () => call("history_fold_boundary"),
    rangeStats: (t0Ms, t1Ms) => call("range_stats", wholeMs(t0Ms, t1Ms)),
    runProgress: (query) => call("run_progress", { query: wireQuery(query) }),
    resetStats: async () => {
      await call("reset_stats");
    },
    clearHistory: async () => {
      await call("clear_history");
    },
    pdStart: async () => {
      await call("pd_start");
    },
    pdStop: async () => {
      await call("pd_stop");
    },
    pdDetails: (index, opened = false) =>
      call("pd_details", opened ? { index, opened } : { index }),
    pdClear: () => call("pd_clear"),
    pdSave: (suggestedName) => call("pd_save", { suggestedName }),
    meterSettings: () => call("meter_settings"),
    meterSettingsWrite: (changes) => call("meter_settings_write", { changes }),
    recordsList: () => call("records_list"),
    recordDownload: (index) => call("record_download", { index }),
    recordsDelete: (target, confirm) =>
      call("records_delete", { index: target === "all" ? null : target, confirm }),
    triggerOpen: async () => {
      await call("trigger_open");
    },
    triggerDetect: () => call("trigger_detect"),
    triggerList: (protocol) => call("trigger_list", { protocol }),
    triggerRisk: (request) => call("trigger_risk", { request }),
    triggerRequest: (request, confirmed, remember) =>
      call("trigger_request", requestArgs(request, confirmed, remember)),
    triggerArmed: () => call("trigger_armed"),
    triggerDisarm: async () => {
      await call("trigger_disarm");
    },
    triggerChargerGone: () => call("trigger_charger_gone"),
    triggerPdEntered: () => call("trigger_pd_entered"),
    triggerBackTo5v: async (leaving = false) => {
      if (leaving) await call("trigger_back_to_5v", { leaving: true });
      else await call("trigger_back_to_5v");
    },
    triggerClose: async () => {
      await call("trigger_close");
    },
    triggerMessageRisk: (message) => call("trigger_message_risk", { message }),
    triggerMessageSend: (message, confirmed) =>
      call("trigger_message_send", { message, confirmed }),
    triggerCableRead: () => call("trigger_cable_read"),
    pdCableSeen: () => call("pd_cable_seen"),
    triggerConsoleRisk: (line) => call("trigger_console_risk", { line }),
    triggerConsoleSend: (line, confirmed) => call("trigger_console_send", { line, confirmed }),
  };
}
