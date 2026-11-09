/**
 * FR QFTCWZ: the browser session driver. Opens the meter (WebUSB, or an offer to fall
 * back to WebHID) and runs the read-only session over a `WebTransport`.
 */

import type {
  CableView,
  LogoPreview,
  LogoProgress,
  LogoRead,
  LogoSettingsCheck,
  LogoWritten,
  MeterSettings,
  OfflineRecord,
  RecordDeleteResult,
  RecordList,
  SettingChanges,
  UiError,
} from "../backend";
import { ERROR_KINDS } from "../errorKinds";
import type { DemoMeterModule } from "./demoTransport";
import type { PdFilesWasm } from "./pdFiles";
import { hidOnlyBasicText } from "./features";
import { disconnectedError, notSafeError, timeoutError, type WebTransport } from "./transport";
import { openWebUsb, type UsbDeviceLike, type WebUsbOptions } from "./webusb";
import { LogoDriver, type LogoReaderLike, type LogoSource, type LogoWriteLike } from "./logoDriver";

export type { LogoReadChunk, LogoReaderLike, LogoSource, LogoWriteLike } from "./logoDriver";

/** The parts of the generated wasm module (`src/wasm/powerz_wasm.js`) the driver uses. */
export interface PowerzWasm extends DemoMeterModule, PdFilesWasm {
  /** FR EAKFEC: the desktop's chart image checks and one-page PDF writer; a PNG it
   * does not take throws a `usage` error in the desktop's words. */
  chartPdfFromPng(png: Uint8Array): Uint8Array;
  chartPngCheck(png: Uint8Array): void;
  connectRequest(id: number): Uint8Array;
  getDataRequest(id: number, mask: number): Uint8Array;
  startQueueRequest(id: number, rateCode: number): Uint8Array;
  stopQueueRequest(id: number): Uint8Array;
  startPdSnifferRequest(id: number): Uint8Array;
  stopPdSnifferRequest(id: number): Uint8Array;
  pdPollRequest(id: number, withAdc: boolean): Uint8Array;
  memoryReadRequest(id: number, addr: number, len: number): Uint8Array;
  authRequest(
    id: number,
    level: number,
    block: Uint8Array,
    nowMs: bigint,
    random: bigint,
  ): Uint8Array;
  decodeAccept(reply: Uint8Array): void;
  decodeAdc(reply: Uint8Array): Omit<MeterSample, "time_ms">;
  decodeQueue(reply: Uint8Array, rateCode: number): MeterSample[];
  /** A recorder CSV's rows, read with the desktop's reader and row rules (FR AHTZW3);
   * a file that is no recording throws a `usage` error in the desktop's words. */
  readRecordingCsv(bytes: Uint8Array): RecordingRows;
  decodeAuthReply(reply: Uint8Array, block: Uint8Array, nowMs: bigint, random: bigint): number;
  decodeMemoryRead(reply: Uint8Array, addr: number, len: number): Uint8Array;
  decodeDeviceInfo(
    hardware: Uint8Array,
    firmware: Uint8Array,
    otp: Uint8Array,
    uid: Uint8Array,
  ): MeterInfo;
  PdDecoder: new () => PdDecoderLike;
  /** GET_DATA SETTINGS (FR W8SHX8). */
  settingsRequest(id: number): Uint8Array;
  /** The settings panel rows of a SETTINGS reply, as the desktop app shows them. */
  decodeSettings(reply: Uint8Array): MeterSettings;
  /** The offline records book: the latest list and its readable windows (FR W8SHX8). */
  RecordBook: new () => RecordBookLike;
  /** One settings write between two reads and a read-back (FR W8SHX9); the
   * constructor refuses bad changes as `usage`. */
  SettingsWrite: new (changes: Record<string, number>) => SettingsWriteLike;
  /** Length of the first complete CDC frame in `buf`, `undefined` while incomplete. */
  cdcFrameLength(buf: Uint8Array): number | undefined;
  /** FR W8SHXB: the preview of a picture file's bytes (a raw logo file when `isBin`),
   * as the desktop app's; no picture throws `usage`. */
  logoPreview(bytes: Uint8Array, isBin: boolean, alpha: boolean): LogoPreview;
  /** The preview of a built-in logo. */
  logoPreviewBuiltin(name: string): LogoPreview;
  /** One upload or Remove over the native logo paths (FR W8SHXB). */
  LogoWrite: {
    upload(bytes: Uint8Array, isBin: boolean, alpha: boolean): LogoWriteLike;
    uploadBuiltin(name: string): LogoWriteLike;
    remove(): LogoWriteLike;
  };
  /** A read of the logo area: the head, and with `full` the whole logo. */
  LogoReader: new (full: boolean) => LogoReaderLike;
  /** One settings check against a logo write's calibration snapshot. */
  settingsSaveCheck(snapshot: Uint8Array, settingsReply: Uint8Array): LogoSettingsCheck;
  /** FR AHTZW2: the `.db` export's table name, SQL text and rows, as the desktop's. */
  dbTableName(month: number, day: number, hour: number, minute: number, second: number): string;
  dbCreateSql(table: string): string;
  dbInsertParamSql(table: string): string;
  dbInsertRowSql(table: string): string;
  dbParamValues(table: string, startUnixMs: number, localStart: string, rateSps: number): string[];
  DbRows: new (startUnixMs: number) => DbRowsLike;
  /** FR AHTZW3: the desktop's SQL and rules that read a vendor `.db` recording back. */
  dbTableNamesSql(): string;
  dbTableColumnsSql(): string;
  dbSelectRowsSql(table: string): string;
  dbRowTotalSql(table: string): string;
  /** The data table among `tables` (`[name, plain]`), or `undefined` when none is one;
   * throws a `usage` error in the desktop's words. */
  dbPickDataTable(
    tables: [string, boolean][],
    columnsOf: (table: string) => string[],
  ): string | undefined;
  DbRecording: new () => DbRecordingLike;
}

/** The wasm `DbRecording` (FR AHTZW3): a `.db` data table's rows, by the desktop's rules. */
export interface DbRecordingLike {
  /** One result row: its ElapsedTime text and ten numbers (NaN = NULL); true when a
   * progress report is due. */
  push(elapsed: string | undefined, numbers: Float64Array): boolean;
  lines(): number;
  /** The rows kept; consumes the reader. Throws a `usage` error when none was. */
  finish(): RecordingRows;
  free(): void;
}

/** One `.db` data row: `ElapsedTime`, then the other ten columns (`null` = NULL). */
export interface DbRowValues {
  elapsed: string;
  numbers: (number | null)[];
}

/** The wasm `DbRows` (FR AHTZW2): samples to `.db` rows. */
export interface DbRowsLike {
  push(
    timeMs: number,
    readings: Float64Array,
    tempC: number | undefined,
    newRun: boolean,
  ): DbRowValues;
  free(): void;
}

/** The wasm `SettingsWrite` (FR W8SHX9). */
export interface SettingsWriteLike {
  readRequest(id: number): Uint8Array;
  takeRead(reply: Uint8Array): void;
  writeRequest(id: number): Uint8Array;
  /** The write's reply, `undefined` when none came in time. */
  takeWriteReply(reply: Uint8Array | undefined): void;
  result(): MeterSettings;
  free(): void;
}

/** The wasm `RecordBook` (FR W8SHX8). */
export interface RecordBookLike {
  listRequest(id: number): Uint8Array;
  decodeList(reply: Uint8Array): RecordList;
  /** The reads of record `index` (1-based): its own listed window, ≤ 256 bytes each. */
  chunks(index: number): { addr: number; len: number }[];
  /** One memory read inside a listed record window; throws `forbidden` otherwise. */
  chunkRequest(id: number, addr: number, len: number): Uint8Array;
  /** Record `index` decoded from its window's bytes. */
  decodeRecord(index: number, data: Uint8Array): OfflineRecord;
  /** FR W8SHXA: the 1-based records a delete removes over the latest list
   * (`undefined` = every record); throws `usage` on a wrong confirmation, an unlisted
   * record or a table too long to write. */
  deleteTargets(index: number | undefined, confirm: string): Uint32Array | number[];
  /** Record `index`'s desktop backup file from its window's bytes. */
  backupFile(index: number, data: Uint8Array): BackupFile;
  /** The SET_DATA 0x48 att 0x28 frame keeping every listed record but `indices`. */
  deleteRequest(id: number, indices: Uint32Array): Uint8Array;
  /** ACCEPT / FINISHED pass; a REJECT or other reply throws. */
  takeDeleteReply(reply: Uint8Array): void;
  /** After a fresh list: throws unless it equals the kept entries. */
  checkAfterDelete(): void;
  free(): void;
}

/** A record's backup file as the desktop worker names and writes it. */
export interface BackupFile {
  name: string;
  csv: string;
}

/** Where the web edition's record backups go: the browser picks the folder. */
const BACKUP_DIR = "your browser's downloads";

/** The wasm `PdDecoder`: poll replies into rows, and the capture kept since the last
 * clear (details, clear, pdcap file). */
export interface PdDecoderLike {
  decode(reply: Uint8Array): unknown;
  details(index: number | bigint): unknown;
  clear(): unknown;
  pdcap(model: string, firmwareVersion: string): unknown;
  /** The newest cable identity since the last clear (FR EAKFEC); `null` for none. */
  latestCable(): CableView | null;
  free(): void;
}

/** What opening a WebUSB device yields: a transport, or an offer to use WebHID instead. */
export type UsbOpenResult = { transport: WebTransport } | { offerHid: true; error: UiError };

function isUiError(e: unknown): e is UiError {
  return typeof e === "object" && e !== null && "kind" in e && "message" in e;
}

/** A failure the app already words for the user (a transport error, or a wasm
 * refusal of a kind the UI knows), as opposed to a wasm decode error. */
function isWorded(e: unknown): e is UiError {
  return isUiError(e) && (ERROR_KINDS as readonly string[]).includes(e.kind);
}

/**
 * Open `device` over its vendor bulk interface. When the interface cannot be claimed
 * (an OS driver holds it), resolve with an offer to switch to the WebHID transport,
 * which carries ADC snapshots only (GET_DATA 0x001; docs/protocol.md "HID limits").
 * Other failures reject.
 */
export async function openUsbOrOfferHid(
  device: UsbDeviceLike,
  options: WebUsbOptions = {},
): Promise<UsbOpenResult> {
  try {
    return { transport: await openWebUsb(device, options) };
  } catch (e) {
    if (isUiError(e) && e.kind === "device_busy") return { offerHid: true, error: e };
    throw e;
  }
}

// ---- the read-only session (AC-QFTCWZ.4) ----

/** GET_DATA attribute bits the session asks for. */
const ATT_ADC = 1 << 0;
const ATT_ADC_QUEUE = 1 << 1;

/** The four safe memory blocks, as the native `DeviceInfo` read takes them. */
const HARDWARE_INFO = { addr: 0x0000_0420, len: 64 } as const;
const FIRMWARE_INFO = { addr: 0x0000_4420, len: 64 } as const;
const OTP = { addr: 0x0300_0c00, len: 64 } as const;
const UID = { addr: 0x4001_0450, len: 12 } as const;

/** Reply timeouts, as `transport::default_timeout` (CONNECT 2 s, others 1 s). */
const CONNECT_TIMEOUT_MS = 2_000;
/** The first CONNECT's wait (native `FIRST_CONNECT_TIMEOUT`); a miss recovers once. */
const FIRST_CONNECT_TIMEOUT_MS = 500;
const REPLY_TIMEOUT_MS = 1_000;
/** Mismatched-id replies discarded before a request fails (native `MAX_STALE_REPLIES`). */
const MAX_STALE_REPLIES = 3;
/** A memory read's confirmation, before its encrypted data. */
const MEMORY_CONFIRMATION_LEN = 20;
/** Rate code of 1000 SPS, polled faster (native `Streamer::poll_period`). */
const RATE_1000_SPS = 3;
/** ADC snapshot period without the queue `[inferred]`: the meter refreshes its ADC
 * block at its own rate, so a 100 ms poll keeps up with it. */
const SNAPSHOT_PERIOD_MS = 100;
/** Least time between two ADC snapshots while the queue streams, for the chip
 * temperature (native `SnapshotCadence::PERIOD`). */
const QUEUE_SNAPSHOT_PERIOD_MS = 500;
/** PD sniffer poll period (native `PdSniffer::POLL_PERIOD`). */
const PD_POLL_PERIOD_MS = 5;
/** Each PD poll also asks for the ADC snapshot (the sniffer's `with_adc` option). */
const PD_POLL_WITH_ADC = true;

/** What a HID session refuses for every logo command. */
const LOGO = "The startup logo";

/** Queue poll period for `rateCode`: 20 ms at 1000 SPS, 100 ms otherwise. */
function queuePollPeriodMs(rateCode: number): number {
  return rateCode === RATE_1000_SPS ? 20 : 100;
}

/** Model, versions and identity of the meter (the wasm `DeviceInfoView`). */
export interface MeterInfo {
  model: string;
  firmware_version: string;
  hardware_version: string;
  serial: string;
  uid: string;
}

/** One measurement: a queue record, or an ADC snapshot on the session clock. */
export interface MeterSample {
  time_ms: number;
  vbus_v: number;
  ibus_a: number;
  cc1_v: number;
  cc2_v: number;
  dp_v: number;
  dm_v: number;
}

/** The rows a recording file opens with (FR AHTZW3), times the file's elapsed ms. */
export interface RecordingRows {
  rows: number;
  skipped: number;
  left_out: number;
  /** The file's own last CHARGE / ENERGY, when it has one. */
  charge_ah?: number | null;
  energy_wh?: number | null;
  samples: MeterSample[];
}

/** One PD table row as the wasm `PdDecoder` gives it. */
export type PdRowView = { name: string } & Record<string, unknown>;

/** How the session measures: the ADC queue, or ADC snapshots when auth failed. */
export type MeterMode = "queue" | "snapshot";

export interface MeterDriverOptions {
  wasm: PowerzWasm;
  transport: WebTransport;
  /** Queue rate code 0..3 (2 SPS, 10 SPS, 50 SPS, 1000 SPS). */
  rateCode: number;
  onBatch: (samples: MeterSample[]) => void;
  /** Each ADC snapshot's chip temperature, °C (`null` when the meter reports none);
   * while the queue streams, a snapshot follows a poll at most every 500 ms. */
  onTemp?: (tempC: number | null) => void;
  /** The session ended on a transfer failure; called at most once. */
  onError: (error: UiError) => void;
  /** Random source for the auth nonce; defaults to `crypto.getRandomValues`. */
  random?: () => bigint;
  /** A WebHID session (AC-QFTCWZ.3): ADC snapshots only, so no CONNECT, device info,
   * auth, queue or PD sniffer; every frame is GET_DATA 0x001. */
  hid?: boolean;
  /** The model the device list shows, for a HID session (it reads no device info). */
  model?: string;
}

/** Any failure as a `UiError`: transport errors pass through, wasm decode errors are
 * `protocol`, DOM transfer errors `disconnected` / `timeout`. */
function asUiError(e: unknown): UiError {
  if (isUiError(e) && (ERROR_KINDS as readonly string[]).includes(e.kind)) {
    return { kind: e.kind, message: e.message, hint: e.hint ?? null };
  }
  if (typeof DOMException !== "undefined" && e instanceof DOMException) {
    return e.name === "TimeoutError" ? timeoutError() : disconnectedError();
  }
  if (isUiError(e)) return { kind: "protocol", message: String(e.message), hint: null };
  return { kind: "internal", message: "Something went wrong inside the app.", hint: null };
}

/** What a HID session refuses: anything beyond the ADC snapshot. */
function hidOnlyBasic(what: string): UiError {
  return {
    kind: "usage",
    message: hidOnlyBasicText(what),
    hint: "Free the meter's USB interface and connect over WebUSB for this.",
  };
}

/** A wasm `BindError` from the records book as the desktop app words it: `forbidden`
 * is an unsafe request, `usage` keeps its message. */
function bookError(e: unknown): unknown {
  if (typeof e !== "object" || e === null) return e;
  const { kind, message, hint } = e as { kind?: unknown; message?: unknown; hint?: unknown };
  if (kind === "forbidden") return notSafeError();
  if (kind === "usage") {
    return {
      kind: "usage",
      message: String(message),
      hint: typeof hint === "string" ? hint : null,
    };
  }
  return e;
}

/** Run a records-book call, rewording its refusals as the desktop app does. */
function callBook<T>(call: () => T): T {
  try {
    return call();
  } catch (e) {
    throw bookError(e);
  }
}

/** A failure before the table write, as the desktop worker words it: the cause's
 * kind and hint (a meter unplugged mid-backup stays `disconnected`), its message
 * after the prefix. */
function backupFailed(e: unknown): UiError {
  const ui = asUiError(e);
  const reason = ui.kind === "internal" && e instanceof Error && e.message ? e.message : ui.message;
  return {
    ...ui,
    message: `The backup download failed, so nothing was deleted. ${reason}`,
  };
}

/** `parts` joined into one array. */
function concat(parts: Uint8Array[]): Uint8Array {
  const joined = new Uint8Array(parts.reduce((n, part) => n + part.length, 0));
  let at = 0;
  for (const part of parts) {
    joined.set(part, at);
    at += part.length;
  }
  return joined;
}

function defaultRandom(): bigint {
  const words = new Uint32Array(2);
  crypto.getRandomValues(words);
  return (BigInt(words[0] ?? 0) << 32n) | BigInt(words[1] ?? 0);
}

/** The read-only meter session: connect, device info, auth, queue (or snapshot)
 * streaming and passive PD capture, one request in flight at a time. */
export class MeterDriver {
  info!: MeterInfo;
  authLevel = 0;
  mode: MeterMode = "snapshot";

  private id = 0;
  private lock: Promise<unknown> = Promise.resolve();
  private ended = false;
  private pollTimer: ReturnType<typeof setTimeout> | null = null;
  private pdTimer: ReturnType<typeof setTimeout> | null = null;
  private pd: { decoder: PdDecoderLike; owned: boolean } | null = null;
  private readonly startedAt = Date.now();
  private lastSnapshotMs: number | null = null;
  private rate: number;
  /** Holds on the stream's polling (a trigger session's automatic list); polls skip
   * while any is held (native `Worker::hold_polling`). */
  private pollHolds = 0;
  /** The UID block read at open, kept for a later level-1 auth. */
  private uidBlock: Uint8Array | null = null;
  /** The offline records book, made on the first list and freed when the session ends. */
  private book: RecordBookLike | null = null;
  /** The last SETTINGS reply read in this session (a logo write's precondition). */
  private settingsReply: Uint8Array | null = null;
  /** The startup logo's sequencing (FR W8SHXB), run while holding the link. */
  private readonly logo: LogoDriver;

  private constructor(private readonly o: MeterDriverOptions) {
    this.rate = o.rateCode;
    this.logo = new LogoDriver({
      wasm: o.wasm,
      nextId: () => this.nextId(),
      exchange: (frame, timeoutMs = REPLY_TIMEOUT_MS) => this.exchange(frame, timeoutMs),
      memoryRead: (request, addr, len) => this.memoryRead(request, addr, len),
      ended: () => this.ended,
      signIn: () => this.signInLocked(),
      readInfo: async () => {
        await this.readInfoLocked();
        return this.info;
      },
      authLevel: () => this.authLevel,
      settingsReply: () => this.settingsReply ?? undefined,
      errorMessage: (e) => asUiError(e).message,
    });
  }

  /** The queue rate code the session streams at. */
  get rateCode(): number {
    return this.rate;
  }

  /** A WebHID session: basic readings (ADC snapshots) only. */
  get hid(): boolean {
    return this.o.hid === true;
  }

  /** Connect, read the device info, authenticate at level 1 and start the queue at
   * `rateCode`; when the meter refuses the auth or the queue, poll ADC snapshots. */
  static async connect(options: MeterDriverOptions): Promise<MeterDriver> {
    const driver = new MeterDriver(options);
    if (options.hid) {
      driver.info = {
        model: options.model ?? "KM003C",
        firmware_version: "",
        hardware_version: "",
        serial: "",
        uid: "",
      };
      driver.schedulePoll(0);
      return driver;
    }
    try {
      await driver.open();
    } catch (e) {
      // A failed sign-in frees the meter: close the transport before rejecting.
      await options.transport.close().catch(() => {});
      throw asUiError(e);
    }
    driver.schedulePoll(0);
    return driver;
  }

  private nextId(): number {
    this.id = (this.id + 1) & 0xff;
    return this.id;
  }

  /** Send `frame` and wait for the reply carrying its id (auth replies carry none). */
  private async exchange(
    frame: Uint8Array,
    timeoutMs: number,
    matchId = true,
  ): Promise<Uint8Array> {
    const t = this.o.transport;
    let reply = await t.transact(frame, timeoutMs);
    if (!matchId) return reply;
    for (let rereads = 0; reply[1] !== frame[1]; rereads++) {
      if (rereads === MAX_STALE_REPLIES) {
        throw {
          kind: "protocol",
          message: `No reply with the request's id after ${MAX_STALE_REPLIES} re-reads.`,
          hint: null,
        } satisfies UiError;
      }
      reply = await t.read(timeoutMs);
    }
    return reply;
  }

  /** FR CJ3QCA: send `frame` over the settings pipe and wait for the reply with its
   * id, skipping replies to earlier requests (left on a reused serial port) until
   * `timeoutMs` runs out instead of counting them; then the `timeout` error. */
  private async pipeExchange(
    pipe: WebTransport,
    frame: Uint8Array,
    timeoutMs: number,
  ): Promise<Uint8Array> {
    const deadline = Date.now() + timeoutMs;
    let reply = await pipe.transact(frame, timeoutMs);
    while (reply[1] !== frame[1]) {
      const left = deadline - Date.now();
      if (left <= 0) throw timeoutError();
      reply = await pipe.read(left);
    }
    return reply;
  }

  /** Run `work` with the link to itself (one request in flight). */
  private exclusive<T>(work: () => Promise<T>): Promise<T> {
    const run = this.lock.then(work, work);
    this.lock = run.catch(() => {});
    return run;
  }

  private async expectAccept(frame: Uint8Array, timeoutMs = REPLY_TIMEOUT_MS): Promise<void> {
    this.o.wasm.decodeAccept(await this.exchange(frame, timeoutMs));
  }

  /** CONNECT as the native `connect_checked`: a 500 ms try; on no matching reply,
   * recover the transport once and send one more CONNECT that waits 2000 ms. */
  private async connectChecked(): Promise<void> {
    const w = this.o.wasm;
    const t = this.o.transport;
    let reply: Uint8Array;
    try {
      reply = await this.exchange(w.connectRequest(this.nextId()), FIRST_CONNECT_TIMEOUT_MS);
    } catch (e) {
      // No reply, or only replies with another id: the pipe may be stuck.
      const kind = asUiError(e).kind;
      if (kind !== "timeout" && kind !== "protocol") throw e;
      // A recovery that fails says less than the CONNECT that failed.
      if (t.recover) await t.recover().catch(() => Promise.reject(e));
      reply = await this.exchange(w.connectRequest(this.nextId()), CONNECT_TIMEOUT_MS);
    }
    w.decodeAccept(reply);
  }

  private readBlock(block: { addr: number; len: number }): Promise<Uint8Array> {
    return this.memoryRead(
      this.o.wasm.memoryReadRequest(this.nextId(), block.addr, block.len),
      block.addr,
      block.len,
    );
  }

  /** Send a memory read `request` of `len` bytes at `addr` and decode its reply. */
  private async memoryRead(request: Uint8Array, addr: number, len: number): Promise<Uint8Array> {
    const w = this.o.wasm;
    const reply = await this.exchange(request, REPLY_TIMEOUT_MS);
    // The real meter sends the 20-byte confirmation alone and the encrypted data
    // (rounded up to 16 bytes) in further IN transfers; the demo meter joins them.
    // A shorter reply (a REJECT) goes straight to the decoder.
    const want = MEMORY_CONFIRMATION_LEN + Math.ceil(len / 16) * 16;
    if (reply.length < MEMORY_CONFIRMATION_LEN || reply.length >= want) {
      return w.decodeMemoryRead(reply, addr, len);
    }
    const parts: Uint8Array[] = [reply];
    let have = reply.length;
    while (have < want) {
      const part = await this.o.transport.read(REPLY_TIMEOUT_MS);
      // An empty transfer would never fill the block: stop rather than read forever.
      if (part.length === 0) {
        throw {
          kind: "protocol",
          message: `The memory read at 0x${addr.toString(16)} ended early.`,
          hint: null,
        } satisfies UiError;
      }
      parts.push(part);
      have += part.length;
    }
    return w.decodeMemoryRead(concat(parts), addr, len);
  }

  /** The device info read (the four safe blocks), while holding the link; returns the
   * UID block the auth signs. */
  private async readInfoLocked(): Promise<Uint8Array> {
    const hardware = await this.readBlock(HARDWARE_INFO);
    const firmware = await this.readBlock(FIRMWARE_INFO);
    const otp = await this.readBlock(OTP);
    const uid = await this.readBlock(UID);
    this.uidBlock = uid;
    this.info = this.o.wasm.decodeDeviceInfo(hardware, firmware, otp, uid);
    return uid;
  }

  private async open(): Promise<void> {
    await this.connectChecked();
    if (await this.authenticate(await this.readInfoLocked())) {
      this.mode = (await this.startQueue()) ? "queue" : "snapshot";
    } else {
      this.mode = "snapshot";
    }
  }

  /** Level-1 auth over the UID; `false` when the meter does not grant it. A
   * disconnect still fails the connect. */
  private async authenticate(uid: Uint8Array): Promise<boolean> {
    const w = this.o.wasm;
    const nowMs = BigInt(Date.now());
    const random = (this.o.random ?? defaultRandom)();
    try {
      const request = w.authRequest(this.nextId(), 1, uid, nowMs, random);
      const reply = await this.exchange(request, REPLY_TIMEOUT_MS, false);
      this.authLevel = w.decodeAuthReply(reply, uid, nowMs, random);
      return this.authLevel > 0;
    } catch (e) {
      if (asUiError(e).kind === "disconnected") throw e;
      this.authLevel = 0;
      return false;
    }
  }

  /** Start the queue; a queue left running by an earlier session is stopped and
   * started again once. `false` when the meter refuses it. */
  private async startQueue(): Promise<boolean> {
    const w = this.o.wasm;
    const start = () => this.expectAccept(w.startQueueRequest(this.nextId(), this.rate));
    try {
      await start();
      return true;
    } catch (e) {
      if (isWorded(e)) throw e;
    }
    try {
      await this.expectAccept(w.stopQueueRequest(this.nextId()));
      await start();
      return true;
    } catch (e) {
      if (isWorded(e)) throw e;
      return false;
    }
  }

  private periodMs(): number {
    return this.mode === "queue" ? queuePollPeriodMs(this.rate) : SNAPSHOT_PERIOD_MS;
  }

  private schedulePoll(delayMs: number): void {
    if (this.ended) return;
    this.pollTimer = setTimeout(() => {
      this.pollTimer = null;
      void this.pollOnce();
    }, delayMs);
  }

  /** Hold (`true`) or release (`false`) the stream's polling: while held, no poll
   * asks the meter; the last release polls at once. A release with no hold is a no-op. */
  holdPolling(hold: boolean): void {
    if (hold) {
      this.pollHolds += 1;
      return;
    }
    if (this.pollHolds === 0) return;
    this.pollHolds -= 1;
    if (this.pollHolds === 0 && this.pollTimer !== null && !this.ended) {
      clearTimeout(this.pollTimer);
      this.schedulePoll(1);
    }
  }

  /** Whether the stream's polling is held. */
  get pollingHeld(): boolean {
    return this.pollHolds > 0;
  }

  private async pollOnce(): Promise<void> {
    if (this.ended) return;
    if (this.pollHolds > 0) {
      this.schedulePoll(this.periodMs());
      return;
    }
    const began = Date.now();
    try {
      const samples = await this.exclusive(() => this.readSamples());
      if (this.ended) return;
      if (samples.length > 0) this.o.onBatch(samples);
    } catch (e) {
      this.fail(e);
      return;
    }
    this.schedulePoll(Math.max(1, began + this.periodMs() - Date.now()));
  }

  private async readSamples(): Promise<MeterSample[]> {
    if (this.ended) return [];
    const w = this.o.wasm;
    if (this.mode === "queue") {
      const reply = await this.exchange(
        w.getDataRequest(this.nextId(), ATT_ADC_QUEUE),
        REPLY_TIMEOUT_MS,
      );
      const samples = w.decodeQueue(reply, this.rate);
      await this.snapshotIfDue();
      return samples;
    }
    const adc = await this.readAdc();
    return [
      {
        time_ms: Date.now() - this.startedAt,
        vbus_v: adc.vbus_v,
        ibus_a: adc.ibus_a,
        cc1_v: adc.cc1_v,
        cc2_v: adc.cc2_v,
        dp_v: adc.dp_v,
        dm_v: adc.dm_v,
      },
    ];
  }

  /** One ADC snapshot; its temperature goes to `onTemp`. */
  private async readAdc(): Promise<Omit<MeterSample, "time_ms">> {
    const w = this.o.wasm;
    const reply = await this.exchange(w.getDataRequest(this.nextId(), ATT_ADC), REPLY_TIMEOUT_MS);
    const adc = w.decodeAdc(reply) as Omit<MeterSample, "time_ms"> & { temp_c?: number | null };
    this.o.onTemp?.(typeof adc.temp_c === "number" ? adc.temp_c : null);
    return adc;
  }

  /** While the queue streams: an ADC snapshot for the temperature once one is due. A
   * failed snapshot costs only the temperature; a disconnect still ends the session. */
  private async snapshotIfDue(): Promise<void> {
    if (!this.o.onTemp) return;
    const now = Date.now();
    if (this.lastSnapshotMs !== null && now - this.lastSnapshotMs < QUEUE_SNAPSHOT_PERIOD_MS) {
      return;
    }
    this.lastSnapshotMs = now;
    try {
      await this.readAdc();
    } catch (e) {
      if (asUiError(e).kind === "disconnected") throw e;
    }
  }

  /** Restart the queue at `rateCode`: stop it, then start it at the new rate. Refused
   * without the queue (the meter declined the auth). */
  async setRate(rateCode: number): Promise<void> {
    if (this.ended) throw disconnectedError();
    if (this.o.hid) throw hidOnlyBasic("Changing the rate");
    if (this.mode !== "queue") {
      throw {
        kind: "usage",
        message: "Changing the rate needs authentication, which the meter refused.",
        hint: null,
      } satisfies UiError;
    }
    const w = this.o.wasm;
    try {
      await this.exclusive(async () => {
        await this.expectAccept(w.stopQueueRequest(this.nextId()));
        this.rate = rateCode;
        await this.expectAccept(w.startQueueRequest(this.nextId(), rateCode));
      });
    } catch (e) {
      throw asUiError(e);
    }
  }

  /** Read the meter's settings (AC-W8SHX8.1): one GET_DATA SETTINGS between polls,
   * decoded by the wasm core into the desktop panel's rows. */
  readSettings(): Promise<MeterSettings> {
    const w = this.o.wasm;
    return this.meterRead("Reading the settings", async () => {
      const reply = await this.exchange(w.settingsRequest(this.nextId()), REPLY_TIMEOUT_MS);
      this.settingsReply = reply;
      return w.decodeSettings(reply);
    });
  }

  /** Change the meter's settings (FR W8SHX9) as the desktop app does: over the
   * settings pipe `openPipe` gives (the meter's serial port), read SETTINGS twice, send
   * one type-0x0A write built by the wasm core and read SETTINGS again. Bad changes are
   * refused before the pipe opens; the stream waits meanwhile, and a queue stream
   * restarts at the session's rate after a change that took (AC-W8SHX9.5). */
  async writeSettings(
    changes: SettingChanges,
    openPipe: () => Promise<WebTransport>,
  ): Promise<MeterSettings> {
    if (this.o.hid) throw hidOnlyBasic("Changing the meter's settings");
    if (this.ended) throw disconnectedError();
    let write: SettingsWriteLike;
    try {
      write = new this.o.wasm.SettingsWrite(changes as Record<string, number>);
    } catch (e) {
      throw asUiError(e);
    }
    let written: MeterSettings;
    try {
      // Open the pipe before waiting for the link: picking a serial port needs the
      // user's click, whose activation must not wait behind an in-flight poll.
      const pipe = await openPipe();
      try {
        written = await this.exclusive(async () => {
          if (this.ended) throw disconnectedError();
          return await this.settingsSequence(pipe, write);
        });
      } finally {
        await pipe.close().catch(() => {});
      }
    } catch (e) {
      throw asUiError(e);
    } finally {
      write.free();
    }
    await this.restartQueueAfterChange();
    return written;
  }

  /** Read, read, write, read over `pipe`; no reply to the write leaves it to the
   * read-back. */
  private async settingsSequence(
    pipe: WebTransport,
    write: SettingsWriteLike,
  ): Promise<MeterSettings> {
    const read = async () =>
      write.takeRead(
        await this.pipeExchange(pipe, write.readRequest(this.nextId()), REPLY_TIMEOUT_MS),
      );
    await read();
    await read();
    const request = write.writeRequest(this.nextId());
    let reply: Uint8Array | undefined;
    try {
      reply = await this.pipeExchange(pipe, request, REPLY_TIMEOUT_MS);
    } catch (e) {
      if (asUiError(e).kind !== "timeout") throw e;
    }
    write.takeWriteReply(reply);
    await read();
    return write.result();
  }

  /** After a settings write the meter's queue goes back to its own rate `[inferred]`:
   * stop and start a queue stream again at the session's rate (native
   * `restart_after_settings_write`). A failed restart does not fail the write. */
  private async restartQueueAfterChange(): Promise<void> {
    if (this.mode !== "queue" || this.ended) return;
    const w = this.o.wasm;
    await this.exclusive(async () => {
      if (this.ended) return;
      await this.expectAccept(w.stopQueueRequest(this.nextId()));
      await this.expectAccept(w.startQueueRequest(this.nextId(), this.rate));
    }).catch(() => {});
  }

  /** List the meter's offline records (AC-W8SHX8.2): a level-1 auth first when the
   * session is below it, then one GET_DATA RECORD_LIST decoded by the wasm core into
   * the desktop Records view's list. An empty list is not an error. */
  listRecords(): Promise<RecordList> {
    return this.meterRead(
      "Reading the offline records",
      async () => (await this.listLocked()).list,
    );
  }

  /** A safe read (FR W8SHX8) that a HID session refuses with `what`: `run` holds the
   * link once the session is known live, and any failure leaves as a `UiError`. */
  private async meterRead<T>(what: string, run: () => Promise<T>): Promise<T> {
    if (this.o.hid) throw hidOnlyBasic(what);
    if (this.ended) throw disconnectedError();
    try {
      return await this.exclusive(async () => {
        if (this.ended) throw disconnectedError();
        return await run();
      });
    } catch (e) {
      throw asUiError(e);
    }
  }

  /** A level-1 auth unless the session already holds it, while holding the link;
   * `fresh` signs in again even then (FR CJ3QCB: the record list). */
  private async signInLocked(fresh = false): Promise<void> {
    if (!fresh && this.authLevel >= 1) return;
    if (!(this.uidBlock && (await this.authenticate(this.uidBlock)))) {
      throw {
        kind: "protocol",
        message: "The meter did not accept the app's sign-in.",
        hint: "Replug the meter and try again.",
      } satisfies UiError;
    }
  }

  /** The record list, read while holding the link; the book keeps its windows. */
  private async listLocked(): Promise<{ book: RecordBookLike; list: RecordList }> {
    const w = this.o.wasm;
    // AC-CJ3QCB.1: the meter answers the list from its last sign-in, so sign in afresh.
    await this.signInLocked(true);
    // The session may have ended during the sign-in: make no book it would not free.
    if (this.ended) throw disconnectedError();
    const book = (this.book ??= new w.RecordBook());
    const reply = await this.exchange(book.listRequest(this.nextId()), REPLY_TIMEOUT_MS);
    // The session may have ended (and freed the book) while the reply was in flight.
    if (this.ended) throw disconnectedError();
    return { book, list: book.decodeList(reply) };
  }

  /** Download offline record `index` (1-based, AC-W8SHX8.3) as the desktop worker
   * does: a fresh record list, then that record's own listed window in memory reads of
   * at most 256 bytes. An unlisted or empty record, or a read outside the listed
   * windows, is refused before it is sent. */
  downloadRecord(index: number): Promise<OfflineRecord> {
    return this.meterRead("Reading the offline records", async () => {
      const { book } = await this.listLocked();
      if (this.ended) throw disconnectedError();
      const data = await this.recordWindowLocked(book, index);
      return callBook(() => book.decodeRecord(index, data));
    });
  }

  /** Record `index`'s listed window, read in the wasm core's chunks (memory reads of
   * at most 256 bytes) and joined. */
  private async recordWindowLocked(book: RecordBookLike, index: number): Promise<Uint8Array> {
    const parts: Uint8Array[] = [];
    for (const c of callBook(() => book.chunks(index))) {
      const request = callBook(() => book.chunkRequest(this.nextId(), c.addr, c.len));
      parts.push(await this.memoryRead(request, c.addr, c.len));
      if (this.ended) throw disconnectedError();
    }
    return concat(parts);
  }

  /** Delete offline record `target` (1-based) or `"all"` (FR W8SHXA) as the desktop
   * worker does: a fresh list, the typed confirmation (the record's name or "all"),
   * a backup of every affected record handed to `saveBackup` before anything is
   * written, one record-table write built by the wasm core, and a fresh list that must
   * equal the kept entries. Nothing is retried. */
  deleteRecords(
    target: number | "all",
    confirm: string,
    saveBackup: (name: string, csv: string) => void | Promise<void>,
    backupDir: string = BACKUP_DIR,
  ): Promise<RecordDeleteResult> {
    return this.meterRead("Deleting offline records", async () => {
      const { book, list } = await this.listLocked();
      const targets = Array.from(
        callBook(() => book.deleteTargets(target === "all" ? undefined : target, confirm)),
      );
      if (targets.length === 0) return { records: [], backup_dir: backupDir, backups: [] };
      const files = await this.backupsLocked(book, list, targets);
      try {
        // Every save settles (a written file) before the table write goes out.
        for (const f of files) await saveBackup(f.name, f.csv);
      } catch (e) {
        throw backupFailed(e);
      }
      try {
        const request = callBook(() => book.deleteRequest(this.nextId(), new Uint32Array(targets)));
        const reply = await this.exchange(request, REPLY_TIMEOUT_MS);
        if (this.ended) throw disconnectedError();
        callBook(() => book.takeDeleteReply(reply));
        const after = await this.listLocked();
        callBook(() => book.checkAfterDelete());
        return {
          records: after.list.records,
          backup_dir: backupDir,
          backups: files.map((f) => f.name),
        };
      } catch (e) {
        const ui = asUiError(bookError(e));
        throw { ...ui, message: `${ui.message} Backup: ${backupDir}` } satisfies UiError;
      }
    });
  }

  /** Download each of `targets` (records with no samples need no reads) and build its
   * backup file; any failure deletes nothing. */
  private async backupsLocked(
    book: RecordBookLike,
    list: RecordList,
    targets: number[],
  ): Promise<BackupFile[]> {
    const files: BackupFile[] = [];
    try {
      for (const index of targets) {
        const data =
          (list.records[index - 1]?.points ?? 0) > 0
            ? await this.recordWindowLocked(book, index)
            : new Uint8Array();
        files.push(callBook(() => book.backupFile(index, data)));
      }
    } catch (e) {
      throw backupFailed(e);
    }
    return files;
  }

  // ---- the startup logo (FR W8SHXB) ----

  /** Upload `source` as the startup logo as the desktop worker does: a level-1 auth if
   * needed, the device info read again, the native preconditions (SETTINGS read in this
   * session, a KM003C), every chunk (1000 ms each), the startup-duration write, the
   * paced read-back and one SETTINGS read. A bad picture is refused before the link is
   * taken; nothing is retried. */
  logoUpload(source: LogoSource, onProgress: (p: LogoProgress) => void): Promise<LogoWritten> {
    if (this.o.hid) return Promise.reject(hidOnlyBasic(LOGO));
    const w = this.o.wasm;
    let write: LogoWriteLike;
    try {
      write =
        "builtin" in source
          ? w.LogoWrite.uploadBuiltin(source.builtin)
          : w.LogoWrite.upload(source.bytes, source.isBin, source.alpha);
    } catch (e) {
      return Promise.reject(asUiError(e));
    }
    return this.logoSequence(write, onProgress);
  }

  /** Remove the startup logo: one 16-byte 0xFF chunk, the duration 0 and a 16-byte
   * read-back, as the desktop worker does. */
  logoRemove(onProgress: (p: LogoProgress) => void): Promise<LogoWritten> {
    if (this.o.hid) return Promise.reject(hidOnlyBasic(LOGO));
    return this.logoSequence(this.o.wasm.LogoWrite.remove(), onProgress);
  }

  private async logoSequence(
    write: LogoWriteLike,
    onProgress: (p: LogoProgress) => void,
  ): Promise<LogoWritten> {
    try {
      return await this.meterRead(LOGO, () => this.logo.write(write, onProgress));
    } finally {
      write.free();
    }
  }

  /** What the logo area holds (a level-1 auth if needed, then memory reads inside the
   * logo window only, paced); with `full`, the stored picture too. */
  logoRead(full: boolean, onProgress?: (p: LogoProgress) => void): Promise<LogoRead> {
    return this.meterRead(LOGO, () => this.logo.read(full, onProgress));
  }

  /** One read-only check of the settings against the last logo write's calibration
   * snapshot: `saved` and `calibration_changed` end the wait. */
  logoSettingsCheck(): Promise<LogoSettingsCheck> {
    return this.meterRead(LOGO, () => this.logo.settingsCheck());
  }

  /** Start the PD sniffer and hand every poll's decoded rows to `onRows`. With a
   * `decoder`, the caller owns it (the capture outlives the run); otherwise the driver
   * makes one and frees it when the capture stops. */
  async startPdCapture(
    onRows: (rows: PdRowView[]) => void,
    decoder?: PdDecoderLike,
  ): Promise<void> {
    if (this.o.hid) throw hidOnlyBasic("PD capture");
    if (this.ended || this.pd) return;
    const w = this.o.wasm;
    try {
      await this.exclusive(() => this.expectAccept(w.startPdSnifferRequest(this.nextId())));
    } catch (e) {
      throw asUiError(e);
    }
    const pd = decoder ? { decoder, owned: false } : { decoder: new w.PdDecoder(), owned: true };
    this.pd = pd;
    const tick = async (): Promise<void> => {
      if (this.ended || this.pd !== pd) return;
      try {
        const rows = await this.exclusive(async () => {
          if (this.ended || this.pd !== pd) return [];
          const reply = await this.exchange(
            w.pdPollRequest(this.nextId(), PD_POLL_WITH_ADC),
            REPLY_TIMEOUT_MS,
          );
          return (pd.decoder.decode(reply) as { rows: PdRowView[] }).rows;
        });
        if (rows.length > 0 && this.pd === pd) onRows(rows);
      } catch (e) {
        this.fail(e);
        return;
      }
      if (!this.ended && this.pd === pd)
        this.pdTimer = setTimeout(() => void tick(), PD_POLL_PERIOD_MS);
    };
    this.pdTimer = setTimeout(() => void tick(), PD_POLL_PERIOD_MS);
  }

  /** Stop polling PD traffic and stop the sniffer. */
  async stopPdCapture(): Promise<void> {
    const pd = this.pd;
    if (!pd || this.ended) return;
    this.pd = null;
    if (this.pdTimer) clearTimeout(this.pdTimer);
    this.pdTimer = null;
    if (pd.owned) pd.decoder.free();
    const w = this.o.wasm;
    try {
      await this.exclusive(() => this.expectAccept(w.stopPdSnifferRequest(this.nextId())));
    } catch (e) {
      throw asUiError(e);
    }
  }

  /** Stop the sniffer and the queue, then release the transport. */
  async disconnect(): Promise<void> {
    if (this.ended) return;
    const pd = this.pd;
    this.stopTimers();
    this.ended = true;
    const w = this.o.wasm;
    await this.lock;
    try {
      if (pd) await this.expectAccept(w.stopPdSnifferRequest(this.nextId()));
      if (this.mode === "queue") await this.expectAccept(w.stopQueueRequest(this.nextId()));
    } catch {
      // The meter may already be gone; the transport is released either way.
    } finally {
      await this.o.transport.close().catch(() => {});
    }
  }

  private stopTimers(): void {
    if (this.pollTimer) clearTimeout(this.pollTimer);
    if (this.pdTimer) clearTimeout(this.pdTimer);
    this.pollTimer = null;
    this.pdTimer = null;
    if (this.pd?.owned) this.pd.decoder.free();
    this.pd = null;
    // Every use of the book holds the link: free it once the link's queue (the request
    // in flight and any queued behind it, which see `ended` and stop) has settled.
    void this.lock.then(() => {
      this.book?.free();
      this.book = null;
    });
  }

  /** End the session on a transfer failure: no further frames, one `onError`. */
  private fail(e: unknown): void {
    if (this.ended) return;
    this.stopTimers();
    this.ended = true;
    this.o.onError(asUiError(e));
    void this.o.transport.close().catch(() => {});
  }
}
