/**
 * FR QFTCWZ: the demo meter behind `WebTransport`. Wraps the wasm `DemoMeter` and
 * advances its clock by the wall time that passed between calls.
 */

import type { DemoCdcLike } from "./cdcTransport";
import { disconnectedError, timeoutError, type WebTransport } from "./transport";

interface DemoMeterLike {
  transact(request: Uint8Array): Uint8Array;
  advance(ms: number): void;
  /** The meter's serial port, sharing its stored settings (FR W8SHX9). */
  cdcTwin(): DemoCdcLike;
  free(): void;
}

/** The part of the wasm module the demo transport needs. */
export interface DemoMeterModule {
  DemoMeter: new (scenario: string) => DemoMeterLike;
}

export class DemoTransport implements WebTransport {
  private meter: DemoMeterLike | null;
  private last: number;

  /** Throws when the wasm module does not know `scenario`. */
  constructor(
    wasm: DemoMeterModule,
    scenario: string,
    private readonly now: () => number = () => performance.now(),
  ) {
    this.meter = new wasm.DemoMeter(scenario);
    this.last = now();
  }

  private tick(meter: DemoMeterLike, t = this.now()): void {
    const elapsed = t - this.last;
    if (elapsed > 0) {
      meter.advance(elapsed);
      this.last = t;
    }
  }

  async transact(frame: Uint8Array, timeoutMs: number): Promise<Uint8Array> {
    const meter = this.meter;
    if (!meter) throw disconnectedError();
    this.tick(meter);
    const reply = meter.transact(frame);
    if (reply.length > 0) return reply;
    return this.timeOut(timeoutMs);
  }

  /** The demo meter's CDC twin (FR W8SHX9): it shares this meter's stored settings
   * and its clock, which {@link syncTo} moves for both. */
  cdcTwin(): DemoCdcLike {
    const meter = this.meter;
    if (!meter) throw disconnectedError();
    this.tick(meter);
    return meter.cdcTwin();
  }

  /** Move the shared demo clock up to time `t` (on this transport's time base). */
  syncTo(t: number): void {
    if (this.meter) this.tick(this.meter, t);
  }

  async read(timeoutMs: number): Promise<Uint8Array> {
    if (!this.meter) throw disconnectedError();
    // The demo meter answers inside transact(); nothing is ever pending.
    return this.timeOut(timeoutMs);
  }

  /** Wait out `timeoutMs`, then reject `timeout` (or `disconnected` if closed meanwhile). */
  private async timeOut(timeoutMs: number): Promise<never> {
    await new Promise((r) => setTimeout(r, timeoutMs));
    throw this.meter ? timeoutError() : disconnectedError();
  }

  async close(): Promise<void> {
    this.meter?.free();
    this.meter = null;
  }
}
