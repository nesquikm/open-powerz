/**
 * FR QFTCX0: one live meter session of the web backend (AC-QFTCX0.3). It runs a
 * `MeterDriver`, keeps the session history and statistics in the browser, and sends
 * the UI the same events as the desktop backend: the connected status, a batch per
 * poll (samples, temperature, stats), rate changes and the session's end.
 */

import type {
  AttachedSession,
  BackendEvent,
  BucketsFor,
  HistoryGap,
  HistorySeries,
  ExportProgress,
  ExportResult,
  MeterCommands,
  MeterSettings,
  OfflineRecord,
  PdCleared,
  PdDetails,
  PdRow,
  RangeStats,
  RecordDeleteResult,
  RecordList,
  RecordingSummary,
  RunProgress,
  RunProgressQuery,
  Sample,
  SettingChanges,
  StatsSnapshot,
  UiError,
} from "../backend";
import { HistoryColumns, SessionStats } from "../history";
import { openBackupSink, type PickDirectory } from "./backupFolder";
import { CsvRecorder } from "./csv";
import type { MeterDriver, MeterSample, PdDecoderLike, PowerzWasm } from "./driver";
import { noWebSerialError } from "./serial";
import type { WebTransport } from "./transport";

/** Queue rates by rate code (2, 10, 50, 1000 SPS). */
export const RATES_SPS: readonly number[] = [2, 10, 50, 1000];

/** Oldest supported and newest known firmware (`powerz_protocol::firmware::version`). */
const MIN_SUPPORTED_FIRMWARE = "1.7.0";
const KNOWN_LATEST_FIRMWARE = "2.0.7";

/** How long a temperature reading stays current (native `SnapshotCadence::MAX_AGE`). */
const TEMP_MAX_AGE_MS = 2_000;

/** A jump in meter time past this many ms (or back) starts a new time base. */
const MAX_STEP_MS = 5_000;
/** How often a recording reports its rows and elapsed time to the top bar. */
const PROGRESS_MS = 500;

/** A recording in progress: its rows, file name, pause state and clock. */
interface WebRecording {
  csv: CsvRecorder;
  name: string;
  paused: boolean;
  startedAt: number;
  /** When the last progress event went out. */
  progressAt: number;
}

export function usage(message: string): UiError {
  return { kind: "usage", message, hint: null };
}

/** The queue rate code of `rateSps`; refuses a rate the meter does not offer. */
export function rateCodeOf(rateSps: number): number {
  const code = RATES_SPS.indexOf(rateSps);
  if (code < 0) throw usage(`The meter streams at 2, 10, 50 or 1000 SPS, not ${rateSps}.`);
  return code;
}

export interface WebSessionOptions {
  deviceId: string;
  sessionId: number;
  onEvent: (event: BackendEvent) => void;
  /** Called once the session ended (disconnect or failure). */
  onEnd: (session: WebSession) => void;
  /** The wasm module (its `PdDecoder` keeps the PD capture). */
  wasm: PowerzWasm;
  /** Saves a file for the user. */
  download: (name: string, data: Blob) => void;
  /** Picks the folder a record delete backs up into (FR W8SHXA); `null`: downloads. */
  pickDirectory?: PickDirectory | null;
  /** Opens the pipe a settings write runs over (FR W8SHX9): the meter's serial port,
   * or the demo meter's CDC twin. Without it, a settings change finds no Web Serial. */
  openSettingsPipe?: () => Promise<WebTransport>;
}

/** The meter commands a {@link WebSession} answers itself. */
export const SESSION_COMMANDS = [
  "stats",
  "resetStats",
  "clearHistory",
  "historyRange",
  "historyGaps",
  "historyFoldBoundary",
  "rangeStats",
  "runProgress",
  "setRate",
  "disconnect",
  "cancelWait",
  "pdStart",
  "pdStop",
  "pdDetails",
  "pdClear",
  "pdSave",
  "startRecording",
  "stopRecording",
  "pauseRecording",
  "resumeRecording",
  "saveRecordingAs",
  "exportSession",
  "meterSettings",
  "meterSettingsWrite",
  "recordsList",
  "recordDownload",
  "recordsDelete",
] as const satisfies readonly (keyof MeterCommands)[];

/** A live session: its driver, history, stats and event sink. */
export class WebSession {
  readonly deviceId: string;
  readonly sessionId: number;
  onEvent: (event: BackendEvent) => void;
  driver!: MeterDriver;

  history = new HistoryColumns();
  stats = new SessionStats();
  gaps: HistoryGap[] = [];
  ended = false;

  /** The PD capture, kept across sniffer runs until the session ends. */
  private pd: PdDecoderLike | null = null;
  private pdCapturing = false;
  private tempC: number | null = null;
  private tempAt = 0;
  private readonly startedAt = Date.now();
  /** Session time = meter time + `base`; `null` until the first sample. */
  private base: number | null = null;
  private lastMeterMs = 0;
  private lastT = -Infinity;
  /** The recording in progress (AC-QFTCX0.5): its rows, file name and pause state. */
  private recording: WebRecording | null = null;
  /** The last finished recording, kept for Save As. */
  private lastRecording: { csv: CsvRecorder; name: string } | null = null;

  constructor(private readonly o: WebSessionOptions) {
    this.deviceId = o.deviceId;
    this.sessionId = o.sessionId;
    this.onEvent = o.onEvent;
  }

  get rateLocked(): boolean {
    return this.driver.mode !== "queue";
  }

  get rateSps(): number | null {
    return this.rateLocked ? null : (RATES_SPS[this.driver.rateCode] ?? null);
  }

  /** Report the session connected (after the driver opened it). */
  announce(): void {
    const info = this.driver.info;
    this.onEvent({
      type: "status",
      data: {
        state: "connected",
        model: info.model,
        firmware_version: info.firmware_version,
        bootloader_version: "",
        mode: "application",
        warnings: [],
        update_required: null,
        firmware_problems: [],
        serial: info.serial,
        rate_sps: this.rateSps,
        rate_locked: this.rateLocked,
        basic_readings_only: this.driver.hid,
        session_id: this.sessionId,
        min_supported_firmware: MIN_SUPPORTED_FIRMWARE,
        known_latest_firmware: KNOWN_LATEST_FIRMWARE,
      },
    });
  }

  /** The driver's temperature readings. */
  onTemp(tempC: number | null): void {
    this.tempC = tempC;
    this.tempAt = Date.now();
  }

  /** The driver's samples: onto the session clock, into history and stats, out as a batch. */
  onBatch(samples: MeterSample[]): void {
    if (this.ended || samples.length === 0) return;
    const out: Sample[] = [];
    for (const m of samples) {
      const s: Sample = {
        t_ms: this.sessionMs(m.time_ms),
        vbus_v: m.vbus_v,
        ibus_a: m.ibus_a,
        cc1_v: m.cc1_v,
        cc2_v: m.cc2_v,
        dp_v: m.dp_v,
        dm_v: m.dm_v,
      };
      this.history.push(s);
      this.stats.push(s);
      out.push(s);
    }
    const fresh = this.tempC !== null && Date.now() - this.tempAt <= TEMP_MAX_AGE_MS;
    const rec = this.recording;
    if (rec && !rec.paused) {
      const now = Date.now();
      for (const s of out) rec.csv.push(s, now, fresh ? this.tempC : null);
    }
    if (rec) this.recordingProgress(rec);
    this.onEvent({
      type: "batch",
      data: { samples: out, temp_c: fresh ? this.tempC : null, stats: this.stats.snapshot() },
    });
  }

  /** Meter time `meterMs` on the session clock (ms since the session start); a meter
   * time that steps back or jumps starts a new base at the wall clock. */
  private sessionMs(meterMs: number): number {
    const step = meterMs - this.lastMeterMs;
    if (this.base === null || step < 0 || step > MAX_STEP_MS) {
      const wall = Date.now() - this.startedAt;
      this.base = Math.max(wall, this.lastT) - meterMs;
    }
    this.lastMeterMs = meterMs;
    const t = Math.max(meterMs + this.base, this.lastT);
    this.lastT = t;
    return t;
  }

  /** The driver failed: the session is over, with `error`. */
  onError(error: UiError): void {
    this.end(error);
  }

  async disconnect(): Promise<void> {
    if (this.ended) return;
    await this.driver.disconnect();
    this.end(null);
  }

  private end(error: UiError | null): void {
    if (this.ended) return;
    this.ended = true;
    if (this.recording) this.finishRecording("disconnected");
    this.pdCapturing = false;
    this.pd?.free();
    this.pd = null;
    this.onEvent({ type: "status", data: { state: "disconnected", error } });
    this.o.onEnd(this);
  }

  async setRate(rateSps: number): Promise<void> {
    const code = rateCodeOf(rateSps);
    await this.driver.setRate(code);
    this.onEvent({
      type: "status",
      data: { state: "rate", rate_sps: rateSps, rate_locked: false },
    });
  }

  /** The PD capture, made on first use. */
  private pdCapture(): PdDecoderLike {
    this.pd ??= new this.o.wasm.PdDecoder();
    return this.pd;
  }

  /** Start passive PD capture: rows go out as `pd_rows`. */
  async pdStart(): Promise<void> {
    if (this.pdCapturing) return;
    await this.driver.startPdCapture((rows) => {
      this.onEvent({ type: "pd_rows", data: { rows: rows as unknown as PdRow[] } });
    }, this.pdCapture());
    this.pdCapturing = true;
    this.onEvent({ type: "pd_state", data: { capturing: true } });
  }

  async pdStop(): Promise<void> {
    if (!this.pdCapturing) return;
    this.pdCapturing = false;
    await this.driver.stopPdCapture();
    this.onEvent({ type: "pd_state", data: { capturing: false } });
  }

  async pdDetails(index: number): Promise<PdDetails> {
    const details = this.pd?.details(BigInt(index)) as PdDetails | undefined;
    if (!details) throw usage(`PD record ${index} is no longer in the capture.`);
    return details;
  }

  async pdClear(): Promise<PdCleared> {
    return this.pdCapture().clear() as PdCleared;
  }

  /** Download the capture so far as a pdcap file named `name`. */
  async pdSave(name: string): Promise<ExportResult | null> {
    const info = this.driver.info;
    const file = this.pdCapture().pdcap(info.model, info.firmware_version) as {
      text: string;
      rows: number;
    };
    this.o.download(name, new Blob([file.text], { type: "application/x-ndjson" }));
    return { path: name, rows: file.rows };
  }

  /** Start collecting rows in memory; Stop downloads them as `suggestedName`. */
  async startRecording(suggestedName: string): Promise<string | null> {
    if (this.recording) throw usage("A recording is already running.");
    const now = Date.now();
    this.recording = {
      csv: new CsvRecorder(),
      name: suggestedName,
      paused: false,
      startedAt: now,
      progressAt: now,
    };
    this.onEvent({ type: "recording", data: { state: "started" } });
    return suggestedName;
  }

  /** Tell the top bar the rows and elapsed time, at most every `PROGRESS_MS`. */
  private recordingProgress(rec: WebRecording): void {
    const now = Date.now();
    if (now - rec.progressAt < PROGRESS_MS) return;
    rec.progressAt = now;
    this.onEvent({
      type: "recording",
      data: {
        state: "progress",
        rows: rec.csv.rows,
        elapsed_ms: now - rec.startedAt,
        paused: rec.paused,
      },
    });
  }

  async stopRecording(): Promise<RecordingSummary> {
    if (!this.recording) throw usage("No recording is running.");
    return this.finishRecording("stopped");
  }

  /** Download the recording's CSV and report it stopped. */
  private finishRecording(ended: "stopped" | "disconnected"): RecordingSummary {
    const rec = this.recording as { csv: CsvRecorder; name: string };
    this.recording = null;
    this.lastRecording = rec;
    this.o.download(rec.name, new Blob([rec.csv.text()], { type: "text/csv" }));
    const summary: RecordingSummary = {
      rows: rec.csv.rows,
      gaps: [],
      disconnected: ended === "disconnected",
      ended,
    };
    this.onEvent({ type: "recording", data: { state: "stopped", ...summary } });
    return summary;
  }

  async pauseRecording(): Promise<void> {
    if (this.recording) this.recording.paused = true;
  }

  async resumeRecording(): Promise<void> {
    if (!this.recording?.paused) return;
    this.recording.paused = false;
    // The paused stretch was not recorded: it adds no charge or energy.
    this.recording.csv.breakRun();
  }

  /** Download the finished recording again, as `path`. */
  async saveRecordingAs(path: string): Promise<string | null> {
    const rec = this.lastRecording;
    if (!rec) throw usage("There is no finished recording to save.");
    this.o.download(path, new Blob([rec.csv.text()], { type: "text/csv" }));
    return path;
  }

  /** Download the whole session history as CSV named `suggestedName`. */
  async exportSession(
    suggestedName: string,
    onProgress: (progress: ExportProgress) => void,
  ): Promise<ExportResult | null> {
    const h = this.history;
    const csv = new CsvRecorder();
    const unix0 = this.startedAt + (h.length > 0 ? h.timeAt(0) : 0);
    let prev = -Infinity;
    for (let j = 0; j < h.length; j++) {
      const s = h.sampleAt(j);
      if (this.gaps.some((g) => g.from_ms >= prev && g.to_ms <= s.t_ms)) csv.breakRun();
      csv.push(s, unix0);
      prev = s.t_ms;
    }
    this.o.download(suggestedName, new Blob([csv.text()], { type: "text/csv" }));
    onProgress({ done: csv.rows, total: csv.rows });
    return { path: suggestedName, rows: csv.rows };
  }

  attach(onEvent: (event: BackendEvent) => void): AttachedSession {
    this.onEvent = onEvent;
    const info = this.driver.info;
    return {
      device_id: this.deviceId,
      model: info.model,
      firmware_version: info.firmware_version,
      serial: info.serial,
      rate_sps: this.rateSps,
      rate_locked: this.rateLocked,
      basic_readings_only: this.driver.hid,
      session_id: this.sessionId,
      recording: null,
    };
  }

  /** The session's history and stats commands. */
  commands(): Pick<MeterCommands, (typeof SESSION_COMMANDS)[number]> {
    return {
      stats: async (): Promise<StatsSnapshot> => this.stats.snapshot(),
      resetStats: async (): Promise<void> => {
        this.stats = new SessionStats();
      },
      clearHistory: async (): Promise<void> => {
        this.history = new HistoryColumns();
        this.gaps = [];
      },
      historyRange: async <const S extends readonly HistorySeries[]>(
        series: S,
        t0Ms: number,
        t1Ms: number,
        buckets: number,
      ): Promise<BucketsFor<S>> =>
        // `HistoryColumns.buckets` maps `series` one to one, so the tuple shape holds.
        this.history.buckets(series, t0Ms, t1Ms, buckets) as BucketsFor<S>,
      historyGaps: async (t0Ms: number, t1Ms: number): Promise<HistoryGap[]> =>
        this.gaps.filter((g) => g.to_ms > t0Ms && g.from_ms < t1Ms).map((g) => ({ ...g })),
      // The browser history keeps every sample: nothing is folded.
      historyFoldBoundary: async (): Promise<number | null> => null,
      rangeStats: async (t0Ms: number, t1Ms: number): Promise<RangeStats | null> =>
        this.history.rangeStats(t0Ms, t1Ms, this.gaps),
      runProgress: async (query: RunProgressQuery): Promise<RunProgress | null> =>
        this.history.runProgress(query, this.gaps),
      setRate: (rateSps: number) => this.setRate(rateSps),
      disconnect: () => this.disconnect(),
      // Nothing waits for an unplugged meter in the browser.
      cancelWait: async (): Promise<void> => {},
      pdStart: () => this.pdStart(),
      pdStop: () => this.pdStop(),
      pdDetails: (index: number) => this.pdDetails(index),
      pdClear: () => this.pdClear(),
      pdSave: (suggestedName: string) => this.pdSave(suggestedName),
      startRecording: (suggestedName: string) => this.startRecording(suggestedName),
      stopRecording: () => this.stopRecording(),
      pauseRecording: () => this.pauseRecording(),
      resumeRecording: () => this.resumeRecording(),
      saveRecordingAs: (path: string) => this.saveRecordingAs(path),
      exportSession: (suggestedName: string, onProgress: (progress: ExportProgress) => void) =>
        this.exportSession(suggestedName, onProgress),
      meterSettings: (): Promise<MeterSettings> => this.driver.readSettings(),
      meterSettingsWrite: (changes: SettingChanges): Promise<MeterSettings> =>
        this.driver.writeSettings(
          changes,
          this.o.openSettingsPipe ?? (() => Promise.reject(noWebSerialError())),
        ),
      recordsList: (): Promise<RecordList> => this.driver.listRecords(),
      recordDownload: (index: number): Promise<OfflineRecord> => this.driver.downloadRecord(index),
      recordsDelete: async (
        target: number | "all",
        confirm: string,
      ): Promise<RecordDeleteResult> => {
        // A HID session refuses before anything is picked or sent.
        if (this.driver.hid) return this.driver.deleteRecords(target, confirm, () => {});
        // Picked first, in the user's click: every backup is a confirmed file write.
        const sink = await openBackupSink(this.o.pickDirectory ?? null, this.o.download);
        return this.driver.deleteRecords(target, confirm, sink.save, sink.label);
      },
    };
  }
}
