/**
 * FR W8SHXB: the startup logo's sequencing in the browser session. `MeterDriver` owns
 * the link (the HID refusal, the lock, `UiError` mapping and freeing the wasm write);
 * `LogoDriver` runs each logo command's frames inside it, as the desktop worker does.
 */

import type { LogoProgress, LogoRead, LogoSettingsCheck, LogoWritten, UiError } from "../backend";
import type { MeterInfo, PowerzWasm } from "./driver";
import { disconnectedError } from "./transport";

/** One planned memory read inside the logo window. */
export interface LogoReadChunk {
  addr: number;
  len: number;
}

/** The wasm `LogoWrite` (FR W8SHXB). */
export interface LogoWriteLike {
  preconditions(
    authLevel: number,
    settingsReply: Uint8Array | undefined,
    model: string | undefined,
  ): void;
  chunkCount(): number;
  chunkRequest(id: number, k: number): Uint8Array;
  takeChunkReply(k: number, reply: Uint8Array): void;
  flagRequest(id: number): Uint8Array;
  takeFlagReply(reply: Uint8Array): void;
  readPlan(): LogoReadChunk[];
  readRequest(id: number, addr: number, len: number): Uint8Array;
  takeRead(addr: number, data: Uint8Array): void;
  /** The step in flight failed off the meter with `reason`: throws the desktop wording
   * when the desktop words that step's failure, returns when the cause stands. */
  fail(reason: string): void;
  settingsRequest(id: number): Uint8Array;
  takeSettings(reply: Uint8Array): LogoWritten;
  pendingSnapshot(): Uint8Array | undefined;
  free(): void;
}

/** The wasm `LogoReader` (FR W8SHXB). */
export interface LogoReaderLike {
  readPlan(): LogoReadChunk[];
  readRequest(id: number, addr: number, len: number): Uint8Array;
  takeRead(addr: number, data: Uint8Array): void;
  result(): LogoRead;
  free(): void;
}

/** What a logo upload writes: a picture file's bytes, or a built-in logo. */
export type LogoSource =
  { bytes: Uint8Array; isBin: boolean; alpha: boolean } | { builtin: string };

/** What `LogoDriver` needs of the session; every call is made while it holds the link. */
export interface LogoLink {
  readonly wasm: PowerzWasm;
  nextId(): number;
  /** Send `frame` and wait for its reply (the session's reply timeout by default). */
  exchange(frame: Uint8Array, timeoutMs?: number): Promise<Uint8Array>;
  /** Send a memory read `request` of `len` bytes at `addr` and decode its reply. */
  memoryRead(request: Uint8Array, addr: number, len: number): Promise<Uint8Array>;
  /** Whether the session has ended. */
  ended(): boolean;
  /** A level-1 auth unless the session already holds it. */
  signIn(): Promise<void>;
  /** The device info read again (the four safe blocks). */
  readInfo(): Promise<MeterInfo>;
  authLevel(): number;
  /** The last SETTINGS reply read in this session. */
  settingsReply(): Uint8Array | undefined;
  /** The `UiError` message a failure leaves the session with. */
  errorMessage(e: unknown): string;
}

/** How long a logo chunk waits for FINISHED (native `CHUNK_TIMEOUT`). */
const LOGO_CHUNK_TIMEOUT_MS = 1_000;
/** Least gap between two logo-window reads (native `READ_GAP`). */
const LOGO_READ_GAP_MS = 50;
/** Least gap between two logo progress messages (desktop `PROGRESS_GAP_MS`). */
const LOGO_PROGRESS_GAP_MS = 50;

/** A progress callback that passes on at most one message per 50 ms (the first always),
 * as the desktop `ProgressGate`. */
function logoProgressGate(onProgress: (p: LogoProgress) => void): (p: LogoProgress) => void {
  let last: number | null = null;
  return (p) => {
    const now = Date.now();
    if (last !== null && now - last < LOGO_PROGRESS_GAP_MS) return;
    last = now;
    onProgress(p);
  };
}

/** The logo commands' frames over a `LogoLink`, with the state they keep per session:
 * the pacing of logo-window reads and a write's pending calibration snapshot. */
export class LogoDriver {
  /** The last logo write's calibration snapshot while its settings wait for a menu save. */
  private pending: Uint8Array | null = null;
  /** When the last logo-window read went out (reads are paced). */
  private readAt: number | null = null;

  constructor(private readonly link: LogoLink) {}

  /** Run `write` as the desktop worker does: signed in, the device info read again, the
   * native preconditions (SETTINGS read in this session, a KM003C), every chunk
   * (1000 ms each), the startup-duration write, the paced read-back and one SETTINGS
   * read. Nothing is retried; the caller frees `write`. */
  async write(write: LogoWriteLike, onProgress: (p: LogoProgress) => void): Promise<LogoWritten> {
    const l = this.link;
    // As the desktop `prepare_write`: signed in, the device info read again.
    await l.signIn();
    const info = await l.readInfo();
    write.preconditions(l.authLevel(), l.settingsReply(), info.model);
    const progress = logoProgressGate(onProgress);
    const total = write.chunkCount();
    for (let k = 0; k < total; k++) {
      const frame = write.chunkRequest(l.nextId(), k);
      const reply = await this.step(write, () => l.exchange(frame, LOGO_CHUNK_TIMEOUT_MS));
      write.takeChunkReply(k, reply);
      progress({ step: "chunks", done: k + 1, total });
    }
    const flag = write.flagRequest(l.nextId());
    write.takeFlagReply(await this.step(write, () => l.exchange(flag)));
    const plan = write.readPlan();
    for (const [i, c] of plan.entries()) {
      const request = write.readRequest(l.nextId(), c.addr, c.len);
      write.takeRead(c.addr, await this.step(write, () => this.pacedRead(request, c)));
      progress({ step: "read_back", done: i + 1, total: plan.length });
    }
    const settings = write.settingsRequest(l.nextId());
    const written = write.takeSettings(await this.step(write, () => l.exchange(settings)));
    this.pending = write.pendingSnapshot() ?? null;
    return written;
  }

  /** What the logo area holds (a level-1 auth if needed, then memory reads inside the
   * logo window only, paced); with `full`, the stored picture too. */
  async read(full: boolean): Promise<LogoRead> {
    const l = this.link;
    await l.signIn();
    const reader = new l.wasm.LogoReader(full);
    try {
      for (let plan = reader.readPlan(); plan.length > 0; plan = reader.readPlan()) {
        for (const c of plan) {
          const request = reader.readRequest(l.nextId(), c.addr, c.len);
          reader.takeRead(c.addr, await this.pacedRead(request, c));
        }
      }
      return reader.result();
    } finally {
      reader.free();
    }
  }

  /** One read-only check of the settings against the last logo write's calibration
   * snapshot: `saved` and `calibration_changed` end the wait. */
  async settingsCheck(): Promise<LogoSettingsCheck> {
    const pending = this.pending;
    if (!pending) {
      throw {
        kind: "usage",
        message: "No logo write is waiting for a settings save.",
        hint: null,
      } satisfies UiError;
    }
    const l = this.link;
    const reply = await l.exchange(l.wasm.settingsRequest(l.nextId()));
    const check = l.wasm.settingsSaveCheck(pending, reply);
    if (check.settings !== "needs_menu_save") this.pending = null;
    return check;
  }

  /** Run one step of `write` off the meter; a failure ends the write in the desktop
   * wording of that step, or as itself where the desktop keeps the cause. */
  private async step<T>(write: LogoWriteLike, run: () => Promise<T>): Promise<T> {
    try {
      return await run();
    } catch (e) {
      write.fail(this.link.errorMessage(e));
      throw e;
    }
  }

  /** One logo-window read, at least 50 ms after the previous one. */
  private async pacedRead(request: Uint8Array, c: LogoReadChunk): Promise<Uint8Array> {
    if (this.readAt !== null) {
      const wait = this.readAt + LOGO_READ_GAP_MS - Date.now();
      if (wait > 0) await new Promise((r) => setTimeout(r, wait));
    }
    if (this.link.ended()) throw disconnectedError();
    this.readAt = Date.now();
    return this.link.memoryRead(request, c.addr, c.len);
  }
}
