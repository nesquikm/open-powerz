/**
 * A scripted stand-in for the Tauri backend, so the web build runs in a
 * plain browser (`VITE_BACKEND=mock`): the Playwright UI suite and the
 * visual checks drive it. Its scenarios follow the simulator's shapes in
 * `crates/powerz-sim`; like the worker, it reports the snapshot temperature
 * at every rate, 1000 SPS included.
 */

import type {
  BuiltinLogoName,
  LogoPreview,
  LogoProgress,
  LogoRead,
  LogoSettingsCheck,
  LogoWritten,
  AppInfo,
  AttachedSession,
  BackendEvent,
  BucketsFor,
  DeviceListing,
  ExportProgress,
  ExportFolded,
  ExportResult,
  FileSummary,
  FirmwareFileInfo,
  FlashDone,
  FlashEvent,
  FlashHold,
  FlashStatus,
  Gap,
  HistoryGap,
  HistorySeries,
  MeterSettingField,
  MeterSettings,
  OfflineRecord,
  OfflineRecordEntry,
  OfflineRecordSample,
  OpenProgress,
  RecordDeleteResult,
  SettingChanges,
  RecordList,
  PdDetails,
  PdCleared,
  PdOpened,
  PdRow,
  PdStatusSample,
  PdStopReason,
  ProtocolSection,
  RecordingEnd,
  RecordingOptions,
  RecordingSummary,
  RiskClass,
  Sample,
  StatsSnapshot,
  Detection,
  CableRead,
  CableView,
  ConsoleLine,
  PdMessageRequest,
  PdSendReply,
  TriggerLimits,
  TriggerOffer,
  TriggerProtocol,
  TriggerReply,
  TriggerRequest,
  UiError,
  MeasurementSource,
  MeterCommands,
  RangeStats,
  RunProgress,
  RunProgressQuery,
} from "./backend";
import { lineRefusal, messageRefusal } from "./console.svelte";
import { HistoryColumns, SEED_HEADROOM, SessionStats } from "./history";
import {
  PD_CYCLE_MS,
  pdContractAt,
  pdDetailsOf,
  pdIsCableDiscover,
  pdRecordTime,
  pdRow,
} from "./mockPd";
import { DEFAULT_MAX_ROWS } from "./settings";

const SCENARIOS = ["idle", "load-5v-1a", "pd-9v-2a", "noisy-negative"] as const;
type Scenario = (typeof SCENARIOS)[number];

/** Batches go out every 25 ms (40 Hz). The app's backend sends at most 20 Hz (FR HR2JAG);
 * the mock keeps its faster cadence, which the browser suite's timings were tuned on. */
const BATCH_MS = 25;

/** Recording progress goes out every 500 ms, like the backend's. */
const PROGRESS_MS = 500;

function usage(message: string): UiError {
  return { kind: "usage", message, hint: null };
}

/** `firmware-running`: the update a reloaded page finds running (AC-4PJRVJ.3). */
const MOCK_RUNNING_FLASH: FlashStatus = {
  device_id: "demo:idle",
  stage: { stage: "writing", done: 4096, total: 8192 },
  replug: null,
  outcome: null,
};

/** Why the mock refuses a request while a firmware update holds the meter (FR V9SK55). */
const FLASH_REFUSAL = "A firmware update is running. Wait for it to finish before using the meter.";

/** The recovery steps of a failed update, as the backend gives them (FR V9SK55, AC .5). */
const FLASH_RECOVERY_HINT =
  "Hold the Back button while plugging the meter in to start it in bootloader mode, then run the update again.";

/** What a stop the user asked for says, as the backend gives it (AC-AXHK2M.13). */
const FLASH_STOPPED =
  "The update was stopped before anything was erased. The meter may be waiting in its bootloader.";

/** How long each stage of a mock update takes, ms: slow enough for the UI suite to see
 * every stage and the blocked controls. The write stage takes `MOCK_FLASH_WRITES` steps. */
const MOCK_FLASH_MS = {
  switching: 800,
  erasing: 800,
  write: 120,
  verifying: 600,
  finished: 200,
  returning: 800,
  replug: 1200,
} as const;
const MOCK_FLASH_WRITES = 20;

/** The engine's message for a mock failure at each stage. */
const MOCK_FLASH_FAILURES: Record<string, string> = {
  switching: "the meter is not in bootloader mode (application mode after JUMP_DFU)",
  erasing: "erase error: no ACCEPT after the erase",
  writing: "download error at write 3: no SYNC",
  verifying: "verify error: no FINISHED after the last write",
  returning: "verify error: the meter reports version 2.0.6 after the update but the file is 2.0.7",
};

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function scenarioOf(deviceId: string): Scenario | undefined {
  const name = deviceId.replace(/^demo:/, "");
  return SCENARIOS.find((s) => s === name);
}

/** Writes the scenario's reading at device time `tMs` into `out` (so a large seed can
 * reuse one object instead of allocating a sample per point). */
function writeSample(scenario: Scenario, tMs: number, out: Sample): Sample {
  const t = tMs / 1000;
  out.t_ms = tMs;
  out.cc1_v = 1.68;
  out.cc2_v = 0;
  out.dp_v = 0.6;
  out.dm_v = 0.6;
  switch (scenario) {
    case "idle":
      out.vbus_v = 0.004;
      out.ibus_a = 0;
      out.cc1_v = 0.08;
      break;
    case "load-5v-1a":
      out.vbus_v = 5 + 0.01 * Math.sin(t * 7);
      out.ibus_a = 1 + 0.02 * Math.sin(t * 3);
      break;
    case "pd-9v-2a":
      out.vbus_v = 9 + 0.02 * Math.sin(t * 5);
      out.ibus_a = 2 + 0.05 * Math.sin(t * 2);
      out.cc1_v = 1.05;
      break;
    case "noisy-negative":
      out.vbus_v = 5 + 0.05 * Math.sin(t * 31);
      out.ibus_a = -0.2 + 0.05 * Math.sin(t * 17);
      break;
  }
  return out;
}

/** The scenario's reading at device time `tMs`. */
function sampleAt(scenario: Scenario, tMs: number): Sample {
  return writeSample(scenario, tMs, {
    t_ms: 0,
    vbus_v: 0,
    ibus_a: 0,
    cc1_v: 0,
    cc2_v: 0,
    dp_v: 0,
    dm_v: 0,
  });
}

/** Rows of each export the demo keeps as text, for a test to compare. */
const EXPORT_CSV_HEAD_ROWS = 2_000;

/** The page URL's query string (`location.search`), if there is a page. */
function pageSearch(): string | undefined {
  return (globalThis as { location?: { search?: string } }).location?.search;
}

/** Parameter `name` of the query string `search`, or null when absent. */
function urlParam(search: string | undefined, name: string): string | null {
  return search ? new URLSearchParams(search).get(name) : null;
}

/** The largest synthetic seed (a 20M seed is already about 640 MB of columns). */
export const MAX_SYNTHETIC_SAMPLES = 20_000_000;

/** `?synthetic=<n>` in the page URL seeds an `n`-sample history (at most
 * {@link MAX_SYNTHETIC_SAMPLES}) on connect, for the Playwright pan-performance check
 * (AC-AH2AEM.4); dev and mock builds only, and the device list is unchanged. */
export function syntheticFromUrl(search: string | undefined, dev: boolean): number | null {
  const raw = dev ? urlParam(search, "synthetic") : null;
  if (raw === null) return null;
  const n = Number(raw);
  return Number.isInteger(n) && n > 0 ? Math.min(n, MAX_SYNTHETIC_SAMPLES) : null;
}

/** `?pdsynthetic=<n>` in the page URL seeds `n` PD records when the capture starts, for
 * the PD table's performance check (AC-TGE83C.6); mock builds only. */
export function pdSyntheticFromUrl(search: string | undefined): number | null {
  const raw = urlParam(search, "pdsynthetic");
  if (raw === null) return null;
  const n = Number(raw);
  return Number.isInteger(n) && n > 0 ? n : null;
}

/** `?foldat=<n>` in the page URL: the mock's full-resolution history limit
 * ({@link MockOptions.historyFullSamples}, FR AXHK2W), a positive whole number of
 * samples; anything else is ignored. */
export function foldAtFromUrl(search: string | undefined): number | null {
  const raw = urlParam(search, "foldat");
  if (raw === null || raw.trim() === "") return null;
  const n = Number(raw);
  return Number.isInteger(n) && n > 0 ? n : null;
}

/** A scripted record-delete failure (FR JKWT3V): `backup` fails the backup (nothing is
 * deleted), `mismatch` keeps the old table so the re-read does not match. */
export type RecordsFault = "backup" | "mismatch";

/** `?recordsfault=backup|mismatch` in the page URL scripts a record-delete failure
 * (FR JKWT3V); anything else is ignored. */
export function recordsFaultFromUrl(search: string | undefined): RecordsFault | null {
  const raw = urlParam(search, "recordsfault");
  return raw === "backup" || raw === "mismatch" ? raw : null;
}

/** A scripted settings-write failure (FR JKWT3W): `mismatch` keeps the old values so
 * the re-read does not match. */
export type SettingsFault = "mismatch";

/** `?settingsfault=mismatch` in the page URL scripts a settings-write failure
 * (FR JKWT3W); anything else is ignored. */
export function settingsFaultFromUrl(search: string | undefined): SettingsFault | null {
  return urlParam(search, "settingsfault") === "mismatch" ? "mismatch" : null;
}

/** A scripted recording-file open (FR AXHK2S): `refused` refuses the file, `slow` holds the
 * open until Cancel, `truncated` opens a file past the row limit, `dialog` stays in the open
 * dialog until Cancel, `unknown-total` holds an open that knows no total until Cancel, `dip`
 * opens the usual file with VBUS held at {@link MOCK_DIP_VBUS_V} for its first
 * {@link MOCK_DIP_MS} ms (FR PBDEQE). */
export type RecordingFileFault =
  "refused" | "slow" | "truncated" | "dialog" | "unknown-total" | "dip";

const RECORDING_FILE_FAULTS: readonly RecordingFileFault[] = [
  "refused",
  "slow",
  "truncated",
  "dialog",
  "unknown-total",
  "dip",
];

/** A `dip` file's VBUS over its first {@link MOCK_DIP_MS} ms. */
const MOCK_DIP_VBUS_V = 3;
const MOCK_DIP_MS = 1_000;

/** `?recordingfile=<a RecordingFileFault>` in the page URL scripts the open (FR AXHK2S);
 * anything else is ignored. */
export function recordingFileFaultFromUrl(search: string | undefined): RecordingFileFault | null {
  const raw = urlParam(search, "recordingfile");
  return RECORDING_FILE_FAULTS.find((fault) => fault === raw) ?? null;
}

/** `?launchfile=1` in the page URL hands the mock its demo recording at launch (FR
 * M078V5); anything else is ignored. */
export function launchFileFromUrl(search: string | undefined): boolean {
  return urlParam(search, "launchfile") === "1";
}

/** The recording file the mock opens (FR AXHK2S). */
export const MOCK_RECORDING_FILE = {
  name: "demo-recording.csv",
  rows: 12_345,
  skipped: 3,
} as const;

/** Readable rows past the limit in a `truncated` file. */
const MOCK_LEFT_OUT = 2_500_000;

/** A scripted logo picture (FR MEHQC0): `small` chooses one with a side below 48. */
export type LogoFile = "small";

/** `?logofile=small` in the page URL chooses the small logo picture (FR MEHQC0);
 * anything else is ignored. */
export function logoFileFromUrl(search: string | undefined): LogoFile | null {
  return urlParam(search, "logofile") === "small" ? "small" : null;
}

/** The logo picture the mock chooses (FR MEHQC0): 100 × 60. */
export const MOCK_LOGO_FILE = "/mock/logo.png";
/** The small logo picture (`?logofile=small`): 240 × 24 once fitted, below 48 high. */
export const MOCK_LOGO_SMALL_FILE = "/mock/logo-small.png";

/** The size each mock logo picture encodes to. */
const MOCK_LOGO_SIZES: Record<string, { width: number; height: number }> = {
  [MOCK_LOGO_FILE]: { width: 100, height: 60 },
  [MOCK_LOGO_SMALL_FILE]: { width: 240, height: 24 },
};

/** A gradient of `width × height` as RGBA8888, base64; `variant` shifts its colours. */
function mockLogoPixels(width: number, height: number, variant = 0): string {
  let out = "";
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      out += String.fromCharCode(
        (x * 2 + variant) & 0xff,
        (y * 4) & 0xff,
        ((x + y) * 3) & 0xff,
        0xff,
      );
    }
  }
  return btoa(out);
}

/** The built-in logos' colour variants (FR 70X5YX): both are 200 × 200. */
const MOCK_BUILTIN_VARIANTS: Record<BuiltinLogoName, number> = {
  surf: 64,
  "surf-wordmark": 128,
};

/** The preview of built-in logo `name` like the backend's: 200 × 200, transparent; any
 * other name is refused naming the two. */
function mockBuiltinPreview(name: BuiltinLogoName): LogoPreview {
  const variant = MOCK_BUILTIN_VARIANTS[name];
  if (variant === undefined) {
    throw usage(`unknown built-in logo "${name}"; expected one of: surf, surf-wordmark`);
  }
  return mockLogoSized(200, 200, true, variant);
}

/** The preview of mock logo picture `path` like the backend's (16-byte header, 2 bytes a
 * pixel opaque or 3 with alpha, padded to 256-byte chunks). */
function mockLogoPreview(path: string, alpha: boolean): LogoPreview | null {
  const size = MOCK_LOGO_SIZES[path];
  if (size === undefined) return null;
  return mockLogoSized(size.width, size.height, alpha);
}

/** A mock preview of a `width × height` logo in the given mode. */
function mockLogoSized(width: number, height: number, alpha: boolean, variant = 0): LogoPreview {
  const blobSize = 16 + width * height * (alpha ? 3 : 2);
  const chunks = Math.ceil(blobSize / 256);
  return {
    width,
    height,
    mode: alpha ? "alpha" : "opaque",
    blob_size: blobSize,
    padded_size: chunks * 256,
    chunks,
    small_side_warning: width < 48 || height < 48,
    rgba: mockLogoPixels(width, height, variant),
  };
}

/** {@link mockLogoPreview}, refusing any other path like a file that is no picture. */
function mockLogoPicture(path: string, alpha: boolean): LogoPreview {
  const preview = mockLogoPreview(path, alpha);
  if (preview === null) throw usage(`${path}: cannot load image file: not a picture`);
  return preview;
}

/** `?logofault=<k>` in the page URL fails a logo upload at chunk `k` (0-based, FR
 * MEHQC0); anything but a whole number ≥ 0 is ignored. */
export function logoFaultFromUrl(search: string | undefined): number | null {
  const raw = urlParam(search, "logofault");
  if (raw === null || raw.trim() === "") return null;
  const n = Number(raw);
  return Number.isInteger(n) && n >= 0 ? n : null;
}

/** The menu-save steps, the same text as the library's `MENU_SAVE_STEPS` (FR QG6RNY). */
export const MENU_SAVE_STEPS =
  "On the meter: Settings → startup logo duration → pick a value → save. Don't unplug the meter first.";

/** How long after a logo write the demo meter does its own menu save, ms (FR QG6RNY). */
export const MOCK_MENU_SAVE_MS = 3000;

/** How the demo meter's settings get saved after a logo write (FR QG6RNY): `never`,
 * `calibration` (the first check after the save time finds changed calibration) or
 * `saved` (the write's own fresh read is already saved). */
export type MenuSaveMode = "never" | "calibration" | "saved";

/** `?menusave=` in the page URL (FR QG6RNY); anything else is ignored. */
export function menuSaveFromUrl(search: string | undefined): MenuSaveMode | null {
  const raw = urlParam(search, "menusave");
  return raw === "never" || raw === "calibration" || raw === "saved" ? raw : null;
}

/** Least gap between two mock logo progress messages, ms: at most 20 a second. */
const MOCK_LOGO_PROGRESS_MS = 50;

/** Bytes per read-back read, as the backend reads the logo window. */
const MOCK_LOGO_READ_BYTES = 2048;

/** What the demo meter's logo area holds (FR MEHQC0). */
type MockLogoArea =
  | { status: "empty" }
  | { status: "unrecognised" }
  | { status: "logo"; width: number; height: number; mode: "opaque" | "alpha"; rgba: string };

/** Options of a {@link MockBackend}. */
export interface MockOptions {
  /** PD records to seed when the capture first starts. */
  pdSynthetic?: number | null;
  /** A scripted record-delete failure (FR JKWT3V). */
  recordsFault?: RecordsFault | null;
  /** A scripted settings-write failure (FR JKWT3W). */
  settingsFault?: SettingsFault | null;
  /** A scripted recording-file open (FR AXHK2S). */
  recordingFileFault?: RecordingFileFault | null;
  /** The history's full-resolution limit (FR AXHK2W); none (the default) never folds. */
  historyFullSamples?: number | null;
  /** The logo picture the open dialog chooses (FR MEHQC0). */
  logoFile?: LogoFile | null;
  /** A logo upload fails at this chunk (0-based) (FR MEHQC0); none (the default) never. */
  logoFault?: number | null;
  /** How the settings get saved after a logo write (FR QG6RNY); none: after
   * {@link MOCK_MENU_SAVE_MS}. */
  menuSave?: MenuSaveMode | null;
  /** The demo recording is handed over at launch (FR M078V5). */
  launchFile?: boolean;
}

/** Rows per `pd_rows` event of a seed, so one event never gets huge. */
const PD_SEED_BATCH = 10_000;

/** The meter's PD status block period while the sniffer runs, ms (FR TGE83D). */
const PD_STATUS_MS = 5;

/** What session `s`'s meter reads at session time `tMs`: its scenario, on the PD
 * contract while a capture runs, so the two tell the same story. */
function measuredAt(s: Running, tMs: number): Sample {
  const sample = sampleAt(s.scenario, tMs);
  if (s.pd.timer === null) return sample;
  const contract = pdContractAt(tMs - s.pd.sessionOffsetMs);
  sample.vbus_v = contract.vbus_v + 0.01 * Math.sin(tMs / 140);
  sample.ibus_a = contract.ibus_a;
  return sample;
}

/** The PD capture of a mock session (FR TGE83C). */
interface MockPdCapture {
  /** The PD sniffer runs. */
  timer: ReturnType<typeof setInterval> | null;
  /** Index of `records[0]`; it grows on a clear, so indices never restart. */
  base: number;
  /** Script record of every captured record, by record index minus `base`. */
  records: number[];
  /** The next script record. */
  next: number;
  /** Capture time reached, ms. */
  nowMs: number;
  /** The seed was played (once per session). */
  seeded: boolean;
  /** Session time (sample `t_ms`) minus capture time for the running capture run. */
  sessionOffsetMs: number;
  /** The row of every captured record, by record index minus `base`. */
  rows: PdRow[];
  /** Session time of the latest status sample sent, 0 if none. */
  statusMs: number;
}

/** A PD capture saved, or opened, in the mock: script records and rows from index 0. */
interface MockPdFile {
  path: string;
  records: number[];
  rows: PdRow[];
}

interface Running {
  /** The picker id the session was connected with; it keeps naming the session after a
   * replug moved the meter to another address, like the backend's. */
  id: string;
  /** The picker id the meter is listed under now. */
  deviceId: string;
  /** The serial the meter reports; a second demo meter gets one of its own (FR 1HHT03). */
  serial: string;
  /** The session id, as in the `connected` status. */
  sessionId: number;
  /** Where the measurements come from (FR 1HHT02). */
  source: MeasurementSource;
  scenario: Scenario;
  rateSps: number;
  /** The meter refused the asked rate and runs at its own. */
  rateLocked: boolean;
  /** Device time of the next sample, ms. */
  nextMs: number;
  /** Fractional samples owed to the next batch. */
  carry: number;
  timer: ReturnType<typeof setInterval>;
  onEvent: (event: BackendEvent) => void;
  /** Samples streamed so far (what an export writes). */
  streamed: number;
  /** The open recording, if any. */
  recording: MockRecording | null;
  /** Every sample since the start or the last clear (what the chart zooms into). */
  history: HistoryColumns;
  /** The gaps between the history's runs (`outage` only). */
  gaps: HistoryGap[];
  /** The outage gap the backend announces before the first samples after it. */
  pendingGap: Gap | null;
  stats: SessionStats;
  pd: MockPdCapture;
  /** The VBUS a trigger request moved the demo charger to, V; null at its own. */
  triggerVbusV: number | null;
  /** This meter's open trigger session (FR DGSQ6S), null when none (FR 1HHT03). */
  trigger: MockTrigger | null;
  /** `pd-start-refused`: a trigger session ran on this connection, so the meter refuses
   * a PD capture Start until a replug [device]. */
  pdStartRefused?: boolean;
  /** The settings were read on this connection, which a logo write needs (FR MEHQC0). */
  settingsRead?: boolean;
}

interface MockRecording {
  /** Which recording: no two share one, like the backend's. */
  id: number;
  path: string;
  rows: number;
  elapsedMs: number;
  sinceProgressMs: number;
  /** A pause holds the rows back. */
  paused: boolean;
  /** Where the pause gap the next rows close began. */
  pauseFromMs: number | null;
  gaps: Gap[];
  /** End once the current's magnitude stays below this, A. */
  belowA: number | null;
  /** How long it must stay below, ms. */
  forMs: number;
  /** Where the current low-current stretch began. */
  lowSinceMs: number | null;
  /** End at this many rows. */
  maxRows: number;
}

/** The auto-stop rule over the samples just recorded: whether the wait is over. */
function currentStayedLow(rec: MockRecording, samples: readonly Sample[]): boolean {
  if (rec.belowA === null) return false;
  for (const sample of samples) {
    if (Math.abs(sample.ibus_a) >= rec.belowA) {
      rec.lowSinceMs = null;
    } else {
      rec.lowSinceMs ??= sample.t_ms;
      if (sample.t_ms - rec.lowSinceMs >= rec.forMs) return true;
    }
  }
  return false;
}

/** Writes one tick's samples, like the backend's `Recorder`: a pause holds them back, and the
 * first row after one closes its gap at `nowMs`. Answers the rule that ended the recording. */
function writeRows(
  rec: MockRecording,
  samples: readonly Sample[],
  nowMs: number,
): RecordingEnd | null {
  if (rec.paused) return null;
  if (rec.pauseFromMs !== null) {
    rec.gaps.push({ kind: "pause", from_ms: rec.pauseFromMs, to_ms: nowMs });
    rec.pauseFromMs = null;
  }
  // Like the backend: the records after the row limit are not written.
  const written = samples.slice(0, rec.maxRows - rec.rows);
  rec.rows += written.length;
  if (currentStayedLow(rec, written)) return "low_current";
  return rec.rows >= rec.maxRows ? "row_limit" : null;
}

/** What a recording that ended for `ended` wrote. */
function summaryOf(rec: MockRecording, ended: RecordingEnd): RecordingSummary {
  return { rows: rec.rows, gaps: rec.gaps, disconnected: false, ended };
}

/** The open recording of `s`; refused when there is none. */
function recordingOf(s: Running): MockRecording {
  if (!s.recording) throw usage("Nothing is being recorded.");
  return s.recording;
}

/** Scripted backend behaviours chosen with `?mock=<name>` in the page URL; the device
 * list stays the same in every one but `replug` and `mode-flip`. */
export const MOCK_MODES = [
  "permission-denied",
  "rate-locked",
  "slow-export",
  "outage",
  "blip",
  "replug",
  "replug-late",
  "trigger-sag",
  "trigger-all",
  "trigger-out-of-sync",
  "trigger-req-error",
  "trigger-detect-disagree",
  "trigger-list-lag",
  "trigger-no-cable",
  "trigger-unlisted-cable",
  "trigger-charger-gone",
  "trigger-cc-gone",
  "pd-start-refused",
  "bootloader",
  "mode-flip",
  "firmware-fail-switching",
  "firmware-fail-erasing",
  "firmware-fail-writing",
  "firmware-fail-verifying",
  "firmware-fail-returning",
  "firmware-other-model",
  "firmware-bad-file",
  "firmware-running",
  "logo-other-model",
  "two-meters",
] as const;
export type MockMode = (typeof MOCK_MODES)[number];

/** `replug-late` (FR PGY704): after a mocked update the meter is listed in application
 * mode again only this long after the update ended, as the real backend sees it. */
const MOCK_LATE_RELIST_MS = 3_000;

/** The `?mock=` mode of the page URL, or null for the normal demo behaviour. */
export function mockModeFromUrl(search: string | undefined): MockMode | null {
  const name = urlParam(search, "mock");
  return MOCK_MODES.find((m) => m === name) ?? null;
}

/** `outage`: the meter has been up for a while (its clock is far from zero), streams
 * `beforeMs`, is gone for `gapMs` (unplugged, then connected again), and the live stream
 * goes on after that; the history holds both runs and reports the gap between them. */
export const MOCK_OUTAGE = { startMs: 30_400_000, beforeMs: 20_000, gapMs: 2_400_000 } as const;

/** `blip`: as `outage`, but the meter is gone for only `gapMs`, shorter than the
 * collapse threshold, so the gap keeps its true width on screen (AC-CWWKJ6.1). */
export const MOCK_BLIP = { startMs: 30_400_000, beforeMs: 20_000, gapMs: 10_000 } as const;

/** `replug`: one real meter (`serial`) is listed next to the demo entries. A session on
 * it streams `beforeMs`, then the meter is unplugged: the session ends with an unplug
 * error, the backend waits for the serial, and the list loses the meter. After `awayMs`
 * the OS lists it again at a new address and the backend reconnects on its own. */
export const MOCK_REPLUG = { serial: "MOCK-REPLUG-0001", beforeMs: 3_000, awayMs: 3_000 } as const;

/** `bootloader`: one real meter in bootloader mode, listed before the demo entries and
 * never connected (FR V9SK52, AC .5). */
const BOOTLOADER_METER: DeviceListing = {
  id: "usb:1:9",
  label: "KM003C — bus 1 address 9 (bootloader mode)",
  model: "KM003C",
  serial: null,
  bus: "1",
  address: 9,
  demo_scenario: null,
  mode: "bootloader",
};

/** `mode-flip`: one real meter (`serial`) listed in application mode at USB address
 * `address`; `appMs` after the list is first watched it turns to bootloader mode under
 * the same entry, and `bootMs` later it is back in application mode (AC-AXHK2M.17). */
export const MOCK_MODE_FLIP = {
  serial: "MOCK-FLIP-0001",
  address: 7,
  appMs: 3_000,
  bootMs: 3_000,
} as const;

/** `two-meters` (FR CCF47K, AC .2): two real KM003C meters listed before the demo
 * entries, `first` at USB address 5 and `second` at address 6; `second` holds the lower
 * serial, so with the serials hidden it reads "KM003C (1)". Each connects as a 5 V 1 A
 * load reporting its own serial. */
export const MOCK_TWO_METERS = { first: "MOCK-PAIR-0002", second: "MOCK-PAIR-0001" } as const;

/** The `two-meters` picker entries. */
function twoMeterListings(): DeviceListing[] {
  return [
    scriptedMeter(MOCK_TWO_METERS.first, 5, "application"),
    scriptedMeter(MOCK_TWO_METERS.second, 6, "application"),
  ];
}

/** The `two-meters` entry with `deviceId`, or null. */
function twoMeterListing(deviceId: string): DeviceListing | null {
  return twoMeterListings().find((d) => d.id === deviceId) ?? null;
}

/** The picker entry of a scripted real KM003C (`serial`) on bus 1 at `address` in `mode`,
 * labelled the way the backend lists it. */
function scriptedMeter(
  serial: string,
  address: number,
  mode: "application" | "bootloader",
): DeviceListing {
  const label = `KM003C — ${serial}`;
  return {
    id: `usb:1:${address}`,
    label: mode === "bootloader" ? `${label} (bootloader mode)` : label,
    model: "KM003C",
    serial,
    bus: "1",
    address,
    demo_scenario: null,
    mode,
  };
}

/** The mode-flip meter's picker entry in `mode` (AC-AXHK2M.17). */
function flipMeter(mode: "application" | "bootloader"): DeviceListing {
  return scriptedMeter(MOCK_MODE_FLIP.serial, MOCK_MODE_FLIP.address, mode);
}

/** The replug meter's picker entry at USB address `address`. */
function replugMeter(
  address: number,
  mode: "application" | "bootloader" = "application",
): DeviceListing {
  return scriptedMeter(MOCK_REPLUG.serial, address, mode);
}

/** The status event of a mock session that just connected; a null rate is a locked one. */
function connectedStatus(
  serial: string,
  rateSps: number | null,
  sessionId = 1,
  firmwareVersion = "2.0.6",
  model = "KM003C",
): BackendEvent {
  return {
    type: "status",
    data: {
      state: "connected",
      model,
      firmware_version: firmwareVersion,
      bootloader_version: "2.5",
      mode: "application",
      warnings: ["outdated"],
      update_required: false,
      firmware_problems: [],
      serial,
      rate_sps: rateSps,
      rate_locked: rateSps === null,
      session_id: sessionId,
      min_supported_firmware: "1.7.0",
      known_latest_firmware: "2.0.7",
    },
  };
}

/** How long a `slow-export` export stays half done, so the UI suite can see its progress. */
const SLOW_EXPORT_MS = 30_000;

/** The meter's own rate when authentication fails (`rate-locked`). */
const LOCKED_RATE_SPS = 50;

/** The Linux permission failure of `src-tauri/src/ui_error.rs` (the source of truth for the
 * wording; `mockBackend.test.ts` checks this copy against it, and the UI suite reads it). */
export const LINUX_PERMISSION_DENIED: Readonly<UiError> = {
  kind: "permission_denied",
  message: "The system did not allow the app to open the meter.",
  hint:
    "Install the udev rule: `sudo cp packaging/linux/60-powerz.rules /etc/udev/rules.d/`, then run " +
    "`sudo udevadm control --reload-rules` and `sudo udevadm trigger`, and replug the meter " +
    "(see `docs/linux.md`).",
};

/** The role swaps (DR, PR, VCONN, FR): control messages the library classifier does
 * not prove to stay at 5 V. */
const SWAP_CONTROL_TYPES: readonly number[] = [9, 10, 11, 19];

/** Vendor_Defined, the data message type of a VDM. */
const VENDOR_DEFINED = 15;

/** Like the library's check: a Vendor_Defined message to a cable plug whose first object
 * is a structured VDM header for a Discover Identity REQ, whatever its SVID, version or
 * other objects. */
function isCableDiscoverIdentity(m: PdMessageRequest): boolean {
  const vdm = m.data_objects[0];
  if (m.sop === "sop" || m.msg_type !== VENDOR_DEFINED || vdm === undefined) return false;
  const structured = (vdm >>> 15) & 1;
  const commandType = (vdm >>> 6) & 3;
  const command = vdm & 0x1f;
  return structured === 1 && commandType === 0 && command === 1;
}

/** Like the library's builder: a control message on SOP is `pd cmd=<type>`, anything
 * else `pd data=<SOP byte><header LE><objects LE>` in uppercase hex; GoodCRC, reserved
 * control types and malformed messages are a usage error. */
function renderMockMessage(m: PdMessageRequest): string {
  const { msg_type: ty, data_objects: objects } = m;
  if (!Number.isInteger(ty) || ty < 0 || ty > 31) {
    throw usage(`Message type ${ty} does not fit the 5-bit field.`);
  }
  const refusal = messageRefusal(m);
  if (refusal !== null) throw usage(refusal);
  if (objects.length === 0) {
    if (ty === 0 || ty > 0x18) throw usage(`Control message type ${ty} is reserved in USB PD.`);
    if (ty === 2) throw usage("A data message needs at least one data object.");
  }
  if (objects.length > 7) {
    throw usage(`${objects.length} data objects: a PD message carries at most 7.`);
  }
  if (objects.length === 0 && m.sop === "sop") return `pd cmd=${ty}`;
  const sop = m.sop === "sop" ? 0 : m.sop === "sop_prime" ? 1 : 2;
  const header = (objects.length << 12) | 0x80 | ty;
  const bytes = [sop, header & 0xff, header >>> 8];
  for (const o of objects) bytes.push(o & 0xff, (o >>> 8) & 0xff, (o >>> 16) & 0xff, o >>> 24);
  const hex = bytes.map((b) => b.toString(16).toUpperCase().padStart(2, "0")).join("");
  return `pd data=${hex}`;
}

/** Like the library classifier for a raw console line: queries are read-only, session
 * control and entries stay at 5 V, `pd req=` goes by the offer, unknown text is above 5 V. */
function mockLineRisk(text: string): RiskClass {
  if (["pdm parm", "pd pdo", "ufcs pdo"].includes(text)) return "read_only";
  if (/^(pdm open|pdm close|reset|pd hrst|entry \S+)$/.test(text)) return "at_most_5v";
  const cmd = /^pd cmd=(\d+)$/.exec(text);
  if (cmd) {
    const ty = Number(cmd[1]);
    return ty >= 1 && ty <= 24 && !SWAP_CONTROL_TYPES.includes(ty) ? "at_most_5v" : "above_5v";
  }
  const req = /^pd req=(\d+)$/.exec(text);
  if (req) {
    const offer = MOCK_TRIGGER_OFFERS.find((o) => o.position === Number(req[1]));
    return offer && offer.max_voltage_mv <= 5000 ? "at_most_5v" : "above_5v";
  }
  return "above_5v";
}

/** The scripted charger's `pdo:` block in words, like the backend's description. */
function mockPdoBlock(): string {
  const v = (mv: number): string => (mv / 1000).toFixed(2);
  const offers = MOCK_TRIGGER_OFFERS.map((o) => {
    const volts =
      o.min_voltage_mv === o.max_voltage_mv
        ? `${v(o.max_voltage_mv)} V`
        : `${v(o.min_voltage_mv)}-${v(o.max_voltage_mv)} V`;
    const current = o.max_current_ma === null ? "" : ` ${v(o.max_current_ma)} A`;
    const power = o.max_power_mw === null ? "" : ` ${(o.max_power_mw / 1000).toFixed(1)} W`;
    return `#${o.position} ${o.kind} ${volts}${current}${power}`;
  });
  return `pdo: ${offers.join(", ")}`;
}

/** The mock trigger session: what was entered last (how to get back to 5 V). */
interface MockTrigger {
  entered: "pd" | "legacy" | null;
  /** The queue rate before the session capped it at 50 SPS, if it did. */
  rateBefore: number | null;
  /** "Don't ask again" is on (FR AXHK2N). */
  armed: boolean;
  /** `trigger-charger-gone`: the charger's going away has been scheduled. */
  goingAway: boolean;
  /** VBUS collapsed under a contract (FR AXHK2N), or both CC lines fell after Detect
   * (`trigger-cc-gone`, FR WFSNZD). */
  chargerGone: boolean;
  /** `trigger-cc-gone`: both CC lines read 0 V until the next PD listing. */
  ccLow?: boolean;
  /** The command log's length right after a way back to 5 V; while it is unchanged
   * nothing went out since, so the close skips another (AC-AXHK2P.15). */
  backAt?: number;
}

/** The queue rate cap while a trigger session is open, SPS. [inferred] */
const TRIGGER_MAX_SPS = 50;
/** `trigger-charger-gone`: how long after the first accepted request VBUS falls, ms. */
const CHARGER_GONE_AFTER_MS = 1000;
/** `trigger-cc-gone`: how long after Detect both CC lines fall, ms. */
const CC_GONE_AFTER_MS = 1000;
/** The refusal of what names an offer once the charger went away. */
const CHARGER_GONE_REFUSAL = "The charger went away. Press Detect.";

/** The backend's `OutOfSync` refusal: the meter's replies lag behind the commands. */
function outOfSyncError(): UiError {
  return {
    kind: "out_of_sync",
    message: "The meter's replies are out of step with the commands, so no more requests are sent.",
    hint: "Power-cycle the meter (unplug it and plug it back in), then open the session again.",
  };
}

/** `trigger-all`: the decoded limits of the charger that speaks every protocol. [inferred] */
const MOCK_SCP_LIMITS: TriggerLimits = {
  index: null,
  min_voltage_mv: 3400,
  max_voltage_mv: 11000,
  min_current_ma: 500,
  max_current_ma: 5000,
  max_power_mw: null,
};
const MOCK_ALL_LIMITS: Partial<Record<TriggerProtocol, readonly TriggerLimits[]>> = {
  scp: [MOCK_SCP_LIMITS],
  vfcp: [{ ...MOCK_SCP_LIMITS, max_current_ma: 4000 }],
  tfcp: [{ ...MOCK_SCP_LIMITS }],
  ufcs: [
    { ...MOCK_SCP_LIMITS, index: 1, max_power_mw: 40000 },
    {
      ...MOCK_SCP_LIMITS,
      index: 2,
      max_voltage_mv: 21000,
      max_current_ma: 3000,
      max_power_mw: 63000,
    },
  ],
};

/** The fixed voltages each protocol allows, V. */
const MOCK_FIXED_VOLTS: Record<"qc" | "fcp" | "afc" | "sfcp", readonly number[]> = {
  qc: [5, 9, 12, 20],
  fcp: [5, 9, 12],
  afc: [5, 9, 12],
  sfcp: [5, 9, 12],
};

/** The scripted charger's PD offers: 5/9/15/20 V fixed, PPS 3.3-21 V, EPR 28 V and AVS. */
const MOCK_TRIGGER_OFFERS: readonly TriggerOffer[] = [
  {
    position: 1,
    kind: "fixed",
    min_voltage_mv: 5000,
    max_voltage_mv: 5000,
    max_current_ma: 3000,
    max_power_mw: null,
    epr: false,
  },
  {
    position: 2,
    kind: "fixed",
    min_voltage_mv: 9000,
    max_voltage_mv: 9000,
    max_current_ma: 3000,
    max_power_mw: null,
    epr: false,
  },
  {
    position: 3,
    kind: "fixed",
    min_voltage_mv: 15000,
    max_voltage_mv: 15000,
    max_current_ma: 3000,
    max_power_mw: null,
    epr: false,
  },
  {
    position: 4,
    kind: "fixed",
    min_voltage_mv: 20000,
    max_voltage_mv: 20000,
    max_current_ma: 5000,
    max_power_mw: null,
    epr: false,
  },
  {
    position: 5,
    kind: "pps",
    min_voltage_mv: 3300,
    max_voltage_mv: 21000,
    max_current_ma: 3250,
    max_power_mw: null,
    epr: false,
  },
  {
    position: 8,
    kind: "fixed",
    min_voltage_mv: 28000,
    max_voltage_mv: 28000,
    max_current_ma: 5000,
    max_power_mw: null,
    epr: true,
  },
  {
    position: 9,
    kind: "epr-avs",
    min_voltage_mv: 15000,
    max_voltage_mv: 28000,
    max_current_ma: null,
    max_power_mw: 140000,
    epr: true,
  },
];

/** One page-lifetime instance; a running session's timer stops on `disconnect()` or
 * when the page goes away (it is a test and demo double, not a resource owner). */
/** The demo e-marked cable's details: VID 0x050D, PID 0x8254, passive, 50 V, 5 A,
 * EPR capable. */
const MOCK_CABLE: CableView = {
  vid: 0x050d,
  pid: 0x8254,
  active: false,
  max_voltage_v: 50,
  max_current_ma: 5000,
  epr_capable: true,
  speed: "USB 2.0",
  vendor: "Belkin",
};

/** The one message the backend sends a cable with no dialog. */
const CABLE_READ_LINE = "pd data=018F1001A800FF";

/** The demo meter's settings rows, as the backend decodes the captured FW 2.0.6
 * block (FR 1HHT01): the screen-checked fields confirmed, rotation and the
 * calibration values inferred. */
const DEMO_METER_SETTINGS: readonly MeterSettingField[] = [
  { key: "language", label: "Language", value: "English", tag: "confirmed" },
  { key: "brightness", label: "Brightness", value: "50%", tag: "confirmed" },
  { key: "dim_timeout", label: "Dim screen after", value: "off", tag: "confirmed" },
  { key: "startup_logo", label: "Startup logo", value: "off", tag: "confirmed" },
  { key: "theme", label: "Theme", value: "blue", tag: "confirmed" },
  { key: "auto_rotate", label: "Auto-rotate", value: "on", tag: "confirmed" },
  { key: "rotation", label: "Rotation", value: "90°", tag: "inferred" },
  { key: "sample_rate", label: "Sample rate", value: "50 SPS", tag: "confirmed" },
  {
    key: "voltage_cal",
    label: "Voltage calibration (raw)",
    value: "606, 606, 606, 606, 606",
    tag: "inferred",
  },
  {
    key: "current_cal",
    label: "Current calibration (raw)",
    value: "996668, 996668, 996668, 996668, 996668",
    tag: "inferred",
  },
  { key: "offset_cal", label: "Offset calibration (raw)", value: "80", tag: "inferred" },
];

/** How the demo meter shows each writable setting's codes (FR JKWT3W): a list
 * indexed by code, or `null` for brightness (1..100, shown as a percentage). Kept
 * here rather than imported from the panel so the mock stays free of UI modules. */
const DEMO_WRITABLE_SETTINGS: Readonly<Record<string, readonly string[] | null>> = {
  language: ["Chinese", "English"],
  brightness: null,
  dim_timeout: ["off", "15 s", "30 s", "60 s"],
  theme: ["blue", "green", "pink", "orange", "yellow", "dark green", "cyan"],
  rotation: ["0°", "90°", "180°", "270°"],
  sample_rate: ["2 SPS", "10 SPS", "50 SPS", "1000 SPS"],
  startup_duration: ["off", "1 s", "3 s", "5 s"],
};

/** The settings row a writable key changes, where it is not the key itself (FR MEHQC0). */
const DEMO_SETTING_ROW: Readonly<Record<string, string>> = { startup_duration: "startup_logo" };

/** How the demo meter shows `code` for writable `key`, or null when it is out of range. */
function demoSettingText(key: string, code: number): string | null {
  if (!Object.hasOwn(DEMO_WRITABLE_SETTINGS, key) || !Number.isInteger(code)) return null;
  const labels = DEMO_WRITABLE_SETTINGS[key];
  if (labels) return labels[code] ?? null;
  return code >= 1 && code <= 100 ? `${code}%` : null;
}

/** The demo meter's offline records (FR 1HHT00, AC .6): the simulator's three load
 * profiles, each sample's charge and energy summed like the meter does; PHONE_5V
 * stores negative totals like the real meter in one current direction. */
const DEMO_RECORD_SPECS = [
  { name: "PHONE_5V", interval_s: 1, points: 300, offset: 0x0000, vbus: 5.1, ibus: -1.8 },
  { name: "PD_9V_TEST", interval_s: 2, points: 120, offset: 0x2000, vbus: 9.0, ibus: 2.0 },
  { name: "LAPTOP_20V", interval_s: 5, points: 60, offset: 0x3000, vbus: 20.0, ibus: 3.0 },
] as const;

/** Every sample of demo record `spec`. */
function demoRecordSamples(spec: (typeof DEMO_RECORD_SPECS)[number]): OfflineRecordSample[] {
  const dtH = spec.interval_s / 3600;
  let charge = 0;
  let energy = 0;
  return Array.from({ length: spec.points }, (_, k) => {
    const t = k * spec.interval_s;
    const v = spec.vbus + 0.02 * Math.sin(t * 0.11);
    const a = spec.ibus * (1 - 0.3 * Math.min(t / 600, 1)) + 0.01 * Math.sin(t * 0.37);
    if (k > 0) {
      charge += a * dtH;
      energy += v * a * dtH;
    }
    return { t_s: t, vbus_v: v, ibus_a: a, charge_ah: charge, energy_wh: energy };
  });
}

/** One demo record: its list entry and its samples, which agree. */
interface DemoRecord {
  entry: OfflineRecordEntry;
  samples: OfflineRecordSample[];
}

/** The demo records, downloaded form: entries and samples agree. */
const DEMO_RECORDS: readonly DemoRecord[] = DEMO_RECORD_SPECS.map((spec, i) => {
  const samples = demoRecordSamples(spec);
  const last = samples[samples.length - 1];
  return {
    entry: {
      index: i + 1,
      name: spec.name,
      interval_s: spec.interval_s,
      points: spec.points,
      duration_s: spec.interval_s * Math.max(spec.points - 1, 0),
      charge_ah: last?.charge_ah ?? 0,
      energy_wh: last?.energy_wh ?? 0,
      offset: spec.offset,
    },
    samples,
  };
});

/** The commands of one meter's tile (FR 1HHT03), as {@link MockBackend.meter} hands them out. */
export type MockMeter = MeterCommands;

/** The demo meters' serial; a second demo meter counts up from it (FR 1HHT03). */
const DEMO_SERIAL = "123456";

export class MockBackend {
  /** The session the commands without a device id act on: the meter connected last. */
  #session: Running | null = null;
  /** Every running session, in the order the meters were connected (FR 1HHT03). */
  #sessions: Running[] = [];
  #nextSessionId = 1;
  #lastRecordingId = 0;
  /** The PD capture saved last in the page, and the one opened (FR TGE83E). */
  #savedPd: MockPdFile | null = null;
  #openedPd: MockPdFile | null = null;
  #mode: MockMode | null;
  /** The updates run so far (FR V9SK55). */
  #firmwareFlashes: { deviceId: string; path: string; confirmation: string }[] = [];
  /** The demo meter's firmware: the flashed file's version after a done update
   * (AC-AXHK2M.16). */
  #firmwareVersion = "2.0.6";
  /** The UI suite's frame-rate tests hold the stream to measure the same page idle:
   * no batches and no samples while held, the sessions untouched (FR 9QTPNQ). */
  #held = false;
  /** Batches sent so far, all sessions (FR 9QTPNQ). */
  #batchesSent = 0;
  /** A firmware update holds the meter (FR V9SK55, AC .4). */
  #flashing = false;
  /** The meter the running update holds; another one can connect (AC-4PJRVM.4). */
  #flashTarget: string | null = null;
  /** The running update was asked to stop / has begun its erase (AC-4PJRVJ.1). */
  #flashStop = false;
  #flashErasing = false;
  #options: MockOptions;
  /** The open recording file, which no meter takes part in (FR AXHK2S). */
  #recordingFile: HistoryColumns | null = null;
  /** Ends a `slow` open that is under way. */
  #endRecordingOpen: (() => void) | null = null;
  #revealed: string[] = [];
  /** The chart images saved (FR AXHK2V). */
  #chartImages: { suggestedName: string; format: string; data: number[] }[] = [];
  /** The head of each export's rows as CSV text, oldest first. */
  #exports: string[] = [];
  /** `replug`: the meter's USB address while the OS lists it, null while unplugged. */
  #meterAddress: number | null = 7;
  /** `replug`: the session whose meter is away, which the backend waits for. */
  #waiting: Running | null = null;
  /** `replug`: the meter is unplugged only once per page. */
  #unplugDone = false;
  #replugTimer: ReturnType<typeof setTimeout> | null = null;
  /** `replug`: the session the unplug script runs for. */
  #replugFor: Running | null = null;
  #watchers = new Set<(devices: DeviceListing[]) => void>();
  /** `mode-flip`: the listed meter's mode, and whether its flips are scheduled. */
  #flipMode: "application" | "bootloader" = "application";
  /** `replug`: the listed meter's mode, bootloader while a mocked update has it switched. */
  #replugMode: "application" | "bootloader" = "application";
  #flipStarted = false;
  /** Settings reads so far (FR 1HHT01). */
  #meterSettingsReads = 0;
  /** Settings writes sent so far (FR JKWT3W). */
  #meterSettingsWrites = 0;
  /** The demo meter's settings rows, which a write changes (FR JKWT3W). */
  #meterSettings: MeterSettingField[] = DEMO_METER_SETTINGS.map((f) => ({ ...f }));
  /** Offline-record downloads so far (FR 1HHT00). */
  #recordDownloads = 0;
  /** The demo meter's logo area, which upload and Remove change (FR MEHQC0). */
  #logoArea: MockLogoArea = { status: "empty" };
  /** When the demo meter saves its settings after the last logo write (`Date.now()`
   * ms), null when no write waits for a menu save (FR QG6RNY). */
  #menuSaveAt: number | null = null;
  /** The demo meter's record table, which a delete changes (FR JKWT3V). */
  #records: DemoRecord[] = [...DEMO_RECORDS];
  /** Record backups written so far, for their folder names (FR JKWT3V). */
  #recordBackups = 0;
  /** The trigger session (FR DGSQ6S) of the meter the commands act on, null when none;
   * each meter has its own (FR 1HHT03). */
  get #trigger(): MockTrigger | null {
    return this.#session?.trigger ?? null;
  }
  /** Every command the scripted charger received. */
  #triggerLog: string[] = [];
  /** The launch file not yet taken (FR M078V5). */
  #launchFile = false;

  /** `mode` defaults to the page URL's `?mock=` parameter, the PD seed to its
   * `?pdsynthetic=`. */
  constructor(
    mode: MockMode | null = mockModeFromUrl(pageSearch()),
    options: MockOptions = {
      pdSynthetic: pdSyntheticFromUrl(pageSearch()),
      recordsFault: recordsFaultFromUrl(pageSearch()),
      settingsFault: settingsFaultFromUrl(pageSearch()),
      recordingFileFault: recordingFileFaultFromUrl(pageSearch()),
      historyFullSamples: foldAtFromUrl(pageSearch()),
      logoFile: logoFileFromUrl(pageSearch()),
      logoFault: logoFaultFromUrl(pageSearch()),
      menuSave: menuSaveFromUrl(pageSearch()),
      launchFile: launchFileFromUrl(pageSearch()),
    },
  ) {
    this.#mode = mode;
    this.#options = options;
    this.#launchFile = options.launchFile ?? false;
    this.#flashing = mode === "firmware-running";
    this.#flashTarget = this.#flashing ? MOCK_RUNNING_FLASH.device_id : null;
  }

  /** `replug`: the real meter's listing while the OS lists it; `bootloader`: the
   * bootloader-mode meter; else null. */
  #listedMeter(): DeviceListing | null {
    if (this.#mode === "bootloader") return BOOTLOADER_METER;
    if (this.#mode === "mode-flip") return flipMeter(this.#flipMode);
    return this.#replugLike() && this.#meterAddress !== null
      ? replugMeter(this.#meterAddress, this.#replugMode)
      : null;
  }

  /** `replug` and `replug-late` list the same real meter; only the late relist differs. */
  #replugLike(): boolean {
    return this.#mode === "replug" || this.#mode === "replug-late";
  }

  async listDevices(): Promise<DeviceListing[]> {
    const meter = this.#listedMeter();
    const meters = this.#mode === "two-meters" ? twoMeterListings() : meter ? [meter] : [];
    const demos = SCENARIOS.map((name) => ({
      id: `demo:${name}`,
      label: `Demo device — ${name}`,
      model: "KM003C",
      serial: DEMO_SERIAL,
      bus: null,
      address: null,
      demo_scenario: name,
      mode: "application" as const,
    }));
    return [...meters, ...demos];
  }

  /** The full list now and on every change (only `replug` and `mode-flip` change it). */
  async watchDevices(onChange: (devices: DeviceListing[]) => void): Promise<() => void> {
    this.#watchers.add(onChange);
    onChange(await this.listDevices());
    if (this.#mode === "mode-flip" && !this.#flipStarted) this.#startFlips();
    return () => this.#watchers.delete(onChange);
  }

  /** `mode-flip`: into the bootloader after `appMs`, back after `bootMs` more. */
  #startFlips(): void {
    this.#flipStarted = true;
    setTimeout(() => {
      this.#flipMode = "bootloader";
      void this.#notifyDevices();
      setTimeout(() => {
        this.#flipMode = "application";
        void this.#notifyDevices();
      }, MOCK_MODE_FLIP.bootMs);
    }, MOCK_MODE_FLIP.appMs);
  }

  async #notifyDevices(): Promise<void> {
    const list = await this.listDevices();
    for (const w of this.#watchers) w(list);
  }

  /** The device ids of the running sessions, in the order they were connected (FR 1HHT03). */
  connectedIds(): string[] {
    return this.#sessions.map((s) => s.deviceId);
  }

  /** The demo serial, or the next one up when a running demo meter already reports it. */
  #freeDemoSerial(): string {
    let n = Number(DEMO_SERIAL);
    while (this.#sessions.some((s) => s.serial === String(n))) n += 1;
    return String(n);
  }

  /** The commands of meter `deviceId` (FR 1HHT03): each acts on that meter's session,
   * whichever meter the commands without a device id act on. */
  meter(deviceId: string): MockMeter {
    const on =
      <A extends unknown[], R>(f: (...args: A) => Promise<R>) =>
      (...args: A): Promise<R> => {
        const s = this.#sessions.find((x) => x.id === deviceId || x.deviceId === deviceId);
        if (!s) return Promise.reject(usage("No meter is connected."));
        const before = this.#session;
        // The command reads its session before its first await.
        this.#session = s;
        try {
          return f.apply(this, args);
        } finally {
          this.#session =
            before && this.#sessions.includes(before) ? before : (this.#sessions.at(-1) ?? null);
        }
      };
    return {
      disconnect: on(this.disconnect),
      // Like the backend: ends this meter's wait only, also while its session is down.
      cancelWait: async () => {
        if (this.#waiting?.id === deviceId || this.#waiting?.deviceId === deviceId) {
          this.#waiting = null;
        }
      },
      setRate: on(this.setRate),
      startRecording: on(this.startRecording),
      stopRecording: on(this.stopRecording),
      pauseRecording: on(this.pauseRecording),
      resumeRecording: on(this.resumeRecording),
      saveRecordingAs: on(this.saveRecordingAs),
      exportSession: on(this.exportSession),
      exportSessionDb: on(this.exportSessionDb),
      stats: on(this.stats),
      historyRange: on(this.historyRange) as MockBackend["historyRange"],
      historyGaps: on(this.historyGaps),
      historyFoldBoundary: on(this.historyFoldBoundary),
      rangeStats: on(this.rangeStats),
      runProgress: on(this.runProgress),
      resetStats: on(this.resetStats),
      clearHistory: on(this.clearHistory),
      pdStart: on(this.pdStart),
      pdStop: on(this.pdStop),
      pdDetails: on(this.pdDetails),
      pdClear: on(this.pdClear),
      pdSave: on(this.pdSave),
      meterSettings: on(this.meterSettings),
      meterSettingsWrite: on(this.meterSettingsWrite),
      recordsList: on(this.recordsList),
      recordDownload: on(this.recordDownload),
      recordsDelete: on(this.recordsDelete),
      triggerOpen: on(this.triggerOpen),
      triggerDetect: on(this.triggerDetect),
      triggerList: on(this.triggerList),
      triggerRisk: on(this.triggerRisk),
      triggerRequest: on(this.triggerRequest),
      triggerArmed: on(this.triggerArmed),
      triggerDisarm: on(this.triggerDisarm),
      triggerChargerGone: on(this.triggerChargerGone),
      triggerPdEntered: on(this.triggerPdEntered),
      triggerBackTo5v: on(this.triggerBackTo5v),
      triggerClose: on(this.triggerClose),
      triggerMessageRisk: on(this.triggerMessageRisk),
      triggerMessageSend: on(this.triggerMessageSend),
      triggerCableRead: on(this.triggerCableRead),
      pdCableSeen: on(this.pdCableSeen),
      triggerConsoleRisk: on(this.triggerConsoleRisk),
      triggerConsoleSend: on(this.triggerConsoleSend),
    };
  }

  /** The running update holds meter `id`, or may: like the backend, a real meter is not
   * told apart from a real target, which can come back at a new address. */
  #heldByFlash(id: string): boolean {
    const target = this.#flashTarget;
    if (!this.#flashing || target === null) return false;
    return id === target || (!id.startsWith("demo:") && !target.startsWith("demo:"));
  }

  /** The source the running session streams from, `null` when none runs (FR 1HHT02). */
  connectedSource(): MeasurementSource | null {
    return this.#session?.source ?? null;
  }

  async connect(
    deviceId: string,
    rateSps: number,
    onEvent: (event: BackendEvent) => void,
    source: MeasurementSource = "queue",
  ): Promise<void> {
    if (this.#heldByFlash(deviceId)) throw usage(FLASH_REFUSAL);
    if (this.#sessions.some((x) => x.id === deviceId || x.deviceId === deviceId)) {
      throw usage("This meter is already connected. Disconnect it first.");
    }
    const pair = this.#mode === "two-meters" ? twoMeterListing(deviceId) : null;
    const isMeter = pair !== null || this.#listedMeter()?.id === deviceId;
    // Like the backend: a connect by hand ends the wait for the unplugged meter, unless
    // it adds a meter beside running ones (FR 1HHT03); a connect of that meter itself
    // always does.
    const waited = this.#waiting;
    if (
      this.#sessions.length === 0 ||
      (waited !== null && (waited.id === deviceId || waited.deviceId === deviceId || isMeter))
    ) {
      this.#waiting = null;
    }
    const scenario = isMeter ? "load-5v-1a" : scenarioOf(deviceId);
    if (!scenario) throw usage("That meter is no longer attached.");
    if (this.#mode === "permission-denied") throw { ...LINUX_PERMISSION_DENIED };
    const locked = this.#mode === "rate-locked";
    const serial = pair?.serial ?? (isMeter ? MOCK_REPLUG.serial : this.#freeDemoSerial());
    const sessionId = this.#nextSessionId++;
    // Like the worker: a locked session's rate is unknown until measured.
    onEvent(
      connectedStatus(
        serial,
        locked ? null : rateSps,
        sessionId,
        this.#firmwareVersion,
        this.#mode === "logo-other-model" ? "KM002C" : "KM003C",
      ),
    );
    if (locked) {
      onEvent({
        type: "status",
        data: { state: "rate", rate_sps: LOCKED_RATE_SPS, rate_locked: true },
      });
    }
    const session: Running = {
      id: deviceId,
      deviceId,
      serial,
      sessionId,
      source,
      scenario,
      rateSps: locked ? LOCKED_RATE_SPS : rateSps,
      rateLocked: locked,
      nextMs: 0,
      carry: 0,
      onEvent,
      streamed: 0,
      recording: null,
      history: new HistoryColumns(),
      gaps: [],
      pendingGap: null,
      triggerVbusV: null,
      trigger: null,
      stats: new SessionStats(),
      pd: {
        timer: null,
        base: 0,
        records: [],
        next: 0,
        nowMs: 0,
        seeded: false,
        sessionOffsetMs: 0,
        rows: [],
        statusMs: 0,
      },
      timer: setInterval(() => this.#tick(session), BATCH_MS),
    };
    this.#sessions.push(session);
    this.#session = session;
    if (this.#mode === "outage") this.#seedOutage(session, MOCK_OUTAGE);
    if (this.#mode === "blip") this.#seedOutage(session, MOCK_BLIP);
    if (isMeter && !this.#unplugDone) {
      this.#unplugDone = true;
      this.#replugFor = session;
      this.#replugTimer = setTimeout(() => this.#unplug(session), MOCK_REPLUG.beforeMs);
    }
    // The mock production build (the Playwright suite) honours it too.
    const synthetic = syntheticFromUrl(
      pageSearch(),
      import.meta.env.DEV || import.meta.env.VITE_BACKEND === "mock",
    );
    if (synthetic !== null) await this.seedSyntheticHistory(synthetic);
  }

  /** `replug`: the cable is pulled. Like the backend: an open recording ends, the
   * session ends with an unplug error, the backend says it waits for the serial, and the
   * list loses the meter; after `awayMs` the meter is back and the session goes on. */
  #unplug(s: Running): void {
    this.#replugTimer = null;
    if (!this.#sessions.includes(s)) return;
    this.#endSession(s, {
      kind: "disconnected",
      message: "The meter was unplugged.",
      hint: null,
    });
    this.#waiting = s;
    s.onEvent({ type: "status", data: { state: "waiting", serial: MOCK_REPLUG.serial } });
    this.#meterAddress = null;
    void this.#notifyDevices();
    this.#replugTimer = setTimeout(() => this.#replug(s), MOCK_REPLUG.awayMs);
  }

  /** `replug`: the OS lists the meter again at a new address; a session still waiting
   * for it reconnects on the same timeline, with an outage gap over the time away. */
  #replug(s: Running): void {
    this.#replugTimer = null;
    this.#meterAddress = 9;
    void this.#notifyDevices();
    if (this.#waiting !== s) return;
    this.#waiting = null;
    const step = 1000 / s.rateSps;
    const from = Math.round(s.nextMs - step);
    s.nextMs += MOCK_REPLUG.awayMs;
    s.carry = 0;
    s.deviceId = replugMeter(9).id;
    const gap = { from_ms: from, to_ms: Math.round(s.nextMs) };
    s.gaps.push(gap);
    s.pendingGap = { kind: "outage", ...gap };
    s.onEvent(connectedStatus(MOCK_REPLUG.serial, s.rateSps, 1, this.#firmwareVersion));
    s.timer = setInterval(() => this.#tick(s), BATCH_MS);
    this.#sessions.push(s);
    // Like the backend: the commands without a device id stay on a meter still running.
    this.#session ??= s;
  }

  /** `outage` / `blip`: the run before the outage, then the live stream after it. */
  #seedOutage(s: Running, outage: typeof MOCK_OUTAGE | typeof MOCK_BLIP): void {
    const step = 1000 / s.rateSps;
    const end = outage.startMs + outage.beforeMs;
    let last: number = outage.startMs;
    for (let t = outage.startMs; t < end; t += step) {
      last = Math.round(t);
      s.history.push(sampleAt(s.scenario, last));
    }
    s.nextMs = end + outage.gapMs;
    const gap = { from_ms: last, to_ms: s.nextMs };
    s.gaps.push(gap);
    s.pendingGap = { kind: "outage", ...gap };
  }

  #tick(s: Running): void {
    if (this.#held) return;
    s.carry += (s.rateSps * BATCH_MS) / 1000;
    const n = Math.floor(s.carry);
    s.carry -= n;
    const step = 1000 / s.rateSps;
    const samples: Sample[] = [];
    for (let i = 0; i < n; i++) {
      const sample = measuredAt(s, Math.round(s.nextMs));
      if (s.triggerVbusV !== null) {
        sample.vbus_v = s.triggerVbusV + 0.01 * Math.sin(s.nextMs / 140);
      }
      if (s.trigger?.ccLow) {
        sample.cc1_v = 0;
        sample.cc2_v = 0;
      }
      samples.push(sample);
      s.history.push(sample);
      s.stats.push(sample);
      s.nextMs += step;
    }
    if (s.pendingGap !== null && n > 0) {
      s.onEvent({ type: "gap", data: s.pendingGap });
      s.pendingGap = null;
      if (s.recording) s.recording.lowSinceMs = null;
    }
    // Like the backend: the CDC stream carries no temperature.
    const tempC = s.source === "cdc" ? null : 30.5 + 0.2 * Math.sin(s.nextMs / 60_000);
    s.onEvent({ type: "batch", data: { samples, temp_c: tempC, stats: s.stats.snapshot() } });
    this.#batchesSent += 1;
    s.streamed += n;
    const rec = s.recording;
    if (rec) {
      const ended = writeRows(rec, samples, s.nextMs);
      if (ended !== null) {
        this.#endRecording(s, ended);
        return;
      }
      rec.elapsedMs += BATCH_MS;
      rec.sinceProgressMs += BATCH_MS;
      if (rec.sinceProgressMs >= PROGRESS_MS) {
        rec.sinceProgressMs = 0;
        s.onEvent({
          type: "recording",
          data: {
            state: "progress",
            rows: rec.rows,
            elapsed_ms: rec.elapsedMs,
            paused: rec.paused,
          },
        });
      }
    }
  }

  /** A new page asks for the running session (the current one, or meter `deviceId`'s);
   * `null` when none is running. Later events go to `onEvent` only. */
  async attach(
    onEvent: (event: BackendEvent) => void,
    deviceId?: string,
  ): Promise<AttachedSession | null> {
    const s =
      deviceId === undefined
        ? this.#session
        : (this.#sessions.find((x) => x.id === deviceId || x.deviceId === deviceId) ?? null);
    if (!s) return null;
    s.onEvent = onEvent;
    const rec = s.recording;
    return {
      device_id: s.deviceId,
      model: "KM003C",
      firmware_version: this.#firmwareVersion,
      serial: s.serial,
      rate_sps: s.rateSps,
      rate_locked: s.rateLocked,
      session_id: s.sessionId,
      recording: rec
        ? { rows: rec.rows, elapsed_ms: rec.elapsedMs, path: rec.path, paused: rec.paused }
        : null,
      pd_capturing: s.pd.timer !== null,
    };
  }

  /** The open session; refuses like the backend when none is. */
  #open(): Running {
    const s = this.#session;
    if (!s) throw usage("No meter is connected.");
    return s;
  }

  /** Stops waiting for the unplugged meter: it is listed again later, not reconnected. */
  async cancelWait(): Promise<void> {
    this.#waiting = null;
  }

  async disconnect(): Promise<void> {
    const s = this.#open();
    // Like the backend: back to 5 V and `pdm close` before the queue stops.
    this.#closeTrigger();
    // Only the meter whose unplug is scripted cancels the script (FR 1HHT03).
    if (this.#replugTimer !== null && this.#replugFor === s) {
      clearTimeout(this.#replugTimer);
      this.#replugTimer = null;
    }
    this.#endSession(s, null);
  }

  /** Stops `s`'s stream, ends an open recording, and reports the session over with
   * `error` (null when it was asked for). */
  #endSession(s: Running, error: UiError | null): void {
    clearInterval(s.timer);
    // Like the worker: the sniffer stops before the session ends.
    this.#stopPd(s);
    this.#sessions = this.#sessions.filter((x) => x !== s);
    if (this.#session === s) this.#session = this.#sessions.at(-1) ?? null;
    if (s.recording) {
      const rows = s.recording.rows;
      s.recording = null;
      s.onEvent({
        type: "recording",
        data: { state: "stopped", rows, gaps: [], disconnected: true, ended: "disconnected" },
      });
    }
    s.onEvent({ type: "status", data: { state: "disconnected", error } });
  }

  async setRate(rateSps: number): Promise<void> {
    const s = this.#open();
    if (this.#mode === "rate-locked") {
      throw usage("Changing the rate needs authentication, which the meter refused.");
    }
    // One file, one rate: the real backend refuses a change mid-recording.
    if (s.recording) throw usage("Stop the recording to change the rate.");
    s.rateSps = rateSps;
    s.carry = 0;
    s.onEvent({ type: "status", data: { state: "rate", rate_sps: rateSps, rate_locked: false } });
  }

  /** A rule ended the recording: the page hears it, as from the backend. */
  #endRecording(s: Running, ended: RecordingEnd): void {
    const rec = s.recording;
    if (!rec) return;
    s.recording = null;
    s.onEvent({ type: "recording", data: { state: "stopped", ...summaryOf(rec, ended) } });
  }

  /** There is no folder dialog in the browser: the choice is fixed. */
  async chooseRecordingFolder(): Promise<string | null> {
    return "/demo/recordings";
  }

  /** There is no folder in the browser: the proposed name, in the folder set, is the
   * "path". */
  async startRecording(suggestedName: string, options?: RecordingOptions): Promise<string | null> {
    const s = this.#open();
    if (s.recording) throw usage("A recording is already running.");
    const path = options?.folder ? `${options.folder}/${suggestedName}` : suggestedName;
    s.recording = {
      id: ++this.#lastRecordingId,
      path,
      rows: 0,
      elapsedMs: 0,
      sinceProgressMs: 0,
      paused: false,
      pauseFromMs: null,
      gaps: [],
      belowA: options?.below_a ?? null,
      forMs: options?.for_ms ?? 0,
      lowSinceMs: null,
      maxRows: options?.max_rows ?? DEFAULT_MAX_ROWS,
    };
    s.onEvent({ type: "recording", data: { state: "started" } });
    return path;
  }

  async stopRecording(): Promise<RecordingSummary> {
    const s = this.#open();
    const rec = recordingOf(s);
    s.recording = null;
    return summaryOf(rec, "stopped");
  }

  /** Like the backend: the stream goes on, and a pause adds one gap if rows came before it. */
  async pauseRecording(): Promise<void> {
    const s = this.#open();
    const rec = recordingOf(s);
    if (rec.paused) return;
    rec.paused = true;
    rec.lowSinceMs = null;
    if (rec.rows > 0) rec.pauseFromMs ??= s.nextMs;
  }

  async resumeRecording(): Promise<void> {
    recordingOf(this.#open()).paused = false;
  }

  /** There is no save dialog in the browser: the file stays where it is. */
  async saveRecordingAs(path: string): Promise<string | null> {
    return path;
  }

  /** Exports every sample streamed so far, finishing on the next timer tick (`slow-export`:
   * reports half the rows at once and finishes after {@link SLOW_EXPORT_MS}). */
  async exportSession(
    suggestedName: string,
    onProgress: (progress: ExportProgress) => void,
  ): Promise<ExportResult | null> {
    const s = this.#open();
    const folded = this.#exportFolded(s);
    const rows = folded === null ? s.streamed : s.history.length - folded.samples;
    this.#exports.push(s.history.csvHead(EXPORT_CSV_HEAD_ROWS));
    const slow = this.#mode === "slow-export";
    if (slow) onProgress({ done: Math.floor(rows / 2), total: rows });
    return new Promise((resolve) =>
      setTimeout(
        () => {
          onProgress({ done: rows, total: rows });
          resolve({ path: suggestedName, rows, folded });
        },
        slow ? SLOW_EXPORT_MS : 0,
      ),
    );
  }

  /** What an export of `s` leaves out once its history is past
   * {@link MockOptions.historyFullSamples} (AC-AXHK2W.14): the samples before the fold
   * boundary, their span and the file's start; `null` while nothing would be folded. */
  #exportFolded(s: Running): ExportFolded | null {
    const samples = this.#foldedSamples(s);
    if (samples === null) return null;
    const h = s.history;
    const ms = h.timeAt(samples) - h.timeAt(0);
    return {
      samples,
      ms,
      start_unix_ms: Date.now() - (h.timeAt(h.length - 1) - h.timeAt(samples)),
    };
  }

  /** How many of `s`'s oldest samples are past {@link MockOptions.historyFullSamples}
   * (the ones a real history would fold); `null` while none are. */
  #foldedSamples(s: Running): number | null {
    const limit = this.#options.historyFullSamples;
    if (limit == null || s.history.length <= limit) return null;
    return s.history.length - Math.max(0, Math.floor(limit));
  }

  /** The session's rows as a `.db` file: like {@link exportSession}, which it shares. */
  async exportSessionDb(
    suggestedName: string,
    onProgress: (progress: ExportProgress) => void,
  ): Promise<ExportResult | null> {
    return this.exportSession(suggestedName, onProgress);
  }

  /** Opens {@link MOCK_RECORDING_FILE}, or what `recordingFileFault` scripts. */
  async openRecordingFile(
    onProgress: (progress: OpenProgress) => void,
  ): Promise<FileSummary | null> {
    const { rows, name, skipped } = MOCK_RECORDING_FILE;
    const fault = this.#options.recordingFileFault ?? null;
    const untilCancel = async (): Promise<null> => {
      await new Promise<void>((resolve) => (this.#endRecordingOpen = resolve));
      return null;
    };
    if (fault === "dialog") return untilCancel();
    onProgress({ done: 0, total: 0 });
    if (fault === "refused") throw usage("This file's header is not a recorder header.");
    if (fault === "unknown-total") return untilCancel();
    onProgress({ done: Math.floor(rows / 2), total: rows });
    if (fault === "slow") return untilCancel();
    onProgress({ done: rows, total: rows });
    const stats = new SessionStats();
    const history = new HistoryColumns(rows);
    for (let t = 0; t < rows; t++) {
      const sample = sampleAt("load-5v-1a", t);
      if (fault === "dip" && t < MOCK_DIP_MS) sample.vbus_v = MOCK_DIP_VBUS_V;
      stats.push(sample);
      history.push(sample);
    }
    this.#recordingFile = history;
    return {
      name,
      rows,
      skipped,
      left_out: fault === "truncated" ? MOCK_LEFT_OUT : 0,
      stats: stats.snapshot(),
    };
  }

  /** Opens the recording handed over at launch, once, as {@link openRecordingFile} does;
   * `null` when none was (FR M078V5). */
  async openLaunchRecordingFile(
    onProgress: (progress: OpenProgress) => void,
  ): Promise<FileSummary | null> {
    if (!this.#launchFile) return null;
    this.#launchFile = false;
    return this.openRecordingFile(onProgress);
  }

  async cancelRecordingFile(): Promise<void> {
    this.#endRecordingOpen?.();
    this.#endRecordingOpen = null;
  }

  async closeRecordingFile(): Promise<void> {
    this.#recordingFile = null;
  }

  async recordingFileRange<const S extends readonly HistorySeries[]>(
    series: S,
    t0Ms: number,
    t1Ms: number,
    buckets: number,
  ): Promise<BucketsFor<S>> {
    const file = this.#recordingFile;
    if (!file) throw usage("No recording file is open.");
    return file.buckets(series, t0Ms, t1Ms, buckets) as BucketsFor<S>;
  }

  /** The ruler's stats over the open recording file's rows (FR AXHK2T). */
  async recordingFileRangeStats(t0Ms: number, t1Ms: number): Promise<RangeStats | null> {
    const file = this.#recordingFile;
    if (!file) throw usage("No recording file is open.");
    return file.rangeStats(t0Ms, t1Ms);
  }

  /** The ruler's stats over the session's history (FR AXHK2T); nothing counts across an
   * outage. */
  async rangeStats(t0Ms: number, t1Ms: number): Promise<RangeStats | null> {
    const s = this.#open();
    return s.history.rangeStats(t0Ms, t1Ms, s.gaps);
  }

  /** Segment marks and E.D. of the open recording file's rows (FR AXHK2T). */
  async recordingFileRunProgress(query: RunProgressQuery): Promise<RunProgress | null> {
    const file = this.#recordingFile;
    if (!file) throw usage("No recording file is open.");
    return file.runProgress(query);
  }

  /** Segment marks and E.D. of the session's current run, since its last outage
   * (FR AXHK2T). */
  async runProgress(query: RunProgressQuery): Promise<RunProgress | null> {
    const s = this.#open();
    return s.history.runProgress(query, s.gaps);
  }

  async stats(): Promise<StatsSnapshot> {
    const s = this.#open();
    return s.stats.snapshot();
  }

  async historyRange<const S extends readonly HistorySeries[]>(
    series: S,
    t0Ms: number,
    t1Ms: number,
    buckets: number,
  ): Promise<BucketsFor<S>> {
    const s = this.#open();
    // `HistoryColumns.buckets` maps `series` one to one, so the tuple shape holds.
    return s.history.buckets(series, t0Ms, t1Ms, buckets) as BucketsFor<S>;
  }

  async historyGaps(t0Ms: number, t1Ms: number): Promise<HistoryGap[]> {
    const s = this.#open();
    return s.gaps.filter((g) => g.to_ms > t0Ms && g.from_ms < t1Ms).map((g) => ({ ...g }));
  }

  /** Where the history's full resolution starts (FR AXHK2W, AC-AXHK2W.11): the oldest of
   * the newest {@link MockOptions.historyFullSamples} samples, once there are more; the
   * demo never actually folds. `null` while nothing would be folded. */
  async historyFoldBoundary(): Promise<number | null> {
    const s = this.#open();
    const samples = this.#foldedSamples(s);
    return samples === null ? null : s.history.timeAt(samples);
  }

  /** Replaces the history with `n` (at most {@link MAX_SYNTHETIC_SAMPLES}) scenario
   * samples, one per millisecond from 0, and moves the live stream past them: a large
   * history for the pan check (AC-AH2AEM.4). */
  async seedSyntheticHistory(n: number): Promise<void> {
    const s = this.#open();
    const count = Math.max(0, Math.min(Math.floor(n), MAX_SYNTHETIC_SAMPLES));
    const history = new HistoryColumns(count + SEED_HEADROOM);
    history.fillSynthetic(count, (t, out) => writeSample(s.scenario, t, out));
    s.history = history;
    s.nextMs = Math.max(s.nextMs, count);
  }

  /** Zeroes the stats; they count again from the next sample. The history stays. */
  async resetStats(): Promise<void> {
    const s = this.#open();
    s.stats = new SessionStats();
  }

  /** Discards the history; the stream and any recording go on. */
  async clearHistory(): Promise<void> {
    const s = this.#open();
    s.history = new HistoryColumns();
    s.gaps = [];
  }

  /** Starts the scripted PD capture: rows go out with every batch tick, like the
   * worker's at most 60 times a second. */
  async pdStart(): Promise<void> {
    if (this.#flashing) throw usage(FLASH_REFUSAL);
    const s = this.#open();
    // The PD capture waits while triggering. [inferred]
    if (this.#trigger) throw usage("Close the trigger session to capture PD traffic.");
    if (s.pdStartRefused) {
      throw {
        kind: "protocol",
        message: "The meter refused to start the PD capture.",
        hint: "Replug the meter and try again.",
      } satisfies UiError;
    }
    const pd = s.pd;
    if (pd.timer !== null) return;
    pd.timer = setInterval(() => this.#pdTick(s), BATCH_MS);
    s.onEvent({ type: "pd_state", data: { capturing: true } });
    const seed = this.#options.pdSynthetic ?? 0;
    const seeding = !pd.seeded && seed > 0;
    // Like the backend: one run's records sit on the samples' time base at a constant
    // offset, the run starting now (a seed ending now).
    const startMs = seeding ? pdRecordTime(pd.next + seed) : pd.nowMs;
    pd.sessionOffsetMs = Math.round(s.nextMs) - startMs;
    if (seeding) {
      pd.seeded = true;
      for (let from = 0; from < seed; from += PD_SEED_BATCH) {
        const rows: PdRow[] = [];
        for (let k = from; k < Math.min(seed, from + PD_SEED_BATCH); k++)
          rows.push(this.#capture(pd));
        s.onEvent({ type: "pd_rows", data: { rows } });
      }
      pd.nowMs = pdRecordTime(pd.next);
    }
  }

  /** Captures the next script record; its row. */
  #capture(pd: MockPdCapture): PdRow {
    const k = pd.next++;
    pd.records.push(k);
    const row = pdRow(k, pd.base + pd.records.length - 1);
    row.session_ms = row.elapsed_ms + pd.sessionOffsetMs;
    pd.rows.push(row);
    return row;
  }

  #pdTick(s: Running): void {
    const pd = s.pd;
    const rows: PdRow[] = [];
    while (pdRecordTime(pd.next) <= pd.nowMs) rows.push(this.#capture(pd));
    // The status blocks polled since the last tick, on the samples' time base.
    const samples: PdStatusSample[] = [];
    for (let t = pd.nowMs; t < pd.nowMs + BATCH_MS; t += PD_STATUS_MS) {
      const ms = t + pd.sessionOffsetMs;
      const m = measuredAt(s, ms);
      samples.push({
        session_ms: ms,
        vbus_v: m.vbus_v,
        ibus_a: m.ibus_a,
        cc1_v: m.cc1_v,
        cc2_v: m.cc2_v,
      });
    }
    pd.nowMs += BATCH_MS;
    pd.statusMs = Math.max(pd.statusMs, samples[samples.length - 1]?.session_ms ?? 0);
    if (rows.length > 0) s.onEvent({ type: "pd_rows", data: { rows } });
    s.onEvent({ type: "pd_status", data: { samples } });
  }

  /** Stops `s`'s PD capture, if it runs; `reason` says why when the app stopped it
   * (FR JQ971R), absent for a stop the user asked for or a disconnect. */
  #stopPd(s: Running, reason?: PdStopReason): void {
    if (s.pd.timer === null) return;
    clearInterval(s.pd.timer);
    s.pd.timer = null;
    s.onEvent({
      type: "pd_state",
      data: reason ? { capturing: false, reason } : { capturing: false },
    });
  }

  async pdStop(): Promise<void> {
    this.#stopPd(this.#open());
  }

  async pdDetails(index: number, opened = false): Promise<PdDetails> {
    if (opened) {
      const k = this.#openedPd?.records[index];
      if (k === undefined) throw usage("That PD record is not in the opened capture.");
      return pdDetailsOf(k, index);
    }
    const s = this.#open();
    const k = s.pd.records[index - s.pd.base];
    if (k === undefined) throw usage("That PD record is not in the capture.");
    return pdDetailsOf(k, index);
  }

  /** The demo meter's settings: the fields of the captured FW 2.0.6 block, all
   * tagged inferred (FR 1HHT01). */
  async meterSettings(): Promise<MeterSettings> {
    this.#refuseUnsavedSettings();
    this.#open().settingsRead = true;
    this.#meterSettingsReads += 1;
    return { fields: this.#meterSettings.map((f) => ({ ...f })) };
  }

  /** Writes `changes` to the demo meter like the backend (FR JKWT3W): unknown or
   * read-only keys, values out of range and an empty change are refused before the
   * write; with the `mismatch` fault the meter keeps its old values and the re-read
   * names the first field that did not change. */
  async meterSettingsWrite(changes: SettingChanges): Promise<MeterSettings> {
    this.#open();
    this.#refuseUnsavedSettings();
    const entries = Object.entries(changes) as [string, number][];
    if (entries.length === 0) throw usage("No setting was changed.");
    const shown = new Map<string, string>();
    for (const [key, code] of entries) {
      if (!Object.hasOwn(DEMO_WRITABLE_SETTINGS, key)) {
        throw usage(`The setting ${key} cannot be changed.`);
      }
      const text = demoSettingText(key, code);
      if (text === null) throw usage(`${key} = ${code} is out of range.`);
      shown.set(DEMO_SETTING_ROW[key] ?? key, text);
    }
    this.#meterSettingsWrites += 1;
    const next = this.#meterSettings.map((f) => ({ ...f, value: shown.get(f.key) ?? f.value }));
    if (this.#options.settingsFault === "mismatch") {
      const moved = next.find((f, i) => f.value !== this.#meterSettings[i]?.value);
      if (moved) {
        const old = this.#meterSettings.find((f) => f.key === moved.key)?.value ?? "?";
        throw {
          kind: "protocol",
          message: `After the write the meter shows ${moved.key} ${old}, expected ${moved.value}.`,
          hint: null,
        } satisfies UiError;
      }
    }
    this.#meterSettings = next;
    return { fields: this.#meterSettings.map((f) => ({ ...f })) };
  }

  /** How many settings writes were sent (for the UI tests). */
  meterSettingsWrites(): number {
    return this.#meterSettingsWrites;
  }

  /** How many times the settings were read (for the UI tests). */
  meterSettingsReads(): number {
    return this.#meterSettingsReads;
  }

  /** The demo meter's offline records (FR 1HHT00). */
  async recordsList(): Promise<RecordList> {
    this.#open();
    return { records: this.#records.map((r) => ({ ...r.entry })) };
  }

  /** Downloads demo record `index` (1-based). */
  async recordDownload(index: number): Promise<OfflineRecord> {
    this.#open();
    const r = this.#records[index - 1];
    if (!r || r.entry.points === 0) throw usage("That record is not on the meter.");
    this.#recordDownloads += 1;
    return {
      index: r.entry.index,
      name: r.entry.name,
      interval_s: r.entry.interval_s,
      samples: r.samples.map((x) => ({ ...x })),
    };
  }

  /** Deletes demo record `target` (1-based), or all with `"all"`, like the backend
   * (FR JKWT3V): the typed `confirm` must be the record's exact name (or `all`); the
   * records are backed up first; the list is read again and must match. */
  async recordsDelete(target: number | "all", confirm: string): Promise<RecordDeleteResult> {
    this.#open();
    const one = target === "all" ? null : this.#records.find((r) => r.entry.index === target);
    if (one === undefined) throw usage("That record is not on the meter.");
    const doomed = one ? [one] : this.#records;
    const want = one ? one.entry.name : "all";
    if (confirm !== want) {
      throw usage(`Type ${target === "all" ? "all" : `the record name "${want}"`} to delete.`);
    }
    if (this.#options.recordsFault === "backup") {
      throw {
        kind: "usage",
        message: "The record backup could not be written, so nothing was deleted.",
        hint: null,
      } satisfies UiError;
    }
    this.#recordBackups += 1;
    const backupDir = `/demo/open-powerz-backups/records-${this.#recordBackups}`;
    const backups = doomed.map(
      (r) =>
        `${backupDir}/${String(r.entry.index).padStart(2, "0")}_${r.entry.name.replace(/[^A-Za-z0-9]+/g, "_")}.csv`,
    );
    if (this.#options.recordsFault === "mismatch") {
      throw {
        kind: "protocol",
        message: `The meter's record list does not match after the delete. Backup: ${backupDir}`,
        hint: null,
      } satisfies UiError;
    }
    this.#records = this.#records
      .filter((r) => !doomed.includes(r))
      .map((r, i) => ({ entry: { ...r.entry, index: i + 1 }, samples: r.samples }));
    return {
      records: this.#records.map((r) => ({ ...r.entry })),
      backup_dir: backupDir,
      backups,
    };
  }

  /** How many records were downloaded (for the UI tests). */
  recordDownloads(): number {
    return this.#recordDownloads;
  }

  /** Drops every captured PD record; a running capture goes on. Like the backend,
   * indices keep counting: resolves to the index of the next record and the latest
   * status session time. */
  async pdClear(): Promise<PdCleared> {
    const hadOpened = this.#openedPd !== null;
    this.#openedPd = null;
    if (hadOpened && this.#session === null) return { next_index: 0, status_floor_ms: 0 };
    const pd = this.#open().pd;
    pd.base += pd.records.length;
    pd.records = [];
    pd.rows = [];
    return { next_index: pd.base, status_floor_ms: pd.statusMs };
  }

  /** The page no longer shows the opened capture. Needs no meter, like the backend. */
  async pdReleaseOpened(): Promise<void> {
    this.#openedPd = null;
  }

  /** There is no save dialog: the proposed name is the path. Resolves to the number of
   * records captured so far; the capture is kept for {@link pdOpen}. */
  async pdSave(suggestedName: string): Promise<ExportResult | null> {
    const pd = this.#open().pd;
    this.#savedPd = {
      path: suggestedName,
      records: [...pd.records],
      rows: pd.rows.map((r, i) => ({ ...r, index: i })),
    };
    return { path: suggestedName, rows: pd.records.length };
  }

  /** There is no open dialog: opens the capture saved last in the page (`null` when
   * none was). Needs no meter, like the backend. */
  async pdOpen(): Promise<PdOpened | null> {
    const saved = this.#savedPd;
    if (saved === null) return null;
    this.#openedPd = saved;
    return {
      path: saved.path,
      rows: saved.rows.map((r) => ({ ...r })),
      status: [],
      dropped: 0,
      damaged_line: null,
      oversized: 0,
      skipped: 0,
      truncated: false,
    };
  }

  /** Synthetic metadata of a KM003C firmware file, whatever `path` names; no file is
   * read, like the backend it needs no meter. */
  async firmwareInspect(path: string): Promise<FirmwareFileInfo> {
    void path;
    if (this.#mode === "firmware-bad-file") {
      throw usage("This is not a firmware file: its header is not a firmware container.");
    }
    return {
      vendor: "ChargerLab",
      container_version: "V1.0",
      model: this.#mode === "firmware-other-model" ? "KM002C" : "KM003C",
      version: "2.0.7",
      date: "2026.9.29",
      crc: "A175E4A0",
      size: 456896,
      stored_size: 456912,
      release_notes_en: "1. Demo note: improved PD capture.\n2. Demo note: minor fixes.",
    };
  }

  /** There is no open dialog: "chooses" a synthetic logo picture (FR MEHQC0), the small
   * one with `?logofile=small`. */
  async logoChoose(): Promise<string | null> {
    return this.#options.logoFile === "small" ? MOCK_LOGO_SMALL_FILE : MOCK_LOGO_FILE;
  }

  /** Previews a mock logo picture like the backend (FR MEHQC0, AC .1); any other path is
   * refused like a file that is no picture. */
  async logoPreview(path: string, alpha: boolean): Promise<LogoPreview> {
    return mockLogoPicture(path, alpha);
  }

  /** What the logo area of meter `deviceId` holds (FR MEHQC0, AC .7); with `full`, the
   * stored image too. */
  async logoRead(deviceId: string, full: boolean): Promise<LogoRead> {
    this.#logoSession(deviceId);
    const a = this.#logoArea;
    if (a.status !== "logo") {
      return { status: a.status, width: null, height: null, mode: null, image: null };
    }
    return {
      status: "logo",
      width: a.width,
      height: a.height,
      mode: a.mode,
      image: full ? a.rgba : null,
    };
  }

  /** Uploads mock picture `path` like the backend (FR MEHQC0, AC .3): chunk progress,
   * then read-back progress, at least 50 ms apart; `logoFault` stops it at that chunk. */
  async logoUpload(
    deviceId: string,
    path: string,
    alpha: boolean,
    onProgress: (progress: LogoProgress) => void,
  ): Promise<LogoWritten> {
    const s = this.#logoSession(deviceId);
    return this.#logoWrite(s, mockLogoPicture(path, alpha), onProgress);
  }

  /** Previews built-in logo `name` like the backend (FR 70X5YX). */
  async logoPreviewBuiltin(name: BuiltinLogoName): Promise<LogoPreview> {
    return mockBuiltinPreview(name);
  }

  /** Uploads built-in logo `name` like a picture (FR 70X5YX). */
  async logoUploadBuiltin(
    deviceId: string,
    name: BuiltinLogoName,
    onProgress: (progress: LogoProgress) => void,
  ): Promise<LogoWritten> {
    const s = this.#logoSession(deviceId);
    return this.#logoWrite(s, mockBuiltinPreview(name), onProgress);
  }

  /** Writes the logo `preview` describes on `s`: chunk progress, then read-back
   * progress, at least 50 ms apart; `logoFault` stops it at that chunk. */
  async #logoWrite(
    s: Running,
    preview: LogoPreview,
    onProgress: (progress: LogoProgress) => void,
  ): Promise<LogoWritten> {
    const before = this.#logoWritable(s);
    const total = preview.chunks;
    const fault = this.#options.logoFault ?? null;
    const step = Math.max(1, Math.ceil(total / 8));
    for (let done = 0; done < total;) {
      const next = Math.min(total, done + step);
      if (fault !== null && fault < next) {
        if (fault > done) onProgress({ step: "chunks", done: fault, total });
        // Chunk 0 carries the header, so chunks 0..k-1 landed read as the logo it describes.
        if (fault > 0) {
          this.#logoArea = {
            status: "logo",
            width: preview.width,
            height: preview.height,
            mode: preview.mode,
            rgba: preview.rgba,
          };
        }
        throw {
          kind: "protocol",
          message: `Writing the logo stopped at chunk ${fault}: REJECT.`,
          hint: "The startup duration was not changed, so the meter keeps its old screen. Try again.",
        } satisfies UiError;
      }
      done = next;
      await sleep(MOCK_LOGO_PROGRESS_MS);
      onProgress({ step: "chunks", done, total });
    }
    await this.#logoReadBack(preview.padded_size, onProgress);
    this.#logoArea = {
      status: "logo",
      width: preview.width,
      height: preview.height,
      mode: preview.mode,
      rgba: preview.rgba,
    };
    return this.#logoWritten(before, "3 s");
  }

  /** Removes the demo meter's logo like the backend (FR MEHQC0, AC .4). */
  async logoRemove(
    deviceId: string,
    onProgress: (progress: LogoProgress) => void,
  ): Promise<LogoWritten> {
    const before = this.#logoWritable(this.#logoSession(deviceId));
    await sleep(MOCK_LOGO_PROGRESS_MS);
    onProgress({ step: "chunks", done: 1, total: 1 });
    this.#logoArea = { status: "empty" };
    await this.#logoReadBack(16, onProgress);
    return this.#logoWritten(before, "off");
  }

  /** The session of meter `deviceId`; refused when it is not connected. */
  #logoSession(deviceId: string): Running {
    const s = this.#sessions.find((x) => x.id === deviceId || x.deviceId === deviceId);
    if (!s) throw usage("No meter is connected.");
    return s;
  }

  /** The startup duration before a logo write on `s`; refused like the backend before
   * the settings were read and on a KM002C. */
  #logoWritable(s: Running): string {
    if (!s.settingsRead) {
      throw usage("The logo was not written: SETTINGS was not read in this session.");
    }
    if (this.#mode === "logo-other-model") {
      throw usage("The logo was not written: the meter is a KM002C, logo upload needs a KM003C.");
    }
    return this.#startupRow();
  }

  /** The demo meter's startup duration row. */
  #startupRow(): string {
    return this.#meterSettings.find((f) => f.key === "startup_logo")?.value ?? "off";
  }

  /** Refuses a settings read or write while a logo write's settings wait for the
   * mock's menu save, like the backend does with a stale SETTINGS block (FR QG6RNY). */
  #refuseUnsavedSettings(): void {
    const at = this.#menuSaveAt;
    if (at === null) return;
    if (this.#options.menuSave !== "never" && Date.now() >= at) return;
    throw {
      kind: "settings_not_saved",
      message: "The meter has not saved its settings since the last change.",
      hint: MENU_SAVE_STEPS,
    } satisfies UiError;
  }

  /** A finished logo write that set the startup duration to `set` (FR QG6RNY): the
   * settings wait for the mock's menu save unless `?menusave=saved`. */
  #logoWritten(before: string, set: string): LogoWritten {
    this.#meterSettings = this.#meterSettings.map((f) =>
      f.key === "startup_logo" ? { ...f, value: set } : f,
    );
    if (this.#options.menuSave === "saved") {
      this.#menuSaveAt = null;
      return {
        duration_before: before,
        duration_set: set,
        settings: "saved",
        startup_duration: set,
      };
    }
    this.#menuSaveAt = Date.now() + MOCK_MENU_SAVE_MS;
    return {
      duration_before: before,
      duration_set: set,
      settings: "needs_menu_save",
      startup_duration: null,
    };
  }

  /** One read-only check of the settings after a logo write, like the backend (FR
   * QG6RNY): not saved until the mock's menu save, then saved with the row's value. */
  async logoSettingsCheck(deviceId: string): Promise<LogoSettingsCheck> {
    this.#logoSession(deviceId);
    const at = this.#menuSaveAt;
    if (at === null) throw usage("No logo write is waiting for a settings save.");
    if (this.#options.menuSave === "never" || Date.now() < at) {
      return { settings: "needs_menu_save", startup_duration: null };
    }
    this.#menuSaveAt = null;
    if (this.#options.menuSave === "calibration") {
      return { settings: "calibration_changed", startup_duration: null };
    }
    return { settings: "saved", startup_duration: this.#startupRow() };
  }

  /** Read-back progress of `bytes`, one read every 50 ms. */
  async #logoReadBack(bytes: number, onProgress: (progress: LogoProgress) => void): Promise<void> {
    const total = Math.ceil(bytes / MOCK_LOGO_READ_BYTES);
    for (let done = 1; done <= total; done++) {
      await sleep(MOCK_LOGO_PROGRESS_MS);
      onProgress({ step: "read_back", done, total });
    }
  }

  /** There is no open dialog: "chooses" a synthetic firmware file (FR V9SK55). */
  async firmwareChoose(): Promise<string | null> {
    return "/mock/KM003C_V2.0.7.mencrypt";
  }

  /** Plays a firmware update like the backend (FR V9SK55, AC .4), checking in its order:
   * the file and the typed confirmation first, then a session of the meter that `agreed`
   * does not cover refuses it, with nothing released (FR AXHK2F). The meter is then taken
   * from the stream and everything else is refused until the update ends; each stage goes
   * to `onStage`, the write stage with its progress. `firmware-fail-<stage>` fails at that
   * stage with the recovery steps. */
  async firmwareFlash(
    deviceId: string,
    path: string,
    confirmation: string,
    onStage: (stage: FlashEvent) => void,
    agreed: FlashHold[] = [],
  ): Promise<FlashDone> {
    const info = await this.firmwareInspect(path);
    if (confirmation.trim() !== info.version) {
      throw usage(`Type the file's version, ${info.version}, to confirm the update.`);
    }
    if (this.#flashing) throw usage(FLASH_REFUSAL);
    const meter = this.#listedMeter();
    const isMeter = meter?.id === deviceId;
    if (!isMeter && !scenarioOf(deviceId)) throw usage("That meter is no longer attached.");
    const s = this.#sessions.find((x) => x.id === deviceId || x.deviceId === deviceId);
    if (s) {
      const listed = (await this.listDevices()).find((d) => d.id === s.deviceId);
      const rec = s.recording;
      const hold: FlashHold = {
        id: s.id,
        label: listed?.label ?? s.id,
        recording: rec !== null,
        ...(rec && { recording_id: rec.id }),
      };
      if (!agreed.some((a) => a.id === hold.id && (!rec || a.recording_id === rec.id))) {
        throw {
          kind: "flash_needs_consent",
          message: "The firmware update would end a running session.",
          hint: null,
          sessions: [hold],
        } satisfies UiError;
      }
    }
    this.#flashing = true;
    this.#flashTarget = deviceId;
    this.#flashStop = false;
    this.#flashErasing = false;
    this.#firmwareFlashes.push({ deviceId, path, confirmation });
    // Like the backend: the meter's trigger session and stream let go of it first, and
    // the other meters keep running.
    if (s) {
      if (this.#session === s) this.#closeTrigger();
      if (this.#replugTimer !== null && this.#replugFor === s) {
        clearTimeout(this.#replugTimer);
        this.#replugTimer = null;
      }
      this.#endSession(s, null);
    }
    const failAt = this.#mode?.startsWith("firmware-fail-")
      ? this.#mode.slice("firmware-fail-".length)
      : null;
    const fail = (stage: string): UiError => ({
      kind: "protocol",
      message: `The firmware update failed: ${MOCK_FLASH_FAILURES[stage] ?? stage}.`,
      hint: FLASH_RECOVERY_HINT,
    });
    // `replug`: the listed meter turns to bootloader mode once switched, and back before
    // the update ends, as the backend lists a real meter jumping.
    // `replug-late`: the meter is still in bootloader mode when the update ends and is
    // listed in application mode again only after it (FR PGY704).
    const jumps = isMeter && this.#replugLike();
    const late = jumps && this.#mode === "replug-late";
    let relistLate = false;
    const setReplugMode = (mode: "application" | "bootloader"): void => {
      if (!jumps || this.#replugMode === mode) return;
      this.#replugMode = mode;
      void this.#notifyDevices();
    };
    try {
      const fromBootloader = isMeter && meter?.mode === "bootloader";
      for (const stage of ["switching", "erasing"] as const) {
        if (stage === "switching" && fromBootloader) continue;
        if (this.#flashStop) {
          throw { kind: "cancelled", message: FLASH_STOPPED, hint: null } satisfies UiError;
        }
        if (stage === "erasing") this.#flashErasing = true;
        onStage({ stage });
        await sleep(MOCK_FLASH_MS[stage]);
        if (failAt === stage) throw fail(stage);
        if (stage === "switching") {
          setReplugMode("bootloader");
          // The prompt shows whether or not the meter restarts by itself.
          onStage({ stage: "replug", to: "bootloader", reply: "none" });
          await sleep(MOCK_FLASH_MS.replug);
        }
      }
      const total = info.stored_size;
      for (let k = 0; k <= MOCK_FLASH_WRITES; k++) {
        onStage({ stage: "writing", done: Math.round((total * k) / MOCK_FLASH_WRITES), total });
        if (failAt === "writing" && k === 3) throw fail("writing");
        if (k < MOCK_FLASH_WRITES) await sleep(MOCK_FLASH_MS.write);
      }
      for (const stage of ["verifying", "finished", "returning"] as const) {
        onStage({ stage });
        await sleep(MOCK_FLASH_MS[stage]);
        if (failAt === stage) throw fail(stage);
      }
      onStage({ stage: "replug", to: "application", reply: "none" });
      await sleep(MOCK_FLASH_MS.replug);
      if (late) {
        relistLate = true;
        setTimeout(() => setReplugMode("application"), MOCK_LATE_RELIST_MS);
      } else {
        setReplugMode("application");
      }
      this.#firmwareVersion = info.version;
      return { outcome: "verified", version: info.version };
    } finally {
      if (!relistLate) setReplugMode("application");
      this.#flashing = false;
    }
  }

  /** Asks the running update to stop, like the backend: until the erase starts. */
  async firmwareCancel(): Promise<void> {
    if (!this.#flashing) throw usage("No firmware update is running.");
    if (this.#flashErasing) {
      // Loaded here, not on top: the Playwright helpers import this file outside Vite,
      // which cannot read the sentence's `.txt?raw` file.
      const { CANCEL_TOO_LATE } = await import("./firmwareUpdate.svelte");
      throw usage(CANCEL_TOO_LATE);
    }
    this.#flashStop = true;
  }

  /** The update running across a reload: only `firmware-running` has one, since the
   * mock's own updates end with the page. */
  async firmwareFlashStatus(): Promise<FlashStatus | null> {
    return this.#mode === "firmware-running" ? MOCK_RUNNING_FLASH : null;
  }

  /** The updates this mock ran (FR V9SK55), for the UI tests to check nothing was sent. */
  firmwareFlashLog(): { deviceId: string; path: string; confirmation: string }[] {
    return this.#firmwareFlashes.map((f) => ({ ...f }));
  }

  /** There is no open dialog: imports a vendor log of one cycle of the demo PD
   * script. Needs no meter, like the backend. */
  async pdImport(): Promise<PdOpened | null> {
    const records: number[] = [];
    for (let k = 0; pdRecordTime(k) < PD_CYCLE_MS; k++) records.push(k);
    const rows = records.map((k, i) => pdRow(k, i));
    this.#openedPd = { path: "vendor-pd-log.sqlite", records, rows };
    return {
      path: "vendor-pd-log.sqlite",
      rows: rows.map((r) => ({ ...r })),
      status: [],
      dropped: 0,
      damaged_line: null,
      oversized: 0,
      skipped: 0,
      truncated: false,
    };
  }

  async appInfo(): Promise<AppInfo> {
    return { appVersion: "0.1.0 (mock backend)", protocolVersion: "0.1.0" };
  }

  /** There is no file manager to open: the request is recorded instead. */
  async revealPath(path: string): Promise<void> {
    this.#revealed.push(path);
  }

  /** There is no save dialog: the proposed name is the path, and the image is recorded
   * (FR AXHK2V). */
  async saveChartImage(
    suggestedName: string,
    format: string,
    data: Uint8Array,
  ): Promise<string | null> {
    this.#chartImages.push({ suggestedName, format, data: Array.from(data) });
    return suggestedName;
  }

  /** How many chart images were saved: cheap to poll, where {@link chartImages} copies
   * every image's bytes out of the page (FR X6VSFF). */
  chartImageCount(): number {
    return this.#chartImages.length;
  }

  /** Every chart image saved, oldest first. */
  chartImages(): { suggestedName: string; format: string; data: number[] }[] {
    return this.#chartImages.map((i) => ({ ...i, data: [...i.data] }));
  }

  /** Holds every stream (`true`) or lets it go on (`false`): held, no batch is sent and
   * no sample is made, and the stream goes on from where it stopped, with no gap. For
   * the UI suite's idle frame-rate windows (FR 9QTPNQ). */
  holdStream(held: boolean): void {
    this.#held = held;
  }

  /** How many batches every stream has sent so far: the UI suite waits on it to know a
   * held stream goes on again, whatever tab shows (FR 9QTPNQ). */
  batchCount(): number {
    return this.#batchesSent;
  }

  /** Every path `revealPath` was asked to show, oldest first. */
  revealedPaths(): string[] {
    return [...this.#revealed];
  }

  /** The first rows of each export as CSV text, oldest first: what the export wrote
   * comes from the samples, never from how the chart draws them (AC-AXHK2T.7). */
  exportedCsv(): string[] {
    return [...this.#exports];
  }

  // ---- Trigger panel (FR DGSQ6S): a scripted charger behind the demo meter ----

  /** Opens the trigger session on the connected demo meter. */
  async triggerOpen(): Promise<void> {
    if (this.#flashing) throw usage(FLASH_REFUSAL);
    if (!this.#session) throw usage("Connect a meter first.");
    if (this.#trigger) return;
    const s = this.#session;
    // Like the backend: the PD capture stops (with the reason) and the queue runs at
    // 50 SPS or less.
    this.#stopPd(s, "trigger");
    let rateBefore: number | null = null;
    if (s.rateSps > TRIGGER_MAX_SPS && !s.recording && this.#mode !== "rate-locked") {
      rateBefore = s.rateSps;
      await this.setRate(TRIGGER_MAX_SPS);
    }
    s.trigger = { entered: null, rateBefore, armed: false, goingAway: false, chargerGone: false };
    if (this.#mode === "pd-start-refused") s.pdStartRefused = true;
    this.#triggerLog.push("pdm open");
  }

  /** The open trigger session, or a usage error. */
  #openTrigger(): MockTrigger {
    if (!this.#trigger) throw usage("No trigger session is open.");
    return this.#trigger;
  }

  /** Like the 140 W charger: Apple fails, the automatic list stalls and ends with `reset`,
   * then the `pdm parm` check that the meter is still in step. */
  async triggerDetect(): Promise<Detection> {
    const t = this.#openTrigger();
    t.armed = false;
    this.#triggerLog.push("entry bc", "entry apple", "entry list", "reset", "pdm parm");
    t.entered = null;
    this.#scheduleCcGone(t);
    if (this.#mode === "trigger-list-lag") {
      // The meter answers the check one command late, like the backend's `OutOfSync`.
      throw outOfSyncError();
    }
    return {
      // `trigger-detect-disagree`: `entry bc` → `fail` against the list's
      // `bc1.2:true` (device trace 2026-10-23).
      bc12: this.#mode === "trigger-detect-disagree" ? "not-supported" : "unknown",
      apple: "not-supported",
      entries: [
        { name: "bc1.2", support: "supported", vbus_mv: 5203, ibus_ma: 0 },
        { name: "ufcs", support: "not-supported", vbus_mv: null, ibus_ma: null },
        { name: "vfcp", support: "not-supported", vbus_mv: null, ibus_ma: null },
      ],
      complete: false,
      // The demo stream's D+ / D- levels, as `detect::hint` labels them.
      hint: "hint: DCP / QC2 12 V (D+ 0.600 V, D- 0.600 V)",
    };
  }

  /** The scripted charger speaks PD and QC; every other protocol is not supported. */
  async triggerList(protocol: TriggerProtocol): Promise<ProtocolSection> {
    const t = this.#openTrigger();
    this.#triggerLog.push(`entry ${protocol}`);
    if (protocol === "pd") {
      this.#triggerLog.push("pdm parm");
      t.entered = "pd";
      t.ccLow = false;
      if (t.chargerGone) {
        t.chargerGone = false;
        if (this.#session) this.#session.triggerVbusV = null;
      }
      return {
        protocol,
        supported: true,
        offers: MOCK_TRIGGER_OFFERS.map((o) => ({ ...o })),
        limits: [],
        settings: { pd_type: "auto", emarker: "50v-240w-epr", sink: "pps-3a" },
      };
    }
    t.entered = "legacy";
    if (this.#mode === "trigger-all") {
      const limits = (MOCK_ALL_LIMITS[protocol] ?? []).map((l) => ({ ...l }));
      return { protocol, supported: true, offers: [], limits, settings: null };
    }
    return { protocol, supported: protocol === "qc", offers: [], limits: [], settings: null };
  }

  /** The voltage `request` asks for, mV; null for a PD position with no offer. */
  #requestMv(request: TriggerRequest): number | null {
    switch (request.protocol) {
      case "pd": {
        if (request.voltage_mv !== null) return request.voltage_mv;
        const offer = MOCK_TRIGGER_OFFERS.find((o) => o.position === request.position);
        return offer ? offer.max_voltage_mv : null;
      }
      case "qc":
      case "fcp":
      case "afc":
      case "sfcp":
        return request.volts * 1000;
      default:
        return request.voltage_mv;
    }
  }

  /** Like the library classifier: above 5 V, or unknown, needs a confirmation. */
  async triggerRisk(request: TriggerRequest): Promise<RiskClass> {
    return this.#risk(this.#openTrigger(), request);
  }

  /** {@link triggerRisk}'s answer, for the request's own check: a test that slows the
   * question does not slow the request too. */
  #risk(t: MockTrigger, request: TriggerRequest): RiskClass {
    if (t.chargerGone) throw usage(CHARGER_GONE_REFUSAL);
    const mv = this.#requestMv(request);
    if (mv !== null && mv <= 5000) return "at_most_5v";
    if (!this.#rememberable(request)) return "above_5v";
    return t.armed ? "armed" : "above_5v_rememberable";
  }

  /** Like the library's `pd::rememberable`: a fixed, PPS or SPR AVS offer at an SPR
   * position, at or below 20 V. */
  #rememberable(request: TriggerRequest): boolean {
    if (request.protocol !== "pd") return false;
    const offer = MOCK_TRIGGER_OFFERS.find((o) => o.position === request.position);
    if (!offer || offer.epr) return false;
    const { voltage_mv: requested } = request;
    if (offer.kind === "fixed") return requested === null && offer.max_voltage_mv <= 20000;
    if (offer.kind !== "pps" && offer.kind !== "spr-avs") return false;
    return requested !== null && Math.max(requested, offer.min_voltage_mv) <= 20000;
  }

  /** The command text the meter would receive for `request`. */
  #commandText(request: TriggerRequest): string {
    switch (request.protocol) {
      case "pd":
        if (request.voltage_mv !== null && request.current_ma !== null) {
          return `pd req=${request.position},volt=${request.voltage_mv},cur=${request.current_ma}`;
        }
        return request.current_ma !== null
          ? `pd req=${request.position},cur=${request.current_ma}`
          : `pd req=${request.position}`;
      case "qc":
      case "fcp":
      case "afc":
      case "sfcp":
        return `${request.protocol} ${request.volts}V`;
      case "qc3":
        return `qc3 volt=${request.voltage_mv}`;
      case "ufcs":
        return `ufcs req=${request.index},volt=${request.voltage_mv},cur=${request.current_ma}`;
      default:
        return `${request.protocol} volt=${request.voltage_mv},cur=${request.current_ma}`;
    }
  }

  /** Why the protocol or the decoded limits refuse `request`, or null when they allow it. */
  #refusal(request: TriggerRequest): string | null {
    switch (request.protocol) {
      case "pd":
        return null;
      case "qc":
      case "fcp":
      case "afc":
      case "sfcp":
        return MOCK_FIXED_VOLTS[request.protocol].includes(request.volts)
          ? null
          : `${request.protocol.toUpperCase()} does not offer ${request.volts} V.`;
      case "qc3": {
        const mv = request.voltage_mv;
        return mv >= 3600 && mv <= 20000 && (mv - 3600) % 200 === 0
          ? null
          : "QC 3.0 runs from 3.6 to 20 V in 200 mV steps.";
      }
      default: {
        const all = this.#mode === "trigger-all" ? (MOCK_ALL_LIMITS[request.protocol] ?? []) : [];
        if (all.length === 0) return null;
        const l = request.protocol === "ufcs" ? all.find((b) => b.index === request.index) : all[0];
        if (!l) return "The charger reports no such capability.";
        const { voltage_mv: v, current_ma: c } = request;
        // Written so that NaN fails too.
        if (!(v >= l.min_voltage_mv && v <= l.max_voltage_mv))
          return "The voltage is outside the limits.";
        if (!(c >= l.min_current_ma && c <= l.max_current_ma))
          return "The current is outside the limits.";
        if (l.max_power_mw !== null && v * c > l.max_power_mw * 1000) {
          return "The request is above the capability's power.";
        }
        return null;
      }
    }
  }

  /** Whether "don't ask again" is armed; false with no trigger session. */
  async triggerArmed(): Promise<boolean> {
    return this.#trigger?.armed ?? false;
  }

  /** Ends "don't ask again"; does nothing with no trigger session. */
  async triggerDisarm(): Promise<void> {
    if (this.#trigger) this.#trigger.armed = false;
  }

  /** Whether VBUS collapsed under a contract; false with no trigger session. */
  async triggerChargerGone(): Promise<boolean> {
    return this.#trigger?.chargerGone ?? false;
  }

  /** Whether PD was entered last, like the backend; false with no trigger session. */
  async triggerPdEntered(): Promise<boolean> {
    return this.#trigger?.entered === "pd";
  }

  /** Refuses an unconfirmed above-5 V request before anything is sent, and a `remember`
   * the offers do not allow; an armed session covers what is rememberable. */
  async triggerRequest(
    request: TriggerRequest,
    confirmed: boolean,
    remember = false,
  ): Promise<TriggerReply> {
    const t = this.#openTrigger();
    if (t.chargerGone) throw usage(CHARGER_GONE_REFUSAL);
    const mv = this.#requestMv(request);
    if (mv === null && request.protocol === "pd") {
      throw usage(`The charger offers nothing at position ${request.position}.`);
    }
    const refusal = this.#refusal(request);
    if (refusal !== null) throw usage(refusal);
    const risk = this.#risk(t, request);
    const arming = confirmed && remember;
    if (arming && risk !== "above_5v_rememberable" && risk !== "armed") {
      throw usage("This request cannot be remembered.");
    }
    if ((risk === "above_5v" || risk === "above_5v_rememberable") && !confirmed) {
      throw usage("A request above 5 V needs your confirmation.");
    }
    const command = this.#commandText(request);
    // Like the session: disarmed before the first write, so a failure leaves "don't ask
    // again" ended. Only a request that qualifies keeps it.
    const wasArmed = t.armed;
    t.armed = false;
    this.#triggerLog.push(command);
    if (this.#mode === "trigger-out-of-sync") {
      // The meter answers with another command's reply (`type=...`), like the backend's
      // `OutOfSync` refusal.
      throw outOfSyncError();
    }
    if (this.#mode === "trigger-req-error" && mv !== null && mv > 5000) {
      // The charger answers every request above 5 V with `req error 252`, like the
      // backend's failed request; the session stays usable.
      throw {
        kind: "protocol",
        message: "The charger refused the request (req error 252).",
        hint: null,
      } satisfies UiError;
    }
    t.entered = request.protocol === "pd" ? "pd" : "legacy";
    t.armed = arming || (wasArmed && risk === "armed");
    if (this.#session && mv !== null) {
      // The demo VBUS follows the request; `trigger-sag` keeps it 12 % below.
      this.#session.triggerVbusV = (mv / 1000) * (this.#mode === "trigger-sag" ? 0.88 : 0.985);
    }
    this.#scheduleChargerGone(t);
    if (request.protocol !== "pd") {
      return {
        command,
        requested_mv: null,
        requested_ma: null,
        delivered_mv: null,
        delivered_ma: null,
        reply: "ready",
      };
    }
    const offer = MOCK_TRIGGER_OFFERS.find((o) => o.position === request.position);
    const requestedMv = mv ?? 5000;
    return {
      command,
      requested_mv: requestedMv,
      requested_ma: request.current_ma ?? offer?.max_current_ma ?? null,
      delivered_mv: Math.round(requestedMv * 0.985),
      delivered_ma: 0,
      reply: null,
    };
  }

  /** `trigger-charger-gone`: VBUS falls to 0 V a moment after the first accepted request,
   * and the session finds out on its own, with nothing sent. */
  #scheduleChargerGone(t: MockTrigger): void {
    this.#scheduleGone(t, "trigger-charger-gone", CHARGER_GONE_AFTER_MS, (s) => {
      s.triggerVbusV = 0;
    });
  }

  /** `trigger-cc-gone`: a moment after the first Detect both CC lines fall to 0 V and
   * the session finds the charger gone on its own, with no request and nothing sent. */
  #scheduleCcGone(t: MockTrigger): void {
    this.#scheduleGone(t, "trigger-cc-gone", CC_GONE_AFTER_MS, () => {
      t.ccLow = true;
    });
  }

  /** Once per session in `mode`: after `afterMs` the charger goes away (`drop` changes
   * the stream), unless the session ended or another one took its place. */
  #scheduleGone(t: MockTrigger, mode: MockMode, afterMs: number, drop: (s: Running) => void): void {
    const s = this.#session;
    if (this.#mode !== mode || t.goingAway || !s) return;
    t.goingAway = true;
    setTimeout(() => {
      if (s.trigger !== t) return;
      drop(s);
      t.armed = false;
      t.chargerGone = true;
    }, afterMs);
  }

  /** Back to 5 V: `pd req=1` after PD, `reset` after the others; the session stays.
   * A leave of the view (FR AXHK2N) does the same: the mock keeps no armed-end log. */
  async triggerBackTo5v(): Promise<void> {
    this.#returnTo5v(this.#openTrigger());
  }

  /** Back to 5 V, then `pdm close`; nothing when no session is open. */
  async triggerClose(): Promise<void> {
    this.#closeTrigger();
  }

  #returnTo5v(t: MockTrigger): void {
    t.armed = false;
    if (t.entered === "pd") this.#triggerLog.push("pd req=1");
    else if (t.entered === "legacy") this.#triggerLog.push("reset");
    if (this.#session) this.#session.triggerVbusV = null;
    t.backAt = this.#triggerLog.length;
  }

  #closeTrigger(): void {
    const t = this.#trigger;
    if (!t) return;
    if (t.backAt !== this.#triggerLog.length) this.#returnTo5v(t);
    this.#triggerLog.push("pdm close");
    const s = this.#session;
    if (s) s.trigger = null;
    if (s && t.rateBefore !== null && !s.recording) {
      s.rateSps = t.rateBefore;
      s.carry = 0;
      s.onEvent({
        type: "status",
        data: { state: "rate", rate_sps: t.rateBefore, rate_locked: false },
      });
    }
  }

  // ---- Custom PD messages and the raw console (FR DGSQ6T) ----

  /** The library risk class of a built message; GoodCRC and reserved control types are
   * refused with a reason. */
  async triggerMessageRisk(message: PdMessageRequest): Promise<RiskClass> {
    this.#openTrigger();
    // Like the backend: exactly this message, never the raw console's same bytes.
    if (renderMockMessage(message) === CABLE_READ_LINE) return "at_most_5v";
    if (message.data_objects.length === 0) {
      return SWAP_CONTROL_TYPES.includes(message.msg_type) ? "above_5v" : "at_most_5v";
    }
    // Request / EPR_Request go by the requested position; every other data message
    // is above 5 V, like the library classifier.
    if (message.sop !== "sop" || (message.msg_type !== 2 && message.msg_type !== 9)) {
      return "above_5v";
    }
    const position = (message.data_objects[0]! >>> 28) & 0xf;
    const offer = MOCK_TRIGGER_OFFERS.find((o) => o.position === position);
    return offer && offer.max_voltage_mv <= 5000 ? "at_most_5v" : "above_5v";
  }

  /** Sends a built message to the scripted charger (or the demo e-marked cable). */
  async triggerMessageSend(message: PdMessageRequest, confirmed: boolean): Promise<PdSendReply> {
    const t = this.#openTrigger();
    const command = renderMockMessage(message);
    if ((await this.triggerMessageRisk(message)) === "above_5v" && !confirmed) {
      throw usage("A message that can raise VBUS above 5 V needs your confirmation.");
    }
    t.armed = false;
    // Like the backend: PD first, unless PD is what was entered last.
    if (t.entered !== "pd") {
      this.#triggerLog.push("entry pd");
      t.entered = "pd";
    }
    this.#triggerLog.push(command);
    // The demo e-marked cable (the session-2 240 W passive cable) answers a
    // Discover Identity REQ to a cable plug.
    const discover = isCableDiscoverIdentity(message);
    return {
      command,
      lines: [{ at_ms: Date.now(), direction: "tx", text: command, block: null }],
      cable: discover ? this.#cableAnswer() : null,
    };
  }

  /** What the cable answers a Discover Identity, by mode. */
  #cableAnswer(): CableView | null {
    if (this.#mode === "trigger-no-cable") return null;
    if (this.#mode === "trigger-unlisted-cable") {
      return { ...MOCK_CABLE, vid: 0x1234, vendor: null };
    }
    return { ...MOCK_CABLE };
  }

  /** One SOP' Discover Identity, only with PD entered last; no retry. */
  async triggerCableRead(): Promise<CableRead> {
    const t = this.#openTrigger();
    if (t.entered !== "pd") {
      throw usage("Reading the cable needs an open PD session. Press a PD offer first.");
    }
    if (this.#mode === "trigger-out-of-sync") throw outOfSyncError();
    t.armed = false;
    this.#triggerLog.push(CABLE_READ_LINE);
    const cable = this.#cableAnswer();
    return cable ? { kind: "found", cable } : { kind: "no_answer" };
  }

  /** The cable identity a capture holds; needs no meter and sends nothing. */
  async pdCableSeen(): Promise<CableView | null> {
    const held = [this.#openedPd?.records, this.#session?.pd.records];
    return held.some((records) => records?.some(pdIsCableDiscover)) ? { ...MOCK_CABLE } : null;
  }

  /** The library risk class of a raw console line. */
  async triggerConsoleRisk(line: string): Promise<RiskClass> {
    this.#openTrigger();
    return mockLineRisk(line.trim());
  }

  /** Sends a raw console line to the scripted charger. */
  async triggerConsoleSend(line: string, confirmed: boolean): Promise<ConsoleLine[]> {
    const t = this.#openTrigger();
    const text = line.trim();
    // Like the backend: GoodCRC is refused before anything is sent.
    const refusal = lineRefusal(text);
    if (refusal !== null) throw usage(refusal);
    const risk = mockLineRisk(text);
    if (risk === "above_5v" && !confirmed) {
      throw usage("A line that can raise VBUS above 5 V needs your confirmation.");
    }
    if (risk !== "read_only") t.armed = false;
    this.#triggerLog.push(text);
    if (this.#mode === "trigger-out-of-sync") throw outOfSyncError();
    const at = Date.now();
    const lines: ConsoleLine[] = [{ at_ms: at, direction: "tx", text, block: null }];
    const rx = (reply: string, block: string | null = null): void => {
      lines.push({ at_ms: at, direction: "rx", text: reply, block });
    };
    const req = /^pd req=(\d+)/.exec(text);
    if (text === "pdm parm") {
      rx("type=0,em=2,sink=0");
    } else if (text === "entry pd") {
      t.entered = "pd";
      rx("ok");
      rx("pdo:", mockPdoBlock());
      rx("ready:5000mV,0mA");
    } else if (/^entry \S+$/.test(text)) {
      t.entered = "legacy";
      rx("ok");
    } else if (req) {
      t.entered = "pd";
      const offer = MOCK_TRIGGER_OFFERS.find((o) => o.position === Number(req[1]));
      rx(offer ? `ready:${offer.max_voltage_mv}mV,0mA` : "fail");
    }
    return lines;
  }

  /** Every command the scripted charger received (`pd req=2`, `pd req=1`, `reset`,
   * `pdm close`, ...), oldest first: what the UI suite checks Cancel against. */
  triggerCommands(): string[] {
    return [...this.#triggerLog];
  }
}
