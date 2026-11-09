/**
 * FR QFTCWZ: the built wasm module the web tests run against, plus the frame classifier
 * the closed-command-set test (AC-QFTCWZ.5) and the driver tests use.
 *
 * This file is itself a test file (Vitest picks it up) so it can live next to the
 * other `*.test.ts` files without being shipped; it exports helpers and checks that
 * the classifier knows the builders.
 */

import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import type { PowerzWasm } from "./driver";

const GLUE_URL = new URL("../../wasm/powerz_wasm.js", import.meta.url);
const MODULE_URL = new URL("../../wasm/powerz_wasm_bg.wasm", import.meta.url);

let cached: PowerzWasm | null = null;

/** The generated glue, initialised once per test file. */
export async function loadWasm(): Promise<PowerzWasm> {
  if (cached) return cached;
  const glue = fileURLToPath(GLUE_URL);
  if (!existsSync(glue)) throw new Error(`${glue} is missing: run bun run build:wasm first`);
  const mod = (await import(/* @vite-ignore */ GLUE_URL.href)) as {
    initSync(o: { module: Uint8Array }): unknown;
  };
  mod.initSync({ module: readFileSync(fileURLToPath(MODULE_URL)) });
  cached = mod as unknown as PowerzWasm;
  return cached;
}

/** The four memory blocks on the safe list (hardware, firmware, OTP, UID). */
export const SAFE_BLOCKS: readonly (readonly [number, number])[] = [
  [0x0000_0420, 64],
  [0x0000_4420, 64],
  [0x0300_0c00, 64],
  [0x4001_0450, 12],
];

/** GET_DATA attribute masks a read-only session may ask for. */
export const READ_MASKS = {
  adc: 0x001,
  adc_queue: 0x002,
  settings: 0x008,
  pd_packet: 0x010,
  pd_status: 0x020,
} as const;

export type FrameName =
  | "connect"
  | "auth"
  | `get_data:${keyof typeof READ_MASKS}`
  | `start_queue:${number}`
  | "stop_queue"
  | "start_pd"
  | "stop_pd"
  | "pd_poll"
  | "pd_poll_adc"
  | `memory_read:${number}`
  | "unknown";

function same(a: Uint8Array, b: Uint8Array): boolean {
  return a.length === b.length && a.every((v, i) => v === b[i]);
}

/** Names `frame` by rebuilding every safe request with the frame's id byte. */
export function classify(w: PowerzWasm, frame: Uint8Array): FrameName {
  const id = frame[1] ?? 0;
  if (same(frame, w.connectRequest(id))) return "connect";
  for (const [name, mask] of Object.entries(READ_MASKS)) {
    if (same(frame, w.getDataRequest(id, mask))) return `get_data:${name}` as FrameName;
  }
  for (let code = 0; code <= 3; code++) {
    if (same(frame, w.startQueueRequest(id, code))) return `start_queue:${code}`;
  }
  if (same(frame, w.stopQueueRequest(id))) return "stop_queue";
  if (same(frame, w.startPdSnifferRequest(id))) return "start_pd";
  if (same(frame, w.stopPdSnifferRequest(id))) return "stop_pd";
  if (same(frame, w.pdPollRequest(id, false))) return "pd_poll";
  if (same(frame, w.pdPollRequest(id, true))) return "pd_poll_adc";
  for (const [addr, len] of SAFE_BLOCKS) {
    if (same(frame, w.memoryReadRequest(id, addr, len))) return `memory_read:${addr}`;
  }
  // Auth carries time and random, so only its type byte and length are fixed.
  const authShape = w.authRequest(0, 1, new Uint8Array(12), 0n, 0n);
  if (frame.length === authShape.length && frame[0] === authShape[0]) return "auth";
  return "unknown";
}

describe("test wasm helpers", () => {
  it("classifies each builder's output by name", async () => {
    const w = await loadWasm();
    expect(classify(w, w.connectRequest(7))).toBe("connect");
    expect(classify(w, w.getDataRequest(9, READ_MASKS.adc_queue))).toBe("get_data:adc_queue");
    expect(classify(w, w.startQueueRequest(3, 2))).toBe("start_queue:2");
    expect(classify(w, w.memoryReadRequest(4, 0x4001_0450, 12))).toBe(`memory_read:${0x4001_0450}`);
    expect(classify(w, new Uint8Array([0x48, 1, 0, 0]))).toBe("unknown");
  });
});
