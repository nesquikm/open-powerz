/**
 * FR W8SHXC: the trigger engine's dedicated worker. It loads the wasm module and runs
 * the desktop trigger engine on a blocking port over two shared byte rings (the page
 * moves the bytes with Web Serial), a monotonic clock whose `sleep` blocks on shared
 * memory, and the page's VBUS cell. The page's calls `{ id, method, args }` are
 * answered `{ id, ok }` or `{ id, error }`; the first is `open` with
 * `[{ portIn, portOut, vbus }]`.
 *
 * Besides the answers the worker posts two kinds of message of its own:
 *
 * - `{ wrote: true }` after each write to the out-ring, so the page drains it at once
 *   (worker messages are not throttled in a hidden tab the way the page's timers are);
 * - `{ failed: UiError }` when the VBUS watch fails for any reason other than the
 *   session having closed: the page ends the session;
 * - `{ log: line }` for each line of the session's log (AC-AHTZW1.4), the lines of the
 *   handshake included: the page writes it to the console.
 */

import {
  ByteRing,
  VbusCell,
  answerRpc,
  engineError,
  sessionClosed,
  type RpcAnswer,
  type RpcCall,
} from "./triggerBridge";
import type { UiError } from "../backend";

/** How often the VBUS watch looks between calls (the desktop's `VBUS_CHECK`). */
export const VBUS_CHECK_MS = 100;
/** Longest one wait for room in the outgoing ring before it counts as gone. */
const WRITE_WAIT_MS = 2_000;

/** The wasm `TriggerEngine` as the worker drives it. */
export interface WorkerEngine {
  pushVbus(reading: unknown): void;
  close(): void;
  free(): void;
}

/** The worker's blocking port, as the engine takes it. */
export interface WorkerPort {
  read(timeoutMs: number): Uint8Array;
  write(bytes: Uint8Array): void;
}

/** The worker's clock, as the engine takes it. */
export interface WorkerClock {
  nowMs(): number;
  sleep(ms: number): void;
}

/** The wasm module's engine constructor. */
export interface WorkerGlue {
  TriggerEngine: {
    open(
      port: WorkerPort,
      clock: WorkerClock,
      vbus: { get(): unknown },
      log: (line: string) => void,
    ): WorkerEngine;
  };
}

/** The parts of the worker's global scope it uses. */
export interface WorkerScope {
  postMessage(message: unknown): void;
  addEventListener(type: "message", fn: (e: { data: unknown }) => void): void;
}

interface OpenChannels {
  portIn: SharedArrayBuffer;
  portOut: SharedArrayBuffer;
  vbus: SharedArrayBuffer;
}

/** A monotonic wall clock, ms: `performance.now()` never steps back the way
 * `Date.now()` can when the system clock is set. */
export function monotonicNowMs(): number {
  return performance.timeOrigin + performance.now();
}

/** Whether `e` is the engine refusing a call because its session closed. */
function isSessionClosed(e: UiError): boolean {
  const closed = sessionClosed();
  return e.kind === closed.kind && e.message === closed.message;
}

/** Write all of `bytes` to `ring`, waiting for room; throws once the ring is closed. */
function writeAll(ring: ByteRing, bytes: Uint8Array): void {
  let rest = bytes;
  while (rest.length > 0) {
    if (ring.closed) throw new Error("the serial port went away");
    const n = ring.write(rest);
    rest = rest.subarray(n);
    if (rest.length > 0) {
      const before = performance.now();
      ring.waitWhileSize(ring.size, WRITE_WAIT_MS);
      if (ring.size === ring.capacity && performance.now() - before >= WRITE_WAIT_MS) {
        throw new Error("the serial port does not take bytes");
      }
    }
  }
}

/** Run the trigger worker on `scope`, its engine from `loadGlue`. */
export function runTriggerWorker(
  scope: WorkerScope,
  loadGlue: () => Promise<WorkerGlue>,
  now: () => number = monotonicNowMs,
): void {
  let engine: WorkerEngine | null = null;
  let opening = false;
  let timer: ReturnType<typeof setInterval> | null = null;
  const sleeper = new Int32Array(new SharedArrayBuffer(4));

  /** Stop the VBUS watch and free the engine; never throws. */
  const stop = (): void => {
    if (timer !== null) clearInterval(timer);
    timer = null;
    const e = engine;
    engine = null;
    try {
      e?.free();
    } catch {
      // Already freed, or the module is broken: nothing more to let go.
    }
  };

  const open = async (channels: OpenChannels): Promise<void> => {
    const glue = await loadGlue();
    const portIn = new ByteRing(channels.portIn);
    const portOut = new ByteRing(channels.portOut);
    const vbus = new VbusCell(channels.vbus);
    const port: WorkerPort = {
      read: (timeoutMs) => {
        if (portIn.closed && portIn.size === 0) throw new Error("the serial port went away");
        return portIn.readBlocking(timeoutMs);
      },
      write: (bytes) => {
        writeAll(portOut, bytes);
        // The page drains the out-ring on this message.
        scope.postMessage({ wrote: true });
      },
    };
    const clock: WorkerClock = {
      nowMs: now,
      sleep: (ms) => {
        if (ms > 0) Atomics.wait(sleeper, 0, 0, ms);
      },
    };
    engine = glue.TriggerEngine.open(port, clock, { get: () => vbus.get() }, (line) =>
      scope.postMessage({ log: line }),
    );
    timer = setInterval(() => {
      const e = engine;
      if (!e) return;
      try {
        e.pushVbus(vbus.get());
      } catch (err) {
        const error = engineError(err);
        (err as { free?: () => void } | null)?.free?.();
        // The session ended meanwhile: the next call says so.
        if (isSessionClosed(error)) return;
        // Anything else ends the session: the page closes it.
        if (timer !== null) clearInterval(timer);
        timer = null;
        scope.postMessage({ failed: error });
      }
    }, VBUS_CHECK_MS);
  };

  scope.addEventListener("message", (e) => {
    const call = e.data as RpcCall;
    if (call.method === "open") {
      // One engine per worker: a second open would leave the first one running.
      if (engine || opening) {
        scope.postMessage({
          id: call.id,
          error: {
            kind: "usage",
            message: "A trigger session is already open.",
            hint: null,
          } satisfies UiError,
        });
        return;
      }
      opening = true;
      open(call.args[0] as OpenChannels).then(
        () => {
          opening = false;
          scope.postMessage({ id: call.id, ok: null });
        },
        (err: unknown) => {
          opening = false;
          stop();
          scope.postMessage({ id: call.id, error: engineError(err) });
        },
      );
      return;
    }
    if (!engine) {
      scope.postMessage({ id: call.id, error: sessionClosed() });
      return;
    }
    let answer: RpcAnswer | null = null;
    try {
      answer = answerRpc(engine, call);
    } finally {
      // The close is answered even when stopping throws.
      if (call.method === "close") stop();
      scope.postMessage(answer ?? { id: call.id, error: engineError(null) });
    }
  });
}

async function loadBundledGlue(): Promise<WorkerGlue> {
  const [glue, wasmUrl] = await Promise.all([
    import("../../wasm/powerz_wasm.js") as unknown as Promise<
      WorkerGlue & { default: (init: { module_or_path: string }) => Promise<unknown> }
    >,
    import("../../wasm/powerz_wasm_bg.wasm?url").then((m: { default: string }) => m.default),
  ]);
  await glue.default({ module_or_path: wasmUrl });
  return glue;
}

// In the dedicated worker only (not when a test imports this module).
const workerScope = globalThis as unknown as { WorkerGlobalScope?: unknown };
if (typeof workerScope.WorkerGlobalScope !== "undefined") {
  runTriggerWorker(globalThis as unknown as WorkerScope, loadBundledGlue);
}
