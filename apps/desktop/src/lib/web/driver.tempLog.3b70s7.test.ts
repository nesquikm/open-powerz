/**
 * FR 3B70S7 AC-3B70S7.3: while the web queue streams, a temperature snapshot that
 * fails or reads no temperature logs one debug line saying which; the next line comes
 * only when the outcome changes.
 *
 * Runs the driver over the fake WebUSB meter that answers like the real KM003C
 * (`realMeter.test.ts`) on Vitest's fake clock. A wrapper transport fails the ADC
 * snapshot on demand, and a wrapped wasm reads the snapshot with no temperature on
 * demand; `console.debug` is spied on.
 */

import { afterEach, beforeEach, describe, expect, it, vi, type MockInstance } from "vitest";
import type { UiError } from "../backend";
import { MeterDriver, openUsbOrOfferHid, type PowerzWasm } from "./driver";
import { realMeterDevice } from "./realMeter.test";
import { classify, loadWasm } from "./testWasm.test";
import type { WebTransport } from "./transport";
import { settle } from "./webBackendFakes.test";

type DriverOptions = Parameters<typeof MeterDriver.connect>[0];

let w: PowerzWasm;
let debug: MockInstance<(...args: unknown[]) => void>;

beforeEach(async () => {
  w = await loadWasm();
  vi.useFakeTimers({
    toFake: ["setTimeout", "clearTimeout", "setInterval", "clearInterval", "Date"],
  });
  vi.setSystemTime(1_790_000_000_000);
  debug = vi.spyOn(console, "debug").mockImplementation(() => {});
});

afterEach(() => {
  debug.mockRestore();
  vi.useRealTimers();
});

/** The ADC snapshot's outcome the fakes give. */
type Outcome = "ok" | "fail" | "none";

/** Fails ADC snapshot transfers (not disconnects) while `outcome` is `fail`. */
class SnapshotFailer implements WebTransport {
  constructor(
    private inner: WebTransport,
    private state: { outcome: Outcome },
  ) {}
  async transact(frame: Uint8Array, timeoutMs: number): Promise<Uint8Array> {
    if (this.state.outcome === "fail" && classify(w, frame) === "get_data:adc") {
      throw {
        kind: "protocol",
        message: "the ADC reply was cut short",
        hint: null,
      } satisfies UiError;
    }
    return this.inner.transact(frame, timeoutMs);
  }
  read(timeoutMs: number): Promise<Uint8Array> {
    return this.inner.read(timeoutMs);
  }
  close(): Promise<void> {
    return this.inner.close();
  }
}

/** `w` with `decodeAdc` reading no temperature while `outcome` is `none`. */
function wasmWithoutTemp(state: { outcome: Outcome }): PowerzWasm {
  return new Proxy(w, {
    get(target, prop) {
      const value = Reflect.get(target, prop) as unknown;
      if (prop === "decodeAdc") {
        return (reply: Uint8Array) => {
          const adc = (value as (r: Uint8Array) => Record<string, unknown>).call(target, reply);
          return state.outcome === "none" ? { ...adc, temp_c: null } : adc;
        };
      }
      return typeof value === "function" ? (value as () => unknown).bind(target) : value;
    },
  });
}

interface Run {
  driver: MeterDriver;
  state: { outcome: Outcome };
  temps: (number | null)[];
  errors: UiError[];
}

/** Connect the driver at 50 SPS (queue mode) over the fake real meter. */
async function connect(): Promise<Run> {
  const state: { outcome: Outcome } = { outcome: "ok" };
  const dev = realMeterDevice(w, { scenario: "load-5v-1a" });
  const opened = await settle(
    openUsbOrOfferHid(dev as unknown as Parameters<typeof openUsbOrOfferHid>[0], {
      debug: false,
    }),
  );
  if (!("transport" in opened)) throw new Error("expected the WebUSB transport");
  const temps: (number | null)[] = [];
  const errors: UiError[] = [];
  const driver = await settle(
    MeterDriver.connect({
      wasm: wasmWithoutTemp(state),
      transport: new SnapshotFailer(opened.transport, state),
      rateCode: 2,
      onBatch: () => {},
      onTemp: (t: number | null) => temps.push(t),
      onError: (e: UiError) => errors.push(e),
      random: () => 0x1234_5678_9abc_def0n,
    } as unknown as DriverOptions),
  );
  expect(driver.mode).toBe("queue");
  return { driver, state, temps, errors };
}

/** The `[temp]` debug lines logged so far, as text. */
const tempLines = (): string[] =>
  debug.mock.calls
    .map((args) => args.map((a) => (typeof a === "string" ? a : String(a))).join(" "))
    .filter((line) => line.startsWith("[temp]"));

describe("the web driver logs a bad temperature snapshot once per change (AC-3B70S7.3)", () => {
  it("3b70s7: good snapshots log no [temp] line", async () => {
    const { driver, temps, errors } = await connect();
    await vi.advanceTimersByTimeAsync(3_000);
    expect(temps.some((t) => typeof t === "number")).toBe(true);
    expect(tempLines()).toEqual([]);
    expect(errors).toEqual([]);
    await settle(driver.disconnect());
  });

  it("3b70s7: failing snapshots log one line saying failed, with the error message", async () => {
    const run = await connect();
    run.state.outcome = "fail";
    // At 500 ms per snapshot, 3 s is about six failed snapshots.
    await vi.advanceTimersByTimeAsync(3_000);
    const lines = tempLines();
    expect(lines).toHaveLength(1);
    expect(lines[0]).toMatch(/failed/i);
    expect(lines[0]).toContain("the ADC reply was cut short");
    expect(run.errors).toEqual([]);
    await settle(run.driver.disconnect());
  });

  it("3b70s7: snapshots with no temperature log one line saying no temperature", async () => {
    const run = await connect();
    run.state.outcome = "none";
    await vi.advanceTimersByTimeAsync(3_000);
    expect(run.temps.slice(-3)).toEqual([null, null, null]);
    const lines = tempLines();
    expect(lines).toHaveLength(1);
    expect(lines[0]).toMatch(/no temperature/i);
    expect(lines[0]).not.toMatch(/failed/i);
    await settle(run.driver.disconnect());
  });

  it("3b70s7: a new line comes only when the outcome changes", async () => {
    const run = await connect();
    run.state.outcome = "fail";
    await vi.advanceTimersByTimeAsync(2_000);
    expect(tempLines()).toHaveLength(1);

    run.state.outcome = "none";
    await vi.advanceTimersByTimeAsync(2_000);
    let lines = tempLines();
    expect(lines).toHaveLength(2);
    expect(lines[1]).toMatch(/no temperature/i);

    // Valid readings may log one line or none; a bad outcome after them logs again.
    run.state.outcome = "ok";
    await vi.advanceTimersByTimeAsync(2_000);
    const afterGood = tempLines().length;
    expect(afterGood).toBeLessThanOrEqual(3);

    run.state.outcome = "none";
    await vi.advanceTimersByTimeAsync(2_000);
    lines = tempLines();
    expect(lines).toHaveLength(afterGood + 1);
    expect(lines[lines.length - 1]).toMatch(/no temperature/i);

    // The same outcome again logs nothing more.
    await vi.advanceTimersByTimeAsync(2_000);
    expect(tempLines()).toHaveLength(afterGood + 1);
    expect(run.errors).toEqual([]);
    await settle(run.driver.disconnect());
  });
});
