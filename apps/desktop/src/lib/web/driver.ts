/**
 * FR QFTCWZ: the browser session driver. Opens the meter (WebUSB, or an offer to fall
 * back to WebHID) and runs the read-only session over a `WebTransport`.
 */

import type { UiError } from "../backend";
import type { DemoMeterModule } from "./demoTransport";
import { disconnectedError, timeoutError, type WebTransport } from "./transport";
import { openWebUsb, type UsbDeviceLike, type WebUsbOptions } from "./webusb";

/** The parts of the generated wasm module (`src/wasm/powerz_wasm.js`) the driver uses. */
export interface PowerzWasm extends DemoMeterModule {
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
  decodeAuthReply(reply: Uint8Array, block: Uint8Array, nowMs: bigint, random: bigint): number;
  decodeMemoryRead(reply: Uint8Array, addr: number, len: number): Uint8Array;
  decodeDeviceInfo(
    hardware: Uint8Array,
    firmware: Uint8Array,
    otp: Uint8Array,
    uid: Uint8Array,
  ): MeterInfo;
  PdDecoder: new () => { decode(reply: Uint8Array): unknown; free(): void };
}

/** What opening a WebUSB device yields: a transport, or an offer to use WebHID instead. */
export type UsbOpenResult = { transport: WebTransport } | { offerHid: true; error: UiError };

function isUiError(e: unknown): e is UiError {
  return typeof e === "object" && e !== null && "kind" in e && "message" in e;
}

/**
 * Open `device` over its vendor bulk interface. When the interface cannot be claimed
 * (an OS driver holds it), resolve with an offer to switch to the WebHID transport,
 * which sends the same commands as 64-byte reports. Other failures reject.
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
const REPLY_TIMEOUT_MS = 1_000;
/** Mismatched-id replies discarded before a request fails (native `MAX_STALE_REPLIES`). */
const MAX_STALE_REPLIES = 3;
/** Rate code of 1000 SPS, polled faster (native `Streamer::poll_period`). */
const RATE_1000_SPS = 3;
/** ADC snapshot period without the queue `[inferred]`: the meter refreshes its ADC
 * block at its own rate, so a 100 ms poll keeps up with it. */
const SNAPSHOT_PERIOD_MS = 100;
/** PD sniffer poll period (native `PdSniffer::POLL_PERIOD`). */
const PD_POLL_PERIOD_MS = 5;
/** Each PD poll also asks for the ADC snapshot (the sniffer's `with_adc` option). */
const PD_POLL_WITH_ADC = true;

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
  /** The session ended on a transfer failure; called at most once. */
  onError: (error: UiError) => void;
  /** Random source for the auth nonce; defaults to `crypto.getRandomValues`. */
  random?: () => bigint;
}

const ERROR_KINDS: readonly string[] = [
  "permission_denied",
  "device_busy",
  "timeout",
  "protocol",
  "disconnected",
  "usage",
  "out_of_sync",
  "flash_needs_consent",
  "cancelled",
  "settings_not_saved",
  "internal",
];

/** Any failure as a `UiError`: transport errors pass through, wasm decode errors are
 * `protocol`, DOM transfer errors `disconnected` / `timeout`. */
function asUiError(e: unknown): UiError {
  if (isUiError(e) && ERROR_KINDS.includes(e.kind) && "hint" in e) {
    return { kind: e.kind, message: e.message, hint: e.hint ?? null };
  }
  if (typeof DOMException !== "undefined" && e instanceof DOMException) {
    return e.name === "TimeoutError" ? timeoutError() : disconnectedError();
  }
  if (isUiError(e)) return { kind: "protocol", message: String(e.message), hint: null };
  return { kind: "internal", message: "Something went wrong inside the app.", hint: null };
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
  private pd: { decoder: { decode(r: Uint8Array): unknown; free(): void } } | null = null;
  private readonly startedAt = Date.now();

  private constructor(private readonly o: MeterDriverOptions) {}

  /** Connect, read the device info, authenticate at level 1 and start the queue at
   * `rateCode`; when the meter refuses the auth or the queue, poll ADC snapshots. */
  static async connect(options: MeterDriverOptions): Promise<MeterDriver> {
    const driver = new MeterDriver(options);
    try {
      await driver.open();
    } catch (e) {
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

  /** Run `work` with the link to itself (one request in flight). */
  private exclusive<T>(work: () => Promise<T>): Promise<T> {
    const run = this.lock.then(work, work);
    this.lock = run.catch(() => {});
    return run;
  }

  private async expectAccept(frame: Uint8Array, timeoutMs = REPLY_TIMEOUT_MS): Promise<void> {
    this.o.wasm.decodeAccept(await this.exchange(frame, timeoutMs));
  }

  private async readBlock(block: { addr: number; len: number }): Promise<Uint8Array> {
    const w = this.o.wasm;
    const reply = await this.exchange(
      w.memoryReadRequest(this.nextId(), block.addr, block.len),
      REPLY_TIMEOUT_MS,
    );
    return w.decodeMemoryRead(reply, block.addr, block.len);
  }

  private async open(): Promise<void> {
    const w = this.o.wasm;
    await this.expectAccept(w.connectRequest(this.nextId()), CONNECT_TIMEOUT_MS);
    const hardware = await this.readBlock(HARDWARE_INFO);
    const firmware = await this.readBlock(FIRMWARE_INFO);
    const otp = await this.readBlock(OTP);
    const uid = await this.readBlock(UID);
    this.info = w.decodeDeviceInfo(hardware, firmware, otp, uid);
    if (await this.authenticate(uid)) {
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
    const start = () => this.expectAccept(w.startQueueRequest(this.nextId(), this.o.rateCode));
    try {
      await start();
      return true;
    } catch (e) {
      if (isUiError(e) && "hint" in e) throw e;
    }
    try {
      await this.expectAccept(w.stopQueueRequest(this.nextId()));
      await start();
      return true;
    } catch (e) {
      if (isUiError(e) && "hint" in e) throw e;
      return false;
    }
  }

  private periodMs(): number {
    return this.mode === "queue" ? queuePollPeriodMs(this.o.rateCode) : SNAPSHOT_PERIOD_MS;
  }

  private schedulePoll(delayMs: number): void {
    if (this.ended) return;
    this.pollTimer = setTimeout(() => {
      this.pollTimer = null;
      void this.pollOnce();
    }, delayMs);
  }

  private async pollOnce(): Promise<void> {
    if (this.ended) return;
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
      return w.decodeQueue(reply, this.o.rateCode);
    }
    const reply = await this.exchange(w.getDataRequest(this.nextId(), ATT_ADC), REPLY_TIMEOUT_MS);
    const adc = w.decodeAdc(reply);
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

  /** Start the PD sniffer and hand every poll's decoded rows to `onRows`. */
  async startPdCapture(onRows: (rows: PdRowView[]) => void): Promise<void> {
    if (this.ended || this.pd) return;
    const w = this.o.wasm;
    try {
      await this.exclusive(() => this.expectAccept(w.startPdSnifferRequest(this.nextId())));
    } catch (e) {
      throw asUiError(e);
    }
    const pd = { decoder: new w.PdDecoder() };
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
    pd.decoder.free();
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
    if (this.pd) this.pd.decoder.free();
    this.pd = null;
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
