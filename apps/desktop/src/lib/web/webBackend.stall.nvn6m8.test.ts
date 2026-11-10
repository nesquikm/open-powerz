/**
 * FR NVN6M8 (AC-NVN6M8.1, AC-NVN6M8.2): the web backend passes the driver's queue stall
 * on as `stall` events, the same event the desktop backend sends: `{ stalled: true }`
 * after 2 s of empty queue polls, `{ stalled: false }` once samples come back.
 *
 * Runs on Vitest's fake clock (setTimeout and Date).
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { BackendEvent } from "../backend";
import type { PowerzWasm } from "./driver";
import { realMeterDevice } from "./realMeter.test";
import { classify, loadWasm } from "./testWasm.test";
import { createWebBackend } from "./webBackend";
import { FakeUsbNavigator, downloadSink, settle } from "./webBackendFakes.test";

type Options = Parameters<typeof createWebBackend>[0];

let w: PowerzWasm;

beforeEach(async () => {
  w = await loadWasm();
  vi.useFakeTimers({
    toFake: ["setTimeout", "clearTimeout", "setInterval", "clearInterval", "Date"],
  });
  vi.setSystemTime(1_790_000_000_000);
});

afterEach(() => {
  vi.useRealTimers();
});

/** The `stall` events among `events`, as their `stalled` flags. */
const stallFlags = (events: BackendEvent[]): unknown[] =>
  events.flatMap((e) =>
    (e as { type: string }).type === "stall"
      ? [(e as unknown as { data: { stalled: unknown } }).data.stalled]
      : [],
  );

describe("the web backend reports a stalled queue (AC-NVN6M8.1, AC-NVN6M8.2)", () => {
  it("nvn6m8: emits stall true after 2 s of empty polls and stall false when samples return", async () => {
    const nav = new FakeUsbNavigator();
    const dev = realMeterDevice(w, { serial: "123456" });
    let stalled = false;
    const inner = dev.respond;
    dev.respond = (frame) =>
      stalled && classify(w, frame) === "get_data:adc_queue"
        ? [new Uint8Array([0x41, frame[1] ?? 0, 0x02, 0x00])]
        : inner(frame);
    nav.devices = [dev];
    const backend = await settle(
      createWebBackend({
        wasm: w,
        usb: nav,
        download: downloadSink().download,
      } as unknown as Options),
    );
    const id = (await settle(backend.listDevices())).find((d) => d.serial === "123456")?.id;
    if (!id) throw new Error("the permitted meter is not listed");
    const events: BackendEvent[] = [];
    await settle(backend.connect(id, 50, (e) => events.push(e)));

    await vi.advanceTimersByTimeAsync(1_500);
    expect(stallFlags(events)).toEqual([]);

    stalled = true;
    await vi.advanceTimersByTimeAsync(4_000);
    expect(stallFlags(events)).toEqual([true]);
    const stall = events.find((e) => (e as { type: string }).type === "stall");
    expect(stall).toEqual({ type: "stall", data: { stalled: true } });

    stalled = false;
    await vi.advanceTimersByTimeAsync(1_500);
    expect(stallFlags(events)).toEqual([true, false]);
    // Samples flow again after the stall ended.
    const last = events.findLastIndex((e) => (e as { type: string }).type === "stall");
    expect(
      events.slice(last + 1).some((e) => e.type === "batch" && e.data.samples.length > 0),
    ).toBe(true);
  });

  it("nvn6m8: the demo meter's stream reports no stall", async () => {
    const backend = await settle(
      createWebBackend({
        wasm: w,
        usb: null,
        download: downloadSink().download,
      } as unknown as Options),
    );
    const demo = (await settle(backend.listDevices())).find((d) => d.demo_scenario !== null);
    if (!demo) throw new Error("no demo meter listed");
    const events: BackendEvent[] = [];
    await settle(backend.connect(demo.id, 2, (e) => events.push(e)));
    await vi.advanceTimersByTimeAsync(10_000);
    expect(stallFlags(events)).toEqual([]);
  });
});
