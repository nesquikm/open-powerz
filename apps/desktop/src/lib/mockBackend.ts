/**
 * A scripted stand-in for the Tauri backend, so the web build runs in a
 * plain browser (`VITE_BACKEND=mock`): the Playwright UI suite and the
 * visual checks drive it. Its scenarios follow the simulator's shapes in
 * `crates/powerz-sim`; like the worker, it reports the snapshot temperature
 * at every rate, 1000 SPS included.
 */

import type {
  AppInfo,
  AttachedSession,
  BackendEvent,
  BucketsFor,
  DeviceListing,
  ExportProgress,
  ExportResult,
  FirmwareFileInfo,
  FlashDone,
  FlashEvent,
  Gap,
  HistoryGap,
  HistorySeries,
  MeterSettingField,
  MeterSettings,
  OfflineRecord,
  OfflineRecordEntry,
  OfflineRecordSample,
  RecordDeleteResult,
  SettingChanges,
  RecordList,
  MinMaxAvg,
  PdDetails,
  PdCleared,
  PdOpened,
  PdRow,
  PdStatusSample,
  ProtocolSection,
  RecordingSummary,
  RiskClass,
  Sample,
  Series,
  SeriesBuckets,
  StatsSnapshot,
  Detection,
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
} from "./backend";
import { widenToNeighbours } from "./chartData";
import { lineRefusal, messageRefusal } from "./console.svelte";
import { PD_CYCLE_MS, pdContractAt, pdDetailsOf, pdRecordTime, pdRow } from "./mockPd";

const SCENARIOS = ["idle", "load-5v-1a", "pd-9v-2a", "noisy-negative"] as const;
type Scenario = (typeof SCENARIOS)[number];

/** Batches go out every 25 ms: 40 Hz, inside the backend's 20–60 Hz band. */
const BATCH_MS = 25;

/** Recording progress goes out every 500 ms, like the backend's. */
const PROGRESS_MS = 500;

function usage(message: string): UiError {
  return { kind: "usage", message, hint: null };
}

/** Why the mock refuses a request while a firmware update holds the meter (FR V9SK55). */
const FLASH_REFUSAL = "A firmware update is running. Wait for it to finish before using the meter.";

/** The recovery steps of a failed update, as the backend gives them (FR V9SK55, AC .5). */
const FLASH_RECOVERY_HINT =
  "Hold the Back button while plugging the meter in to start it in bootloader mode, then run the update again.";

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

/** Running min / max / sum of one quantity. */
class RunningMinMax {
  min = Infinity;
  max = -Infinity;
  sum = 0;

  push(x: number): void {
    this.min = Math.min(this.min, x);
    this.max = Math.max(this.max, x);
    this.sum += x;
  }

  snapshot(n: number): MinMaxAvg | null {
    return n > 0 ? { min: this.min, max: this.max, avg: this.sum / n } : null;
  }
}

/** Statistics over the samples since the last reset, like `src-tauri/src/stats.rs`:
 * charge and energy by the trapezoid rule, never across a reset. */
class MockStats {
  #first: number | null = null;
  #last: Sample | null = null;
  #samples = 0;
  #chargeAh = 0;
  #energyWh = 0;
  #vbus = new RunningMinMax();
  #ibus = new RunningMinMax();
  #power = new RunningMinMax();

  push(s: Sample): void {
    const prev = this.#last;
    if (prev) {
      const dtH = (s.t_ms - prev.t_ms) / 3_600_000;
      this.#chargeAh += ((prev.ibus_a + s.ibus_a) / 2) * dtH;
      this.#energyWh += ((prev.vbus_v * prev.ibus_a + s.vbus_v * s.ibus_a) / 2) * dtH;
    }
    this.#first ??= s.t_ms;
    this.#last = s;
    this.#samples += 1;
    this.#vbus.push(s.vbus_v);
    this.#ibus.push(s.ibus_a);
    this.#power.push(s.vbus_v * s.ibus_a);
  }

  snapshot(): StatsSnapshot {
    const n = this.#samples;
    return {
      elapsed_ms: this.#first === null || !this.#last ? 0 : this.#last.t_ms - this.#first,
      samples: n,
      charge_ah: this.#chargeAh,
      energy_wh: this.#energyWh,
      vbus: this.#vbus.snapshot(n),
      ibus: this.#ibus.snapshot(n),
      power: this.#power.snapshot(n),
    };
  }
}

const SERIES_FIELD: Record<Series, keyof Sample> = {
  vbus: "vbus_v",
  ibus: "ibus_a",
  dp: "dp_v",
  dm: "dm_v",
  cc1: "cc1_v",
  cc2: "cc2_v",
};

const FIELDS = Object.keys(SERIES_FIELD) as Series[];

/** Spare room left after a synthetic seed, so live samples append without a copy. */
const SEED_HEADROOM = 1 << 20;

/** The session history as columns (sample times plus one column per series), so a
 * range is found by binary search and bucketed without building sample objects:
 * a pan over a 10-million-sample history stays well inside a frame (AC-AH2AEM.4). */
class HistoryColumns {
  length = 0;
  #t = new Float64Array(0);
  /** Float32 values (plenty for a demo double) keep a 10M seed near 320 MB. */
  #cols = {} as Record<Series, Float32Array>;

  constructor(capacity = 1024) {
    this.#alloc(capacity);
  }

  #alloc(capacity: number): void {
    const t = new Float64Array(capacity);
    t.set(this.#t.subarray(0, this.length));
    this.#t = t;
    for (const name of FIELDS) {
      const col = new Float32Array(capacity);
      const old = this.#cols[name] as Float32Array | undefined;
      if (old) col.set(old.subarray(0, this.length));
      this.#cols[name] = col;
    }
  }

  push(s: Sample): void {
    if (this.length === this.#t.length) this.#alloc(Math.max(1024, this.#t.length * 2));
    const i = this.length++;
    this.#t[i] = s.t_ms;
    for (const name of FIELDS) this.#cols[name][i] = s[SERIES_FIELD[name]] as number;
  }

  /** Appends `n` scenario samples, one per millisecond from 0, straight into the
   * columns (no sample object per point). */
  fillSynthetic(scenario: Scenario, n: number): void {
    if (this.length + n > this.#t.length) this.#alloc(this.length + n + SEED_HEADROOM);
    const s = sampleAt(scenario, 0);
    for (let t = 0; t < n; t++) {
      writeSample(scenario, t, s);
      const i = this.length++;
      this.#t[i] = t;
      for (const name of FIELDS) this.#cols[name][i] = s[SERIES_FIELD[name]] as number;
    }
  }

  /** The first index whose time is `>= t` (or `> t` when `after`). */
  #search(t: number, after: boolean): number {
    let lo = 0;
    let hi = this.length;
    while (lo < hi) {
      const mid = (lo + hi) >>> 1;
      const v = this.#t[mid] as number;
      if (v < t || (after && v === t)) lo = mid + 1;
      else hi = mid;
    }
    return lo;
  }

  /** Series `name` of sample `j`; Power is VBUS × IBUS of that sample, so it takes no
   * column of its own. */
  #value(name: HistorySeries, j: number): number {
    if (name !== "power") return this.#cols[name][j] as number;
    return (this.#cols.vbus[j] as number) * (this.#cols.ibus[j] as number);
  }

  /** Min / max buckets over `t0..=t1`, at most `buckets` per series, plus the sample
   * just outside each edge that falls between samples, so lines run to the view's
   * edges (AC-9CFMD3.4). */
  buckets(
    series: readonly HistorySeries[],
    t0: number,
    t1: number,
    buckets: number,
  ): SeriesBuckets[] {
    const [from, to] = widenToNeighbours(
      this.#search(t0, false),
      this.#search(t1, true),
      this.length,
      (i) => this.#t[i] as number,
      t0,
      t1,
    );
    const n = Math.max(0, to - from);
    const per = buckets > 0 ? Math.max(1, Math.ceil(n / buckets)) : 0;
    return series.map((name) => {
      const out: SeriesBuckets = { series: name, buckets: [] };
      if (per === 0) return out;
      for (let i = from; i < to; i += per) {
        const end = Math.min(to, i + per);
        let min = Infinity;
        let max = -Infinity;
        for (let j = i; j < end; j++) {
          const v = this.#value(name, j);
          if (v < min) min = v;
          if (v > max) max = v;
        }
        out.buckets.push({ t_ms: this.#t[i] as number, min, max });
      }
      return out;
    });
  }
}

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

/** Options of a {@link MockBackend}. */
export interface MockOptions {
  /** PD records to seed when the capture first starts. */
  pdSynthetic?: number | null;
  /** A scripted record-delete failure (FR JKWT3V). */
  recordsFault?: RecordsFault | null;
  /** A scripted settings-write failure (FR JKWT3W). */
  settingsFault?: SettingsFault | null;
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
  stats: MockStats;
  pd: MockPdCapture;
  /** The VBUS a trigger request moved the demo charger to, V; null at its own. */
  triggerVbusV: number | null;
  /** This meter's open trigger session (FR DGSQ6S), null when none (FR 1HHT03). */
  trigger: MockTrigger | null;
}

interface MockRecording {
  path: string;
  rows: number;
  elapsedMs: number;
  sinceProgressMs: number;
}

/** Scripted backend behaviours chosen with `?mock=<name>` in the page URL; the device
 * list stays the same in every one but `replug`. */
export const MOCK_MODES = [
  "permission-denied",
  "rate-locked",
  "slow-export",
  "outage",
  "blip",
  "replug",
  "trigger-sag",
  "trigger-all",
  "trigger-out-of-sync",
  "trigger-req-error",
  "trigger-detect-disagree",
  "trigger-list-lag",
  "bootloader",
  "firmware-fail-switching",
  "firmware-fail-erasing",
  "firmware-fail-writing",
  "firmware-fail-verifying",
  "firmware-fail-returning",
  "firmware-other-model",
  "firmware-bad-file",
] as const;
export type MockMode = (typeof MOCK_MODES)[number];

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

/** The replug meter's picker entry at USB address `address`. */
function replugMeter(address: number): DeviceListing {
  return {
    id: `usb:1:${address}`,
    label: `KM003C — ${MOCK_REPLUG.serial}`,
    model: "KM003C",
    serial: MOCK_REPLUG.serial,
    bus: "1",
    address,
    demo_scenario: null,
    mode: "application",
  };
}

/** The status event of a mock session that just connected; a null rate is a locked one. */
function connectedStatus(serial: string, rateSps: number | null, sessionId = 1): BackendEvent {
  return {
    type: "status",
    data: {
      state: "connected",
      model: "KM003C",
      firmware_version: "2.0.6",
      bootloader_version: "2.5",
      mode: "application",
      warnings: ["outdated"],
      update_required: false,
      firmware_problems: [],
      serial,
      rate_sps: rateSps,
      rate_locked: rateSps === null,
      session_id: sessionId,
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
}

/** The queue rate cap while a trigger session is open, SPS. [inferred] */
const TRIGGER_MAX_SPS = 50;

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
};

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
};

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
  /** The PD capture saved last in the page, and the one opened (FR TGE83E). */
  #savedPd: MockPdFile | null = null;
  #openedPd: MockPdFile | null = null;
  #mode: MockMode | null;
  /** The updates run so far (FR V9SK55). */
  #firmwareFlashes: { deviceId: string; path: string; confirmation: string }[] = [];
  /** A firmware update holds the meter (FR V9SK55, AC .4). */
  #flashing = false;
  #options: MockOptions;
  #revealed: string[] = [];
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
  /** Settings reads so far (FR 1HHT01). */
  #meterSettingsReads = 0;
  /** Settings writes sent so far (FR JKWT3W). */
  #meterSettingsWrites = 0;
  /** The demo meter's settings rows, which a write changes (FR JKWT3W). */
  #meterSettings: MeterSettingField[] = DEMO_METER_SETTINGS.map((f) => ({ ...f }));
  /** Offline-record downloads so far (FR 1HHT00). */
  #recordDownloads = 0;
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

  /** `mode` defaults to the page URL's `?mock=` parameter, the PD seed to its
   * `?pdsynthetic=`. */
  constructor(
    mode: MockMode | null = mockModeFromUrl(pageSearch()),
    options: MockOptions = {
      pdSynthetic: pdSyntheticFromUrl(pageSearch()),
      recordsFault: recordsFaultFromUrl(pageSearch()),
      settingsFault: settingsFaultFromUrl(pageSearch()),
    },
  ) {
    this.#mode = mode;
    this.#options = options;
  }

  /** `replug`: the real meter's listing while the OS lists it; `bootloader`: the
   * bootloader-mode meter; else null. */
  #listedMeter(): DeviceListing | null {
    if (this.#mode === "bootloader") return BOOTLOADER_METER;
    return this.#mode === "replug" && this.#meterAddress !== null
      ? replugMeter(this.#meterAddress)
      : null;
  }

  async listDevices(): Promise<DeviceListing[]> {
    const meter = this.#listedMeter();
    const meters = meter ? [meter] : [];
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

  /** The full list now and on every change (only `replug` changes it). */
  async watchDevices(onChange: (devices: DeviceListing[]) => void): Promise<() => void> {
    this.#watchers.add(onChange);
    onChange(await this.listDevices());
    return () => this.#watchers.delete(onChange);
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
      exportSession: on(this.exportSession),
      stats: on(this.stats),
      historyRange: on(this.historyRange) as MockBackend["historyRange"],
      historyGaps: on(this.historyGaps),
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
      triggerBackTo5v: on(this.triggerBackTo5v),
      triggerClose: on(this.triggerClose),
      triggerMessageRisk: on(this.triggerMessageRisk),
      triggerMessageSend: on(this.triggerMessageSend),
      triggerConsoleRisk: on(this.triggerConsoleRisk),
      triggerConsoleSend: on(this.triggerConsoleSend),
    };
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
    if (this.#flashing) throw usage(FLASH_REFUSAL);
    if (this.#sessions.some((x) => x.id === deviceId || x.deviceId === deviceId)) {
      throw usage("This meter is already connected. Disconnect it first.");
    }
    const isMeter = this.#listedMeter()?.id === deviceId;
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
    const serial = isMeter ? MOCK_REPLUG.serial : this.#freeDemoSerial();
    const sessionId = this.#nextSessionId++;
    // Like the worker: a locked session's rate is unknown until measured.
    onEvent(connectedStatus(serial, locked ? null : rateSps, sessionId));
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
      stats: new MockStats(),
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
    s.onEvent(connectedStatus(MOCK_REPLUG.serial, s.rateSps));
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
      samples.push(sample);
      s.history.push(sample);
      s.stats.push(sample);
      s.nextMs += step;
    }
    if (s.pendingGap !== null && n > 0) {
      s.onEvent({ type: "gap", data: s.pendingGap });
      s.pendingGap = null;
    }
    // Like the backend: the CDC stream carries no temperature.
    const tempC = s.source === "cdc" ? null : 30.5 + 0.2 * Math.sin(s.nextMs / 60_000);
    s.onEvent({ type: "batch", data: { samples, temp_c: tempC, stats: s.stats.snapshot() } });
    s.streamed += n;
    const rec = s.recording;
    if (rec) {
      rec.rows += n;
      rec.elapsedMs += BATCH_MS;
      rec.sinceProgressMs += BATCH_MS;
      if (rec.sinceProgressMs >= PROGRESS_MS) {
        rec.sinceProgressMs = 0;
        s.onEvent({
          type: "recording",
          data: { state: "progress", rows: rec.rows, elapsed_ms: rec.elapsedMs },
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
      firmware_version: "2.0.6",
      serial: s.serial,
      rate_sps: s.rateSps,
      rate_locked: s.rateLocked,
      session_id: s.sessionId,
      recording: rec ? { rows: rec.rows, elapsed_ms: rec.elapsedMs } : null,
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
        data: { state: "stopped", rows, gaps: [], disconnected: true },
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

  /** There is no save dialog in the browser: the proposed name is the "path". */
  async startRecording(suggestedName: string): Promise<string | null> {
    const s = this.#open();
    if (s.recording) throw usage("A recording is already running.");
    s.recording = { path: suggestedName, rows: 0, elapsedMs: 0, sinceProgressMs: 0 };
    s.onEvent({ type: "recording", data: { state: "started" } });
    return suggestedName;
  }

  async stopRecording(): Promise<RecordingSummary> {
    const s = this.#open();
    const rec = s.recording;
    if (!rec) throw usage("Nothing is being recorded.");
    s.recording = null;
    return { rows: rec.rows, gaps: [], disconnected: false };
  }

  /** Exports every sample streamed so far, finishing on the next timer tick (`slow-export`:
   * reports half the rows at once and finishes after {@link SLOW_EXPORT_MS}). */
  async exportSession(
    suggestedName: string,
    onProgress: (progress: ExportProgress) => void,
  ): Promise<ExportResult | null> {
    const s = this.#open();
    const rows = s.streamed;
    const slow = this.#mode === "slow-export";
    if (slow) onProgress({ done: Math.floor(rows / 2), total: rows });
    return new Promise((resolve) =>
      setTimeout(
        () => {
          onProgress({ done: rows, total: rows });
          resolve({ path: suggestedName, rows });
        },
        slow ? SLOW_EXPORT_MS : 0,
      ),
    );
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

  /** Replaces the history with `n` (at most {@link MAX_SYNTHETIC_SAMPLES}) scenario
   * samples, one per millisecond from 0, and moves the live stream past them: a large
   * history for the pan check (AC-AH2AEM.4). */
  async seedSyntheticHistory(n: number): Promise<void> {
    const s = this.#open();
    const count = Math.max(0, Math.min(Math.floor(n), MAX_SYNTHETIC_SAMPLES));
    const history = new HistoryColumns(count + SEED_HEADROOM);
    history.fillSynthetic(s.scenario, count);
    s.history = history;
    s.nextMs = Math.max(s.nextMs, count);
  }

  /** Zeroes the stats; they count again from the next sample. The history stays. */
  async resetStats(): Promise<void> {
    const s = this.#open();
    s.stats = new MockStats();
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

  /** Stops `s`'s PD capture, if it runs. */
  #stopPd(s: Running): void {
    if (s.pd.timer === null) return;
    clearInterval(s.pd.timer);
    s.pd.timer = null;
    s.onEvent({ type: "pd_state", data: { capturing: false } });
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
    this.#open();
    this.#meterSettingsReads += 1;
    return { fields: this.#meterSettings.map((f) => ({ ...f })) };
  }

  /** Writes `changes` to the demo meter like the backend (FR JKWT3W): unknown or
   * read-only keys, values out of range and an empty change are refused before the
   * write; with the `mismatch` fault the meter keeps its old values and the re-read
   * names the first field that did not change. */
  async meterSettingsWrite(changes: SettingChanges): Promise<MeterSettings> {
    this.#open();
    const entries = Object.entries(changes) as [string, number][];
    if (entries.length === 0) throw usage("No setting was changed.");
    const shown = new Map<string, string>();
    for (const [key, code] of entries) {
      if (!Object.hasOwn(DEMO_WRITABLE_SETTINGS, key)) {
        throw usage(`The setting ${key} cannot be changed.`);
      }
      const text = demoSettingText(key, code);
      if (text === null) throw usage(`${key} = ${code} is out of range.`);
      shown.set(key, text);
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
      release_notes_en: "1. Demo note: improved PD capture.\n2. Demo note: minor fixes.",
    };
  }

  /** There is no open dialog: "chooses" a synthetic firmware file (FR V9SK55). */
  async firmwareChoose(): Promise<string | null> {
    return "/mock/KM003C_V2.0.7.mencrypt";
  }

  /** Plays a firmware update like the backend (FR V9SK55, AC .4): the typed confirmation
   * is re-checked first, then the meter is taken from the stream and everything else is
   * refused until the update ends; each stage goes to `onStage`, the write stage with its
   * progress. `firmware-fail-<stage>` fails at that stage with the recovery steps. */
  async firmwareFlash(
    deviceId: string,
    path: string,
    confirmation: string,
    onStage: (stage: FlashEvent) => void,
  ): Promise<FlashDone> {
    if (this.#flashing) throw usage(FLASH_REFUSAL);
    const meter = this.#listedMeter();
    const isMeter = meter?.id === deviceId;
    if (!isMeter && !scenarioOf(deviceId)) throw usage("That meter is no longer attached.");
    const info = await this.firmwareInspect(path);
    if (confirmation.trim() !== info.version) {
      throw usage(`Type the file's version, ${info.version}, to confirm the update.`);
    }
    this.#flashing = true;
    this.#firmwareFlashes.push({ deviceId, path, confirmation });
    // Like the backend: the trigger session and the stream let go of the meter first.
    const s = this.#session;
    if (s) {
      this.#closeTrigger();
      if (this.#replugTimer !== null) clearTimeout(this.#replugTimer);
      this.#replugTimer = null;
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
    try {
      const fromBootloader = isMeter && meter?.mode === "bootloader";
      for (const stage of ["switching", "erasing"] as const) {
        if (stage === "switching" && fromBootloader) continue;
        onStage({ stage });
        await sleep(MOCK_FLASH_MS[stage]);
        if (failAt === stage) throw fail(stage);
        if (stage === "switching") {
          // The prompt shows whether or not the meter restarts by itself.
          onStage({ stage: "replug", to: "bootloader", reply: "none" });
          await sleep(MOCK_FLASH_MS.replug);
        }
      }
      const total = info.size;
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
      return { outcome: "verified", version: info.version };
    } finally {
      this.#flashing = false;
    }
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

  /** Every path `revealPath` was asked to show, oldest first. */
  revealedPaths(): string[] {
    return [...this.#revealed];
  }

  // ---- Trigger panel (FR DGSQ6S): a scripted charger behind the demo meter ----

  /** Opens the trigger session on the connected demo meter. */
  async triggerOpen(): Promise<void> {
    if (this.#flashing) throw usage(FLASH_REFUSAL);
    if (!this.#session) throw usage("Connect a meter first.");
    if (this.#trigger) return;
    const s = this.#session;
    // Like the backend: the PD capture pauses and the queue runs at 50 SPS or less.
    this.#stopPd(s);
    let rateBefore: number | null = null;
    if (s.rateSps > TRIGGER_MAX_SPS && !s.recording && this.#mode !== "rate-locked") {
      rateBefore = s.rateSps;
      await this.setRate(TRIGGER_MAX_SPS);
    }
    s.trigger = { entered: null, rateBefore };
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
    this.#triggerLog.push("entry bc", "entry apple", "entry list", "reset", "pdm parm");
    t.entered = null;
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
    this.#openTrigger();
    const mv = this.#requestMv(request);
    return mv !== null && mv <= 5000 ? "at_most_5v" : "above_5v";
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

  /** Refuses an unconfirmed above-5 V request before anything is sent. */
  async triggerRequest(request: TriggerRequest, confirmed: boolean): Promise<TriggerReply> {
    this.#openTrigger();
    const mv = this.#requestMv(request);
    if (mv === null && request.protocol === "pd") {
      throw usage(`The charger offers nothing at position ${request.position}.`);
    }
    const refusal = this.#refusal(request);
    if (refusal !== null) throw usage(refusal);
    if ((await this.triggerRisk(request)) === "above_5v" && !confirmed) {
      throw usage("A request above 5 V needs your confirmation.");
    }
    const command = this.#commandText(request);
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
    this.#openTrigger().entered = request.protocol === "pd" ? "pd" : "legacy";
    if (this.#session && mv !== null) {
      // The demo VBUS follows the request; `trigger-sag` keeps it 12 % below.
      this.#session.triggerVbusV = (mv / 1000) * (this.#mode === "trigger-sag" ? 0.88 : 0.985);
    }
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

  /** Back to 5 V: `pd req=1` after PD, `reset` after the others; the session stays. */
  async triggerBackTo5v(): Promise<void> {
    this.#returnTo5v(this.#openTrigger());
  }

  /** Back to 5 V, then `pdm close`; nothing when no session is open. */
  async triggerClose(): Promise<void> {
    this.#closeTrigger();
  }

  #returnTo5v(t: MockTrigger): void {
    if (t.entered === "pd") this.#triggerLog.push("pd req=1");
    else if (t.entered === "legacy") this.#triggerLog.push("reset");
    if (this.#session) this.#session.triggerVbusV = null;
  }

  #closeTrigger(): void {
    const t = this.#trigger;
    if (!t) return;
    this.#returnTo5v(t);
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
    renderMockMessage(message);
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
      cable: discover ? { ...MOCK_CABLE } : null,
    };
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
    if (mockLineRisk(text) === "above_5v" && !confirmed) {
      throw usage("A line that can raise VBUS above 5 V needs your confirmation.");
    }
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
