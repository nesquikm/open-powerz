/**
 * FR QFTCWZ: the browser session driver. Opens the meter (WebUSB, or an offer to fall
 * back to WebHID) and runs the read-only session over a `WebTransport`.
 */

import type { UiError } from "../backend";
import { ERROR_KINDS } from "../errorKinds";
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
  PdDecoder: new () => PdDecoderLike;
}

/** The wasm `PdDecoder`: poll replies into rows, and the capture kept since the last
 * clear (details, clear, pdcap file). */
export interface PdDecoderLike {
  decode(reply: Uint8Array): unknown;
  details(index: number | bigint): unknown;
  clear(): unknown;
  pdcap(model: string, firmwareVersion: string): unknown;
  free(): void;
}

/** What opening a WebUSB device yields: a transport, or an offer to use WebHID instead. */
export type UsbOpenResult = { transport: WebTransport } | { offerHid: true; error: UiError };

function isUiError(e: unknown): e is UiError {
  return typeof e === "object" && e !== null && "kind" in e && "message" in e;
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
  if (isUiError(e) && (ERROR_KINDS as readonly string[]).includes(e.kind) && "hint" in e) {
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
    message: `${what} is not available over HID: it gives basic readings only.`,
    hint: "Free the meter's USB interface and connect over WebUSB for this.",
  };
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

  private constructor(private readonly o: MeterDriverOptions) {
    this.rate = o.rateCode;
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

  private async readBlock(block: { addr: number; len: number }): Promise<Uint8Array> {
    const w = this.o.wasm;
    const reply = await this.exchange(
      w.memoryReadRequest(this.nextId(), block.addr, block.len),
      REPLY_TIMEOUT_MS,
    );
    // The real meter sends the 20-byte confirmation alone and the encrypted data
    // (rounded up to 16 bytes) in further IN transfers; the demo meter joins them.
    // A shorter reply (a REJECT) goes straight to the decoder.
    const want = MEMORY_CONFIRMATION_LEN + Math.ceil(block.len / 16) * 16;
    if (reply.length < MEMORY_CONFIRMATION_LEN || reply.length >= want) {
      return w.decodeMemoryRead(reply, block.addr, block.len);
    }
    const parts: Uint8Array[] = [reply];
    let have = reply.length;
    while (have < want) {
      const part = await this.o.transport.read(REPLY_TIMEOUT_MS);
      parts.push(part);
      have += part.length;
    }
    const joined = new Uint8Array(have);
    let at = 0;
    for (const part of parts) {
      joined.set(part, at);
      at += part.length;
    }
    return w.decodeMemoryRead(joined, block.addr, block.len);
  }

  private async open(): Promise<void> {
    const w = this.o.wasm;
    await this.connectChecked();
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
    const start = () => this.expectAccept(w.startQueueRequest(this.nextId(), this.rate));
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
    return this.mode === "queue" ? queuePollPeriodMs(this.rate) : SNAPSHOT_PERIOD_MS;
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
