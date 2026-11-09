/**
 * FR W8SHXC: the pure parts of the browser trigger bridge, shared by the page and the
 * trigger engine's worker.
 *
 * The trigger engine (wasm) blocks on its port, so it runs in a dedicated worker; the
 * page owns the Web Serial port and moves the bytes through shared memory:
 *
 * - `ByteRing`: a one-writer, one-reader byte FIFO over a `SharedArrayBuffer`.
 * - `VbusCell`: the session's latest VBUS reading, set by the page from each stream
 *   batch and read by the worker's VBUS watch.
 * - `RpcClient` / `answerRpc`: the page's promise calls `{ id, method, args }` and the
 *   worker's `{ id, ok }` / `{ id, error }` answers.
 */

import type { UiError } from "../backend";

/** Header cells (Int32): read index, write index, bytes held, closed flag. */
const READ = 0;
const WRITE = 1;
const SIZE = 2;
const CLOSED = 3;
const HEADER_BYTES = 16;

/** A byte FIFO in shared memory: one side writes, the other reads. */
export class ByteRing {
  readonly buffer: SharedArrayBuffer;
  readonly capacity: number;
  private readonly header: Int32Array;
  private readonly data: Uint8Array;

  /** A new ring holding up to `capacity` bytes. */
  static create(capacity: number): ByteRing {
    return new ByteRing(new SharedArrayBuffer(HEADER_BYTES + capacity));
  }

  /** The ring over `buffer` (the other side's ring sees the same bytes). */
  constructor(buffer: SharedArrayBuffer) {
    this.buffer = buffer;
    this.capacity = buffer.byteLength - HEADER_BYTES;
    this.header = new Int32Array(buffer, 0, HEADER_BYTES / 4);
    this.data = new Uint8Array(buffer, HEADER_BYTES, this.capacity);
  }

  /** Bytes held. */
  get size(): number {
    return Atomics.load(this.header, SIZE);
  }

  get closed(): boolean {
    return Atomics.load(this.header, CLOSED) !== 0;
  }

  /** Write what fits of `bytes`; returns how many bytes went in. */
  write(bytes: Uint8Array): number {
    const free = this.capacity - this.size;
    const n = Math.min(free, bytes.length);
    let at = Atomics.load(this.header, WRITE);
    for (let i = 0; i < n; i++) {
      this.data[at] = bytes[i] ?? 0;
      at = at + 1 === this.capacity ? 0 : at + 1;
    }
    Atomics.store(this.header, WRITE, at);
    if (n > 0) {
      Atomics.add(this.header, SIZE, n);
      Atomics.notify(this.header, SIZE);
    }
    return n;
  }

  /** Take up to `max` held bytes (all of them by default); empty when none. */
  read(max = Infinity): Uint8Array {
    const n = Math.min(this.size, max);
    const out = new Uint8Array(n);
    let at = Atomics.load(this.header, READ);
    for (let i = 0; i < n; i++) {
      out[i] = this.data[at] ?? 0;
      at = at + 1 === this.capacity ? 0 : at + 1;
    }
    Atomics.store(this.header, READ, at);
    if (n > 0) {
      Atomics.sub(this.header, SIZE, n);
      Atomics.notify(this.header, SIZE);
    }
    return out;
  }

  /** The held bytes, waiting up to `timeoutMs` for some (empty then, or once closed).
   * Blocks the thread: for the worker only. */
  readBlocking(timeoutMs: number): Uint8Array {
    const deadline = performance.now() + timeoutMs;
    for (;;) {
      if (this.size > 0) return this.read();
      if (this.closed) return new Uint8Array(0);
      const left = deadline - performance.now();
      if (left <= 0) return new Uint8Array(0);
      Atomics.wait(this.header, SIZE, 0, left);
    }
  }

  /** Wait up to `timeoutMs` while the ring holds `full` bytes. Blocks the thread. */
  waitWhileSize(full: number, timeoutMs: number): void {
    Atomics.wait(this.header, SIZE, full, timeoutMs);
  }

  /** End the ring: its reader wakes with nothing. */
  close(): void {
    Atomics.store(this.header, CLOSED, 1);
    Atomics.notify(this.header, SIZE);
  }
}

/** One VBUS reading of the stream, as the engine's VBUS watch takes it. */
export interface VbusSample {
  volts: number;
  cc1_v: number;
  cc2_v: number;
  /** The record's device time, ms. */
  time_ms: number;
  /** How far apart the stream's records are, ms. */
  period_ms: number;
}

/** Sequence cell (Int32, odd while a write is under way) and the has-reading flag. */
const SEQ = 0;
const HAS = 1;
const VBUS_FIELDS = 5;

/** The latest VBUS reading in shared memory (seqlock: a reader never sees half a write). */
export class VbusCell {
  readonly buffer: SharedArrayBuffer;
  private readonly flags: Int32Array;
  private readonly values: Float64Array;

  static create(): VbusCell {
    return new VbusCell(new SharedArrayBuffer(8 + VBUS_FIELDS * 8));
  }

  constructor(buffer: SharedArrayBuffer) {
    this.buffer = buffer;
    this.flags = new Int32Array(buffer, 0, 2);
    this.values = new Float64Array(buffer, 8, VBUS_FIELDS);
  }

  set(r: VbusSample): void {
    Atomics.add(this.flags, SEQ, 1);
    this.values.set([r.volts, r.cc1_v, r.cc2_v, r.time_ms, r.period_ms]);
    Atomics.store(this.flags, HAS, 1);
    Atomics.add(this.flags, SEQ, 1);
  }

  /** The latest reading, `null` before the first. */
  get(): VbusSample | null {
    for (;;) {
      const before = Atomics.load(this.flags, SEQ);
      if (before % 2 !== 0) continue;
      const has = Atomics.load(this.flags, HAS) !== 0;
      const [volts = 0, cc1_v = 0, cc2_v = 0, time_ms = 0, period_ms = 0] = this.values;
      if (Atomics.load(this.flags, SEQ) !== before) continue;
      return has ? { volts, cc1_v, cc2_v, time_ms, period_ms } : null;
    }
  }
}

/** A call the page posts to the worker. */
export interface RpcCall {
  id: number;
  method: string;
  args: unknown[];
}

/** The worker's answer to one call. */
export interface RpcAnswer {
  id: number;
  ok?: unknown;
  error?: UiError;
}

/** An engine (binding) error, or any failure, as a plain `UiError` (it crosses
 * `postMessage`, so getters of a wasm error object are copied out). */
export function engineError(e: unknown): UiError {
  if (typeof e === "object" && e !== null && "kind" in e && "message" in e) {
    const { kind, message, hint } = e as { kind: unknown; message: unknown; hint?: unknown };
    return {
      kind: String(kind) as UiError["kind"],
      message: String(message),
      hint: typeof hint === "string" ? hint : null,
    };
  }
  return { kind: "internal", message: "Something went wrong inside the app.", hint: null };
}

/** A call on a trigger session that has ended (or never opened its engine). */
export const sessionClosed = (): UiError => ({
  kind: "usage",
  message: "The trigger session is closed.",
  hint: null,
});

/** The page's side of the RPC: each call a promise, answered by its id. */
export class RpcClient {
  private nextId = 1;
  private readonly waiting = new Map<
    number,
    { resolve: (v: unknown) => void; reject: (e: UiError) => void }
  >();

  constructor(private readonly post: (message: RpcCall) => void) {}

  call(method: string, args: unknown[]): Promise<unknown> {
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      this.waiting.set(id, { resolve, reject });
      try {
        this.post({ id, method, args });
      } catch (e) {
        this.waiting.delete(id);
        reject(engineError(e));
      }
    });
  }

  /** The worker's answer `message`. */
  receive(message: unknown): void {
    const m = message as RpcAnswer;
    const w = this.waiting.get(m.id);
    if (!w) return;
    this.waiting.delete(m.id);
    if (m.error) w.reject(m.error);
    else w.resolve(m.ok);
  }

  /** Reject every pending call with `error` (the worker went away). */
  failAll(error: UiError): void {
    const pending = [...this.waiting.values()];
    this.waiting.clear();
    for (const w of pending) w.reject(error);
  }
}

/** The worker's side: call `engine[method](...args)` and answer by the call's id. */
export function answerRpc(engine: object, call: RpcCall): RpcAnswer {
  const fn = (engine as Record<string, unknown>)[call.method];
  if (typeof fn !== "function") {
    return {
      id: call.id,
      error: {
        kind: "internal",
        message: `The trigger engine has no ${call.method}.`,
        hint: null,
      },
    };
  }
  try {
    const ok: unknown = (fn as (...a: unknown[]) => unknown).apply(engine, call.args);
    return { id: call.id, ok: ok ?? null };
  } catch (e) {
    const error = engineError(e);
    (e as { free?: () => void } | null)?.free?.();
    return { id: call.id, error };
  }
}
