/**
 * FR W8SHXC: protocol triggering in the browser, the page's side.
 *
 * A real meter's trigger session runs the desktop trigger engine (wasm) in a dedicated
 * worker: the page picks and opens the meter's serial port (as for the settings change,
 * and shared with it), pumps the port's bytes into the worker's in-ring and the
 * worker's out-ring to the port, keeps the VBUS cell current from the stream, and
 * forwards each trigger command as an RPC call. The demo meter runs the demo engine in
 * the page (its port is simulated and synchronous). The commands answer, and refuse,
 * as the desktop app's do.
 */

import type { MeterCommands, TriggerReply, UiError } from "../backend";
import type { MeterSample } from "./driver";
import { openSerialLink, pickMeterPort, type SerialLike, type SerialLink } from "./serial";
import { usage } from "./session";
import {
  ByteRing,
  RpcClient,
  VbusCell,
  engineError,
  sessionClosed,
  type VbusSample,
} from "./triggerBridge";

/** The parts of a `Worker` the bridge uses. */
export interface TriggerWorkerLike {
  postMessage(message: unknown): void;
  addEventListener(
    type: "message" | "error" | "messageerror",
    fn: (e: { data?: unknown }) => void,
  ): void;
  terminate(): void;
}

/** The wasm `TriggerEngine` (its methods take and give plain values; errors throw). */
type EngineLike = Record<string, unknown> & { free?: () => void };

/** The wasm module's demo engine constructor. */
interface TriggerModule {
  TriggerEngine: { openDemo(nowMs: number): EngineLike };
}

/** Each ring's room: far more than any PDM reply. */
const RING_BYTES = 64 * 1024;
/** The page drains the worker's out-ring on each worker message (the worker posts one
 * after every write); this slow timer is only the fallback. */
const PUMP_FALLBACK_MS = 25;
/** How long the page waits for room in the worker's in-ring. */
const RING_WAIT_MS = 4;
/** How long one port read waits before looking again. */
const READ_WAIT_MS = 1_000;
/** The highest queue rate while a trigger session is open (native `TRIGGER_MAX_SPS`). */
export const TRIGGER_MAX_SPS = 50;
/** Longest the close waits for the engine's way back to 5 V and `pdm close`. */
const CLOSE_WAIT_MS = 15_000;
/** Longest the open waits for the worker to load the engine and run the PDM handshake. */
export const OPEN_WAIT_MS = 10_000;
/** Above this a request's contract counts as above 5 V, mV. */
const SAFE_MV = 5_500;

/** The trigger commands, as the desktop app names them. */
type TriggerCommands = Pick<
  MeterCommands,
  | "triggerOpen"
  | "triggerDetect"
  | "triggerList"
  | "triggerRisk"
  | "triggerRequest"
  | "triggerArmed"
  | "triggerDisarm"
  | "triggerChargerGone"
  | "triggerPdEntered"
  | "triggerBackTo5v"
  | "triggerClose"
  | "triggerMessageRisk"
  | "triggerMessageSend"
  | "triggerCableRead"
  | "triggerConsoleRisk"
  | "triggerConsoleSend"
>;

const noSession = (): UiError => usage("No trigger session is open.");

/** The page is not cross-origin isolated, so no shared memory for the worker. */
const notIsolated = (): UiError => ({
  kind: "usage",
  message: "Protocol triggering needs the page to finish setting itself up.",
  hint: "Reload the page once, then try again.",
});

/** The engine's worker failed or went away: every call of the session refuses (as the
 * desktop's do once its trigger thread's port is gone). */
const engineStopped = (): UiError => ({
  kind: "disconnected",
  message: "The trigger engine stopped, so the trigger session ended.",
  hint: "Close the trigger panel, then open it again.",
});

/** The worker did not finish opening the session in time. */
const openTimedOut = (): UiError => ({
  kind: "timeout",
  message: "The trigger session did not open in time.",
  hint: "Unplug the meter, plug it back in, then try again.",
});

/** No Web Serial in this browser. */
const noWebSerial = (): UiError =>
  usage(
    "This browser has no Web Serial: protocol triggering needs Chrome or Edge on a desktop computer.",
  );

/** One open trigger session. */
interface TriggerHandle {
  call(method: string, args: unknown[]): Promise<unknown>;
  /** The stream's latest VBUS reading. */
  vbus(reading: VbusSample): void;
  /** The page goes away: the leave way back, then the close, sent at once; the promise
   * is the close's (it never rejects). */
  leave(): Promise<void>;
  close(): Promise<void>;
}

/** The demo meter's engine, in the page. */
class DemoTrigger implements TriggerHandle {
  constructor(private engine: EngineLike | null) {}

  call(method: string, args: unknown[]): Promise<unknown> {
    try {
      return Promise.resolve(this.callNow(method, args));
    } catch (e) {
      return Promise.reject(engineError(e));
    }
  }

  private callNow(method: string, args: unknown[]): unknown {
    const engine = this.engine;
    if (!engine) throw sessionClosed();
    const fn = engine[method];
    if (typeof fn !== "function") throw engineError(null);
    const out: unknown = (fn as (...a: unknown[]) => unknown).apply(engine, args);
    return out ?? null;
  }

  vbus(reading: VbusSample): void {
    try {
      this.callNow("pushVbus", [reading]);
    } catch {
      // Nothing to watch any more.
    }
  }

  leave(): Promise<void> {
    void this.call("leaveTo5v", []).catch(() => {});
    return this.close().catch(() => {});
  }

  async close(): Promise<void> {
    const engine = this.engine;
    if (!engine) return;
    try {
      this.callNow("close", []);
    } catch (e) {
      throw engineError(e);
    } finally {
      this.engine = null;
      engine.free?.();
    }
  }
}

/** A real meter's engine, in its worker, over the meter's serial port. */
class WorkerTrigger implements TriggerHandle {
  private readonly portIn = ByteRing.create(RING_BYTES);
  private readonly portOut = ByteRing.create(RING_BYTES);
  private readonly cell = VbusCell.create();
  private readonly rpc: RpcClient;
  private running = true;
  private pumpTimer: ReturnType<typeof setTimeout> | null = null;
  private readonly reading: Promise<void>;
  private writing: Promise<void> = Promise.resolve();
  private closing: Promise<void> | null = null;
  /** Why the session ended on its own (the worker failed); every call refuses with it. */
  private dead: UiError | null = null;

  constructor(
    private readonly worker: TriggerWorkerLike,
    private readonly link: SerialLink,
  ) {
    this.rpc = new RpcClient((m) => worker.postMessage(m));
    worker.addEventListener("message", (e) => this.onMessage(e.data));
    worker.addEventListener("error", () => this.die(engineStopped()));
    worker.addEventListener("messageerror", () => this.die(engineStopped()));
    this.reading = this.readLoop();
    this.schedulePump();
  }

  private onMessage(data: unknown): void {
    // Every worker message drains the out-ring: the worker posts one after each write.
    this.pumpOut();
    // A line of the session's log (AC-AHTZW1.4): to the console, no RPC answer.
    const log = (data as { log?: unknown } | null)?.log;
    if (typeof log === "string") {
      console.debug(`[trigger] ${log}`);
      return;
    }
    const failed = (data as { failed?: UiError } | null)?.failed;
    if (failed) {
      this.die(failed.kind === "disconnected" ? failed : engineStopped());
      return;
    }
    this.rpc.receive(data);
  }

  /** The worker failed: every pending and later call refuses, the engine is asked to go
   * back to 5 V and close (best effort), and the serial port is let go. */
  private die(error: UiError): void {
    if (this.dead) return;
    this.dead = error;
    this.rpc.failAll(error);
    if (!this.closing) {
      void this.rpc.call("leaveTo5v", []).catch(() => {});
      this.closing = this.shutdown().catch(() => {});
    }
  }

  /** Start the worker's engine on the rings (the PDM handshake runs there). Past
   * `OPEN_WAIT_MS` the worker is stopped, the port let go, and the open refused. */
  async start(): Promise<void> {
    const opened = this.rpc.call("open", [
      { portIn: this.portIn.buffer, portOut: this.portOut.buffer, vbus: this.cell.buffer },
    ]);
    let timer: ReturnType<typeof setTimeout> | undefined;
    const late = new Promise<"late">((resolve) => {
      timer = setTimeout(() => resolve("late"), OPEN_WAIT_MS);
    });
    try {
      const first = await Promise.race([opened.then(() => "open" as const), late]);
      if (first === "late") {
        opened.catch(() => {});
        await this.abort();
        throw openTimedOut();
      }
    } finally {
      clearTimeout(timer);
    }
    if (this.dead) throw this.dead;
  }

  /** Stop at once: no close is asked of the engine (it never opened). */
  private async abort(): Promise<void> {
    this.closing ??= Promise.resolve();
    this.worker.terminate();
    this.rpc.failAll(sessionClosed());
    await this.release();
  }

  /** The pumps stop, the rings close and the serial port is let go. */
  private async release(): Promise<void> {
    this.running = false;
    if (this.pumpTimer !== null) clearTimeout(this.pumpTimer);
    this.pumpTimer = null;
    this.portIn.close();
    this.portOut.close();
    await this.link.close().catch(() => {});
    await this.reading;
  }

  /** The port's bytes into the worker's in-ring, until the port or the session ends. */
  private async readLoop(): Promise<void> {
    while (this.running) {
      let chunk: Uint8Array;
      try {
        chunk = await this.link.read(READ_WAIT_MS);
      } catch (e) {
        if ((e as UiError | null)?.kind === "timeout") continue;
        break;
      }
      let rest = chunk;
      while (rest.length > 0 && this.running) {
        const n = this.portIn.write(rest);
        rest = rest.subarray(n);
        if (rest.length > 0) await new Promise((r) => setTimeout(r, RING_WAIT_MS));
      }
    }
    // The worker's reads say the port went away.
    this.portIn.close();
  }

  private schedulePump(): void {
    if (!this.running) return;
    this.pumpTimer = setTimeout(() => {
      this.pumpTimer = null;
      this.pumpOut();
      this.schedulePump();
    }, PUMP_FALLBACK_MS);
  }

  /** The worker's bytes to the port, in order. */
  private pumpOut(): void {
    if (!this.running || this.portOut.size === 0) return;
    const bytes = this.portOut.read();
    this.writing = this.writing.then(
      () => this.link.write(bytes),
      () => this.link.write(bytes),
    );
    this.writing.catch(() => {
      // The port went away: the worker's writes say so.
      this.portOut.close();
    });
  }

  call(method: string, args: unknown[]): Promise<unknown> {
    if (this.dead) return Promise.reject(this.dead);
    if (this.closing) return Promise.reject(sessionClosed());
    return this.rpc.call(method, args);
  }

  vbus(reading: VbusSample): void {
    this.cell.set(reading);
  }

  leave(): Promise<void> {
    if (!this.closing) void this.rpc.call("leaveTo5v", []).catch(() => {});
    return this.close().catch(() => {});
  }

  close(): Promise<void> {
    this.closing ??= this.shutdown();
    return this.closing;
  }

  private async shutdown(): Promise<void> {
    // Posted at once: the engine goes back to 5 V and sends `pdm close`.
    const closed = this.rpc.call("close", []);
    let timer: ReturnType<typeof setTimeout> | undefined;
    const late = new Promise<never>((_, reject) => {
      timer = setTimeout(
        () =>
          reject({
            kind: "timeout",
            message: "The trigger session did not close in time.",
            hint: "Unplug the charger if in doubt, then power-cycle the meter.",
          } satisfies UiError),
        CLOSE_WAIT_MS,
      );
    });
    let error: UiError | null = null;
    try {
      await Promise.race([closed, late]);
    } catch (e) {
      error = engineError(e);
    } finally {
      clearTimeout(timer);
    }
    // The last bytes out, then the pumps stop and the port is let go.
    this.pumpOut();
    await this.writing.catch(() => {});
    await this.release();
    this.worker.terminate();
    this.rpc.failAll(this.dead ?? sessionClosed());
    if (error) throw error;
  }
}

/** The meter's measurement session, as a trigger session changes it (desktop
 * `install_trigger_for`): its PD capture stops, its queue rate is capped, and its
 * polling pauses while the automatic list runs. */
export interface TriggerStream {
  /** The queue rate the session streams at; `null`: no queue (snapshots). */
  readonly rateSps: number | null;
  /** Stop a running PD capture, with the reason `trigger`. */
  pdStopForTrigger(): Promise<void>;
  /** Restart the queue at `rateSps`. */
  setRate(rateSps: number): Promise<void>;
  /** Hold or release the stream's polling. */
  holdPolling(hold: boolean): void;
}

/** What the trigger sessions run on. */
export interface TriggersOptions {
  /** The wasm module (its demo engine). */
  module: () => Promise<unknown>;
  /** The browser's Web Serial API, or `null` without it. */
  webSerial: SerialLike | null;
  /** Whether the page is cross-origin isolated (shared memory for the worker). */
  isolated: boolean;
  /** Starts the engine's dedicated worker. */
  startWorker: () => TriggerWorkerLike;
  /** Where the page's `pagehide` and `beforeunload` are listened for; `null`: nowhere. */
  pageEvents: EventTarget | null;
  /** Meter `deviceId`'s live measurement session; `null`: none, i.e. the meter is gone
   * (absent: no stream to change, and the meter counts as always there). */
  stream?: (deviceId: string) => TriggerStream | null;
}

/** Whether the contract `reply` reports is above 5 V; with no voltage in the reply, a
 * confirmed request counts as above (it needed the question). */
function aboveFiveVolts(reply: TriggerReply | null, confirmed: boolean): boolean {
  const mv = Math.max(reply?.requested_mv ?? 0, reply?.delivered_mv ?? 0);
  if (mv > 0) return mv > SAFE_MV;
  return confirmed;
}

/** The browser asks before the tab closes (the page cannot wait for a way back). */
function askBeforeUnload(e: Event): void {
  e.preventDefault();
  // Older browsers ask only when `returnValue` is set.
  (e as BeforeUnloadEvent).returnValue = "";
}

/** The open trigger sessions, one per meter. */
export class Triggers {
  private readonly open = new Map<string, TriggerHandle>();
  private readonly opening = new Set<string>();
  /** Sessions whose close is under way: their port is still held until it finished. */
  private readonly closing = new Map<string, Promise<void>>();
  /** Meters whose last accepted contract is above 5 V. */
  private readonly above = new Set<string>();
  private unloadGuard = false;
  /** The latest reading's device time and the reading before it, per meter. */
  private readonly lastTime = new Map<string, number>();
  /** The queue rate before the trigger session capped it, per meter. */
  private readonly rateBefore = new Map<string, number>();

  constructor(private readonly o: TriggersOptions) {
    // Best effort: the page posts the leave way back and the close to the engine at
    // once, but a closed tab cannot wait for them (the worker may be stopped before
    // the bytes reach the meter). The page tries; a closed tab cannot wait. Above 5 V
    // the `beforeunload` question below gives the user the chance to stay.
    o.pageEvents?.addEventListener("pagehide", () => {
      for (const [id, handle] of this.open) this.track(id, handle.leave());
      this.open.clear();
      this.rateBefore.clear();
      this.above.clear();
      this.syncUnloadGuard();
    });
  }

  /** Whether meter `deviceId`'s trigger session holds its serial port (until its close
   * has finished). */
  holds(deviceId: string): boolean {
    return this.open.has(deviceId) || this.opening.has(deviceId) || this.closing.has(deviceId);
  }

  /** Whether meter `deviceId` still has its measurement session. */
  private present(deviceId: string): boolean {
    return !this.o.stream || this.o.stream(deviceId) !== null;
  }

  /** Count meter `deviceId`'s port as held until `close` settles. */
  private track(deviceId: string, close: Promise<void>): Promise<void> {
    const done = close.finally(() => {
      if (this.closing.get(deviceId) === done) this.closing.delete(deviceId);
    });
    this.closing.set(deviceId, done);
    done.catch(() => {});
    return done;
  }

  /** Meter `deviceId`'s contract is now `above` 5 V or not. */
  private setAbove(deviceId: string, above: boolean): void {
    if (above) this.above.add(deviceId);
    else this.above.delete(deviceId);
    this.syncUnloadGuard();
  }

  /** While a contract above 5 V stands, the browser asks before the tab closes. */
  private syncUnloadGuard(): void {
    const want = this.above.size > 0;
    if (want === this.unloadGuard || !this.o.pageEvents) return;
    this.unloadGuard = want;
    if (want) this.o.pageEvents.addEventListener("beforeunload", askBeforeUnload);
    else this.o.pageEvents.removeEventListener("beforeunload", askBeforeUnload);
  }

  private async openFor(deviceId: string): Promise<void> {
    if (this.holds(deviceId)) throw usage("A trigger session is already open.");
    this.opening.add(deviceId);
    try {
      // The meter refuses a PD capture during PDM [device]: it stops first.
      await this.o
        .stream?.(deviceId)
        ?.pdStopForTrigger()
        .catch(() => {});
      const handle = deviceId.startsWith("demo:") ? await this.openDemo() : await this.openReal();
      // The meter went away meanwhile: its session's end found no trigger to close.
      if (!this.present(deviceId)) {
        await handle.close().catch(() => {});
        throw usage("Connect a meter first.");
      }
      this.open.set(deviceId, handle);
      await this.capRate(deviceId);
    } finally {
      this.opening.delete(deviceId);
    }
  }

  /** The queue stays at `TRIGGER_MAX_SPS` or less while triggering; the earlier rate
   * is kept for the close. */
  private async capRate(deviceId: string): Promise<void> {
    this.rateBefore.delete(deviceId);
    const stream = this.o.stream?.(deviceId);
    const before = stream?.rateSps ?? null;
    if (!stream || before === null || before <= TRIGGER_MAX_SPS) return;
    try {
      await stream.setRate(TRIGGER_MAX_SPS);
      this.rateBefore.set(deviceId, before);
    } catch {
      // The rate stays as it was; nothing to restore.
    }
  }

  /** Back to the rate the trigger session capped, if it did and the meter is there. */
  private async restoreRate(deviceId: string): Promise<void> {
    const before = this.rateBefore.get(deviceId);
    this.rateBefore.delete(deviceId);
    if (before === undefined) return;
    await this.o
      .stream?.(deviceId)
      ?.setRate(before)
      .catch(() => {});
  }

  private async openDemo(): Promise<TriggerHandle> {
    const w = (await this.o.module()) as TriggerModule;
    try {
      return new DemoTrigger(w.TriggerEngine.openDemo(Date.now()));
    } catch (e) {
      throw engineError(e);
    }
  }

  private async openReal(): Promise<TriggerHandle> {
    if (!this.o.isolated) throw notIsolated();
    const serial = this.o.webSerial;
    if (!serial) throw noWebSerial();
    const port = await pickMeterPort(serial);
    const link = await openSerialLink(port);
    let worker: TriggerWorkerLike;
    try {
      worker = this.o.startWorker();
    } catch {
      await link.close().catch(() => {});
      throw engineError(null);
    }
    const handle = new WorkerTrigger(worker, link);
    try {
      await handle.start();
    } catch (e) {
      await handle.close().catch(() => {});
      throw engineError(e);
    }
    return handle;
  }

  /** Close meter `deviceId`'s trigger session, if one is open. */
  async closeFor(deviceId: string): Promise<void> {
    const handle = this.open.get(deviceId);
    this.lastTime.delete(deviceId);
    if (!handle) {
      this.rateBefore.delete(deviceId);
      return;
    }
    this.open.delete(deviceId);
    this.setAbove(deviceId, false);
    // The port stays held until the close has finished.
    const closed = this.track(deviceId, handle.close());
    try {
      await closed;
    } finally {
      await this.restoreRate(deviceId);
    }
  }

  /** The stream's samples of meter `deviceId` reach its session's VBUS watch. */
  samples(deviceId: string, samples: readonly MeterSample[], rateSps: number | null): void {
    const handle = this.open.get(deviceId);
    const last = samples.at(-1);
    if (!handle || !last) return;
    const before = samples.at(-2)?.time_ms ?? this.lastTime.get(deviceId);
    this.lastTime.set(deviceId, last.time_ms);
    const step = before !== undefined ? last.time_ms - before : 0;
    const period_ms = step > 0 ? step : rateSps ? 1000 / rateSps : 20;
    handle.vbus({
      volts: last.vbus_v,
      cc1_v: last.cc1_v,
      cc2_v: last.cc2_v,
      time_ms: Math.max(0, Math.round(last.time_ms)),
      period_ms,
    });
  }

  /** The trigger commands of meter `deviceId()` (`null`: no meter). */
  commands(deviceId: () => string | null): TriggerCommands {
    const handle = (): TriggerHandle | null => {
      const id = deviceId();
      return id === null ? null : (this.open.get(id) ?? null);
    };
    const call = async <T>(method: string, ...args: unknown[]): Promise<T> => {
      const h = handle();
      if (!h) throw noSession();
      return (await h.call(method, args)) as T;
    };
    /** With no session: `fallback`, as the desktop answers. */
    const quiet = async <T>(method: string, fallback: T): Promise<T> => {
      const h = handle();
      if (!h) return fallback;
      try {
        return (await h.call(method, [])) as T;
      } catch {
        return fallback;
      }
    };
    return {
      triggerOpen: async () => {
        const id = deviceId();
        if (id === null) throw usage("Connect a meter first.");
        await this.openFor(id);
      },
      triggerDetect: async () => {
        // The stream's polling pauses while the automatic list runs, and resumes
        // however it ends (desktop `PollingHeld`).
        const id = deviceId();
        const stream = id === null || !handle() ? null : (this.o.stream?.(id) ?? null);
        stream?.holdPolling(true);
        try {
          return await call("detect");
        } finally {
          stream?.holdPolling(false);
        }
      },
      triggerList: (protocol) => call("list", protocol),
      triggerRisk: (request) => call("risk", request),
      triggerRequest: async (request, confirmed, remember = false) => {
        const id = deviceId();
        const reply = await call<TriggerReply>("request", request, confirmed, remember);
        if (id !== null && this.open.has(id)) this.setAbove(id, aboveFiveVolts(reply, confirmed));
        return reply;
      },
      triggerArmed: () => quiet("armed", false),
      triggerDisarm: async () => {
        await quiet("disarm", null);
      },
      triggerChargerGone: () => quiet("chargerGone", false),
      triggerPdEntered: () => quiet("pdEntered", false),
      triggerBackTo5v: async (leaving = false) => {
        const id = deviceId();
        await call(leaving ? "leaveTo5v" : "backTo5v");
        if (id !== null) this.setAbove(id, false);
      },
      triggerClose: async () => {
        const id = deviceId();
        if (id !== null) await this.closeFor(id);
      },
      triggerMessageRisk: (message) => call("messageRisk", message),
      triggerMessageSend: async (message, confirmed) => {
        const id = deviceId();
        const reply = await call<Awaited<ReturnType<TriggerCommands["triggerMessageSend"]>>>(
          "sendMessage",
          message,
          confirmed,
        );
        // A confirmed message was above 5 V: it may stand on the charger now.
        if (confirmed && id !== null && this.open.has(id)) this.setAbove(id, true);
        return reply;
      },
      triggerCableRead: () => call("cableRead"),
      triggerConsoleRisk: (line) => call("consoleRisk", line),
      triggerConsoleSend: async (line, confirmed) => {
        const id = deviceId();
        const lines = await call<Awaited<ReturnType<TriggerCommands["triggerConsoleSend"]>>>(
          "consoleSend",
          line,
          confirmed,
        );
        if (confirmed && id !== null && this.open.has(id)) this.setAbove(id, true);
        return lines;
      },
    };
  }
}
