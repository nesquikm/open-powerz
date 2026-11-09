/**
 * FR QFTCWY, AC-QFTCWY.4 (Testing): smoke test of the built WebAssembly module in Node.
 *
 * Loads apps/desktop/src/wasm/ (written by `bun run build:wasm`, which the gate runs before
 * Vitest) and drives the demo meter through the exported builders and decoders: connect,
 * a UID memory read, auth at level 1 and one queue poll. The module is imported by URL at
 * run time, so typechecking does not need the generated glue to exist.
 */

import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { beforeAll, describe, expect, it } from "vitest";

const GLUE_URL = new URL("../../wasm/powerz_wasm.js", import.meta.url);
const MODULE_URL = new URL("../../wasm/powerz_wasm_bg.wasm", import.meta.url);

const UID_ADDR = 0x4001_0450;
const UID_LEN = 12;
const RATE_50_SPS = 2;
const ATT_ADC_QUEUE = 0x002;

interface DemoMeter {
  transact(request: Uint8Array): Uint8Array;
  advance(ms: number): void;
  free(): void;
}

interface QueueSample {
  time_ms: number;
  vbus_v: number;
  ibus_a: number;
  cc1_v: number;
  cc2_v: number;
  dp_v: number;
  dm_v: number;
}

interface BindingError {
  kind: string;
  message: string;
}

/** The parts of the generated glue this test uses. */
interface Wasm {
  initSync(options: { module: Uint8Array }): unknown;
  DemoMeter: new (scenario: string) => DemoMeter;
  connectRequest(id: number): Uint8Array;
  memoryReadRequest(id: number, addr: number, len: number): Uint8Array;
  authRequest(
    id: number,
    level: number,
    credential: Uint8Array,
    nowMs: bigint,
    random: bigint,
  ): Uint8Array;
  startQueueRequest(id: number, rateCode: number): Uint8Array;
  getDataRequest(id: number, mask: number): Uint8Array;
  decodeAccept(reply: Uint8Array): void;
  decodeMemoryRead(reply: Uint8Array, addr: number, len: number): Uint8Array;
  decodeAuthReply(reply: Uint8Array, credential: Uint8Array, nowMs: bigint, random: bigint): number;
  decodeQueue(reply: Uint8Array, rateCode: number): QueueSample[];
}

let wasm: Wasm;

beforeAll(async () => {
  const glue = fileURLToPath(GLUE_URL);
  if (!existsSync(glue)) throw new Error(`${glue} is missing: run bun run build:wasm first`);
  wasm = (await import(/* @vite-ignore */ GLUE_URL.href)) as Wasm;
  wasm.initSync({ module: readFileSync(fileURLToPath(MODULE_URL)) });
});

/** The error the binding throws from `fn`, or a failure when it returns. */
function thrown(fn: () => unknown): BindingError {
  try {
    fn();
  } catch (e) {
    return e as BindingError;
  }
  throw new Error("expected the binding to throw");
}

describe("powerz-wasm in Node (AC-QFTCWY.4)", () => {
  it("connects, authenticates and polls the queue of the demo meter", () => {
    const meter = new wasm.DemoMeter("load-5v-1a");
    try {
      wasm.decodeAccept(meter.transact(wasm.connectRequest(1)));

      const uidReply = meter.transact(wasm.memoryReadRequest(2, UID_ADDR, UID_LEN));
      const uid = wasm.decodeMemoryRead(uidReply, UID_ADDR, UID_LEN);
      expect(uid).toBeInstanceOf(Uint8Array);
      expect(uid.length).toBe(UID_LEN);

      const now = BigInt(Date.now());
      const random = 0x1234_5678_9abc_def0n;
      const authReply = meter.transact(wasm.authRequest(3, 1, uid, now, random));
      expect(wasm.decodeAuthReply(authReply, uid, now, random)).toBe(1);

      wasm.decodeAccept(meter.transact(wasm.startQueueRequest(4, RATE_50_SPS)));
      meter.advance(500);
      const samples = wasm.decodeQueue(
        meter.transact(wasm.getDataRequest(5, ATT_ADC_QUEUE)),
        RATE_50_SPS,
      );
      expect(Array.isArray(samples)).toBe(true);
      expect(samples.length).toBeGreaterThanOrEqual(20);
      for (const s of samples) {
        expect(typeof s.time_ms).toBe("number");
        expect(s.vbus_v).toBeCloseTo(5, 0);
        expect(s.ibus_a).toBeCloseTo(1, 0);
        for (const k of ["cc1_v", "cc2_v", "dp_v", "dm_v"] as const) {
          expect(typeof s[k]).toBe("number");
        }
      }
    } finally {
      meter.free();
    }
  });

  it("throws an error with a kind on malformed or rejected input", () => {
    const rejected = thrown(() => wasm.decodeAccept(new Uint8Array([0x06, 0x01, 0x00, 0x00])));
    expect(rejected.kind).toBe("rejected");
    expect(typeof rejected.message).toBe("string");

    const short = thrown(() => wasm.decodeQueue(new Uint8Array([0x41]), RATE_50_SPS));
    expect(short.kind).toBe("malformed");
    expect(short.message.length).toBeGreaterThan(0);

    const scenario = thrown(() => new wasm.DemoMeter("no-such-scenario"));
    expect(scenario.kind).toBe("invalid");
  });
});
