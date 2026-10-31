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
  | "out_of_sync"
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
  "out_of_sync",
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
 * `onEvent`. Resolves to `null` when no session is running. Without `deviceId` it is the
 * current meter's session; with one, that meter's (FR 1HHT03). */
export async function attach(
  onEvent: (event: BackendEvent) => void,
  deviceId?: string,
): Promise<AttachedSession | null> {
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
  if (mock) return mock.connectedIds();
  return invoke<string[]>("running_meters");
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

/** The raw bytes and field tree of PD record `index`: of the capture opened from a file
 * when `opened`, of the live capture otherwise. */
export async function pdDetails(index: number, opened = false): Promise<PdDetails> {
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
}

/** Reads the connected meter's settings (GET_DATA 0x008 only); rejects with the
 * backend's error, e.g. without a meter or on a CRC mismatch. */
export async function meterSettings(): Promise<MeterSettings> {
  if (mock) return mock.meterSettings();
  return invoke<MeterSettings>("meter_settings");
}

/** A settings field the panel may change (FR JKWT3W); calibration, auto-rotate and
 * the startup logo never are. */
export type EditableSettingKey =
  "language" | "brightness" | "dim_timeout" | "theme" | "rotation" | "sample_rate";

/** A settings change: the new wire code of each changed field (FR JKWT3W). */
export type SettingChanges = Partial<Record<EditableSettingKey, number>>;

/** Writes `changes` to the connected meter (one settings write between two reads) and
 * resolves to the settings as read back; rejects with the backend's error, e.g. when
 * the re-read does not match. Nothing is retried. */
export async function meterSettingsWrite(changes: SettingChanges): Promise<MeterSettings> {
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
  if (mock) return mock.recordsList();
  return invoke<RecordList>("records_list");
}

/** Downloads offline record `index` (1-based): its own window, read in chunks of at most
 * 256 bytes. */
export async function recordDownload(index: number): Promise<OfflineRecord> {
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
  if (mock) return mock.pdOpen();
  return invoke<PdOpened | null>("pd_open");
}

/** Opens the open dialog and imports the chosen vendor PD log database (`.sqlite`)
 * read-only (FR TGE83E, AC .6); needs no meter. Resolves like {@link pdOpen}: the
 * imported capture, or `null` when the dialog was cancelled. */
export async function pdImport(): Promise<PdOpened | null> {
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
  release_notes_en: string;
}

/** Reads the firmware container at `path` and resolves to its metadata; needs no meter.
 * A corrupt file or one that is not a firmware container rejects with a usage error. */
export async function firmwareInspect(path: string): Promise<FirmwareFileInfo> {
  if (mock) return mock.firmwareInspect(path);
  return invoke<FirmwareFileInfo>("firmware_inspect", { path });
}

/** Opens the open dialog filtered to firmware containers (`.mencrypt`) (FR V9SK55,
 * AC .2). Resolves to the chosen path, or `null` when the dialog was cancelled. */
export async function firmwareChoose(): Promise<string | null> {
  if (mock) return mock.firmwareChoose();
  return invoke<string | null>("firmware_choose");
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
 * outcome, or rejects with the error and its recovery steps. */
export async function firmwareFlash(
  deviceId: string,
  path: string,
  confirmation: string,
  onStage: (stage: FlashEvent) => void,
): Promise<FlashDone> {
  if (mock) return mock.firmwareFlash(deviceId, path, confirmation, onStage);
  const channel = new Channel<FlashEvent>();
  channel.onmessage = onStage;
  return invoke<FlashDone>("firmware_flash", {
    deviceId,
    path,
    confirmation,
    onProgress: channel,
  });
}

/** Asks the running update to stop (AC-4PJRVJ.1); rejects once its erase has begun. */
export async function firmwareCancel(): Promise<void> {
  if (mock) return mock.firmwareCancel();
  return invoke<void>("firmware_cancel");
}

/** A running firmware update as the backend reports it (AC-4PJRVJ.3); `stage` is null
 * before its first stage. */
export interface FlashStatus {
  device_id: string;
  stage: FlashStage | null;
}

/** The update the backend is running, null when none: a reloaded page picks it up. */
export async function firmwareFlashStatus(): Promise<FlashStatus | null> {
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
  if (mock) return mock.pdClear();
  return invoke<PdCleared>("pd_clear");
}

/** Opens the save dialog proposing `suggestedName` and saves the PD capture so far as a
 * pdcap file (FR TGE83E). Resolves to the path and the number of records (`rows`), or
 * `null` when the dialog was cancelled. */
export async function pdSave(suggestedName: string): Promise<ExportResult | null> {
  if (mock) return mock.pdSave(suggestedName);
  return invoke<ExportResult | null>("pd_save", { suggestedName });
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

/** The library risk class of a request. */
export type RiskClass = "read_only" | "at_most_5v" | "above_5v";

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
  if (mock) return mock.triggerOpen();
  await invoke("trigger_open");
}

/** BC 1.2, Apple and the automatic list. */
export async function triggerDetect(): Promise<Detection> {
  if (mock) return mock.triggerDetect();
  return invoke<Detection>("trigger_detect");
}

/** One protocol's section (offers, limits, PD emulation settings). */
export async function triggerList(protocol: TriggerProtocol): Promise<ProtocolSection> {
  if (mock) return mock.triggerList(protocol);
  return invoke<ProtocolSection>("trigger_list", { protocol });
}

/** The risk class of `request`: `above_5v` needs the user's confirmation. */
export async function triggerRisk(request: TriggerRequest): Promise<RiskClass> {
  if (mock) return mock.triggerRisk(request);
  return invoke<RiskClass>("trigger_risk", { request });
}

/** Sends `request`; the backend refuses an above-5 V one unless `confirmed`. */
export async function triggerRequest(
  request: TriggerRequest,
  confirmed: boolean,
): Promise<TriggerReply> {
  if (mock) return mock.triggerRequest(request, confirmed);
  return invoke<TriggerReply>("trigger_request", { request, confirmed });
}

/** Back to 5 V; the session stays open. */
export async function triggerBackTo5v(): Promise<void> {
  if (mock) return mock.triggerBackTo5v();
  await invoke("trigger_back_to_5v");
}

/** Back to 5 V where the protocol allows it, then closes the session. */
export async function triggerClose(): Promise<void> {
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
}

/** What sending a built message did. */
export interface PdSendReply {
  command: string;
  lines: ConsoleLine[];
  cable: CableView | null;
}

/** The library risk class of a built message; GoodCRC is a usage error. */
export async function triggerMessageRisk(message: PdMessageRequest): Promise<RiskClass> {
  if (mock) return mock.triggerMessageRisk(message);
  return invoke<RiskClass>("trigger_message_risk", { message });
}

/** Sends a built message; above 5 V needs `confirmed`. */
export async function triggerMessageSend(
  message: PdMessageRequest,
  confirmed: boolean,
): Promise<PdSendReply> {
  if (mock) return mock.triggerMessageSend(message, confirmed);
  return invoke<PdSendReply>("trigger_message_send", { message, confirmed });
}

/** The library risk class of a raw console line. */
export async function triggerConsoleRisk(line: string): Promise<RiskClass> {
  if (mock) return mock.triggerConsoleRisk(line);
  return invoke<RiskClass>("trigger_console_risk", { line });
}

/** Sends a raw console line; above 5 V (any unparsable line too) needs `confirmed`. */
export async function triggerConsoleSend(line: string, confirmed: boolean): Promise<ConsoleLine[]> {
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
  startRecording(suggestedName: string): Promise<string | null>;
  stopRecording(): Promise<RecordingSummary>;
  exportSession(
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
  triggerRequest(request: TriggerRequest, confirmed: boolean): Promise<TriggerReply>;
  triggerBackTo5v(): Promise<void>;
  triggerClose(): Promise<void>;
  triggerMessageRisk(message: PdMessageRequest): Promise<RiskClass>;
  triggerMessageSend(message: PdMessageRequest, confirmed: boolean): Promise<PdSendReply>;
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
  exportSession,
  stats,
  historyRange,
  historyGaps,
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
  triggerBackTo5v,
  triggerClose,
  triggerMessageRisk,
  triggerMessageSend,
  triggerConsoleRisk,
  triggerConsoleSend,
};

/** The commands of meter `deviceId` (FR 1HHT03): each invokes its Tauri command with
 * `deviceId`, or the mock's {@link MockBackend.meter}. */
export function meterBackend(deviceId: string): MeterBackend {
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
    startRecording: (suggestedName) => call("start_recording", { suggestedName }),
    stopRecording: () => call("stop_recording"),
    exportSession: (suggestedName, onProgress) => {
      const channel = new Channel<ExportProgress>();
      channel.onmessage = onProgress;
      return call("export_session", { suggestedName, onProgress: channel });
    },
    stats: () => call("stats"),
    historyRange: (series, t0Ms, t1Ms, buckets) =>
      call("history_range", { series, ...wholeMs(t0Ms, t1Ms), buckets }),
    historyGaps: (t0Ms, t1Ms) => call("history_gaps", wholeMs(t0Ms, t1Ms)),
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
    triggerRequest: (request, confirmed) => call("trigger_request", { request, confirmed }),
    triggerBackTo5v: async () => {
      await call("trigger_back_to_5v");
    },
    triggerClose: async () => {
      await call("trigger_close");
    },
    triggerMessageRisk: (message) => call("trigger_message_risk", { message }),
    triggerMessageSend: (message, confirmed) =>
      call("trigger_message_send", { message, confirmed }),
    triggerConsoleRisk: (line) => call("trigger_console_risk", { line }),
    triggerConsoleSend: (line, confirmed) => call("trigger_console_send", { line, confirmed }),
  };
}
