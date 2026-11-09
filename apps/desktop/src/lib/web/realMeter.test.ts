/**
 * FR F3M587: a fake WebUSB KM003C that answers like the real meter (AC-F3M587.4), the
 * default fake meter of the web driver session tests (AC-F3M587.5).
 *
 * It wraps the wasm demo meter and reshapes its replies with the habits the web HIL
 * runs showed [device] (firmware 2.0.6):
 * - a stale DISCONNECT from before the session waits on bulk IN at open;
 * - after sign-in every reply header carries the auth level in bits 17..18 (byte 2
 *   gets `level << 1`);
 * - a memory read's 20-byte confirmation and its encrypted data come in separate IN
 *   transfers;
 * - the first queue poll after each queue start and the first PD poll after each
 *   sniffer start get a bare PUT_DATA (`41 id 02 00` at level 1: nothing new yet);
 * - queue replies come whole in one IN transfer, longer than 64 bytes at 50 SPS and
 *   up.
 *
 * Exported for the driver and web backend tests (the session over it runs in
 * `driver.realMeter.test.ts`); the test at the bottom checks the fake on its own.
 */

import { describe, expect, it } from "vitest";
import type { PowerzWasm } from "./driver";
import { FakeUsbDevice } from "./fakeDevices.test";
import { classify, loadWasm, type FrameName } from "./testWasm.test";

/** Type code of the auth reply. */
const AUTH_REPLY = 0x4c;
/** Length of the memory-read confirmation. */
const MEMORY_CONFIRMATION_LEN = 20;

export interface RealMeterOptions {
  /** The meter's serial number (the USB descriptor's). */
  serial?: string;
  /** The demo meter scenario behind the fake. */
  scenario?: string;
  /** While the queue streams, answer each ADC snapshot request with a bare PUT_DATA (no
   * ADC section). */
  noAdcWhileQueue?: boolean;
}

/** One request the fake meter got, with the IN transfers it answered. */
export interface RealMeterExchange {
  name: FrameName;
  request: Uint8Array;
  replies: Uint8Array[];
}

/** The fake real meter: a `FakeUsbDevice` that logs what it answered. */
export class RealMeterDevice extends FakeUsbDevice {
  /** Every request, in order, with the IN transfers that answered it. */
  exchanges: RealMeterExchange[] = [];
  /** The auth level the meter granted (stamped into every reply header after it). */
  level = 0;

  /** The replies to requests named `name`, joined per request. */
  repliesTo(name: FrameName): Uint8Array[] {
    return this.exchanges.filter((e) => e.name === name).map((e) => concat(e.replies));
  }
}

function concat(parts: Uint8Array[]): Uint8Array {
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let at = 0;
  for (const p of parts) {
    out.set(p, at);
    at += p.length;
  }
  return out;
}

/** A fake WebUSB KM003C that answers like the real meter (see the file comment). */
export function realMeterDevice(w: PowerzWasm, options: RealMeterOptions = {}): RealMeterDevice {
  const meter = new w.DemoMeter(options.scenario ?? "load-5v-1a");
  let last = Date.now();
  let queueRunning = false;
  let queueFresh = false;
  let pdFresh = false;
  const dev = new RealMeterDevice();
  const stamp = (reply: Uint8Array): Uint8Array => {
    const r = reply.slice();
    if (r.length >= 4) r[2] = (r[2] ?? 0) | (dev.level << 1);
    return r;
  };
  const empty = (id: number): Uint8Array => new Uint8Array([0x41, id, dev.level << 1, 0x00]);
  dev.respond = (frame) => {
    const now = Date.now();
    if (now > last) {
      meter.advance(now - last);
      last = now;
    }
    const name = classify(w, frame);
    const id = frame[1] ?? 0;
    const reply = meter.transact(frame);
    let replies: Uint8Array[];
    if (reply.length === 0) {
      replies = [];
    } else if (name === "auth") {
      replies = [reply];
      if ((reply[0] ?? 0) === AUTH_REPLY) dev.level = 1;
    } else if (name.startsWith("memory_read:") && reply.length > MEMORY_CONFIRMATION_LEN) {
      replies = [
        stamp(reply.slice(0, MEMORY_CONFIRMATION_LEN)),
        reply.slice(MEMORY_CONFIRMATION_LEN),
      ];
    } else if (name === "get_data:adc_queue" && queueFresh) {
      queueFresh = false;
      replies = [empty(id)];
    } else if ((name === "pd_poll" || name === "pd_poll_adc") && pdFresh) {
      pdFresh = false;
      replies = [empty(id)];
    } else if (name === "get_data:adc" && queueRunning && options.noAdcWhileQueue) {
      replies = [empty(id)];
    } else {
      replies = [stamp(reply)];
    }
    const accepted = (replies[0]?.[0] ?? 0) === 0x05;
    if (name.startsWith("start_queue:") && accepted) {
      queueRunning = true;
      queueFresh = true;
    }
    if (name === "stop_queue" && accepted) queueRunning = false;
    if (name === "start_pd" && accepted) pdFresh = true;
    dev.exchanges.push({ name, request: frame.slice(), replies });
    return replies.length > 0 ? replies : null;
  };
  dev.serialNumber = options.serial ?? "123456";
  (dev as unknown as { productName: string }).productName = "KM003C";
  // A DISCONNECT from before this session waits on bulk IN.
  dev.queueIn(new Uint8Array([0x03, 0x2a, 0x00, 0x00]));
  return dev;
}

describe("the real-meter fake", () => {
  it("has a stale frame waiting and splits a memory read after its confirmation", async () => {
    const w = await loadWasm();
    const dev = realMeterDevice(w);
    expect(dev.pendingIn).toBe(1);
    const stale = await dev.transferIn(6, 4096);
    expect(new Uint8Array(stale.data?.buffer ?? new ArrayBuffer(0))).toEqual(
      new Uint8Array([0x03, 0x2a, 0x00, 0x00]),
    );
    await dev.transferOut(5, new Uint8Array(w.connectRequest(1)));
    await dev.transferIn(6, 4096);
    await dev.transferOut(5, new Uint8Array(w.memoryReadRequest(2, 0x4001_0450, 12)));
    const confirmation = await dev.transferIn(6, 4096);
    const data = await dev.transferIn(6, 4096);
    expect(confirmation.data?.byteLength).toBe(20);
    expect(data.data?.byteLength).toBe(16);
    expect(dev.exchanges.map((e) => e.name)).toEqual(["connect", `memory_read:${0x4001_0450}`]);
  });
});
