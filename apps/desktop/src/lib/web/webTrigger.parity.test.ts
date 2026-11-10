/**
 * FR W8SHXC: a trigger session on a real meter changes the meter's measurement session
 * as the desktop's does (`install_trigger_for`, `pause_during_list` / `PollingHeld`):
 *
 * - a running PD capture stops first, with the reason `trigger` (the meter refuses a
 *   capture during PDM [device]);
 * - a queue stream above 50 SPS is capped to 50 SPS, and the earlier rate comes back
 *   when the trigger session closes (not after a failed open, not at 50 SPS or below);
 * - the stream's polling pauses while the automatic list (Detect) runs, and resumes
 *   however it ends.
 *
 * Runs on Vitest's fake clock against a fake WebUSB meter, a fake Web Serial port and
 * a fake engine worker.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { BackendEvent, UiError } from "../backend";
import type { PowerzWasm } from "./driver";
import { FakeUsbDevice } from "./fakeDevices.test";
import { FakeSerial, FakeSerialPort, demoMeterWithTwin } from "./fakeSerial.test";
import { classify, loadWasm } from "./testWasm.test";
import { createWebBackend } from "./webBackend";
import { FakeUsbNavigator, downloadSink, settle } from "./webBackendFakes.test";

type Options = Parameters<typeof createWebBackend>[0];
type Backend = Awaited<ReturnType<typeof createWebBackend>>;

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

type Posted = { id: number; method: string; args: unknown[] };
type Listener = (e: { data: unknown }) => void;

/** An engine worker that answers every call, `detect` only once `release()` runs. */
class ParityWorker {
  posted: Posted[] = [];
  failOpen = false;
  private listeners = new Set<Listener>();
  private held: (() => void)[] = [];
  holdDetect = false;

  postMessage(data: unknown): void {
    const m = data as Posted;
    this.posted.push(m);
    const answer = () => {
      const body =
        m.method === "open" && this.failOpen
          ? { error: { kind: "timeout", message: "No answer.", hint: null } }
          : m.method === "detect"
            ? { ok: { hint: null, bc12: null, apple: null, protocols: [] } }
            : { ok: null };
      for (const l of this.listeners) l({ data: { id: m.id, ...body } });
    };
    if (m.method === "detect" && this.holdDetect) this.held.push(answer);
    else void Promise.resolve().then(answer);
  }

  release(): void {
    for (const a of this.held.splice(0)) a();
  }

  addEventListener(type: string, fn: Listener): void {
    if (type === "message") this.listeners.add(fn);
  }

  terminate(): void {}
}

interface Run {
  backend: Backend;
  dev: FakeUsbDevice;
  events: BackendEvent[];
  workers: ParityWorker[];
}

async function realMeter(rateSps: number, worker?: (w: ParityWorker) => void): Promise<Run> {
  const meter = demoMeterWithTwin(w);
  let last = Date.now();
  const dev = new FakeUsbDevice((frame) => {
    const now = Date.now();
    if (now > last) {
      meter.advance(now - last);
      last = now;
    }
    const reply = meter.transact(frame);
    return reply.length ? reply : null;
  });
  dev.serialNumber = "123456";
  (dev as unknown as { productName: string }).productName = "KM003C";
  const usb = new FakeUsbNavigator();
  usb.devices = [dev];
  const serial = new FakeSerial();
  serial.pick = new FakeSerialPort(() => []);
  const workers: ParityWorker[] = [];
  const backend = await settle(
    createWebBackend({
      wasm: w,
      usb,
      hid: null,
      serial,
      download: downloadSink().download,
      crossOriginIsolated: true,
      pageEvents: new EventTarget(),
      triggerWorker: () => {
        const fake = new ParityWorker();
        worker?.(fake);
        workers.push(fake);
        return fake;
      },
    } as unknown as Options),
  );
  const id = (await settle(backend.listDevices())).find((d) => d.serial === "123456")?.id;
  if (!id) throw new Error("the meter is not listed");
  const events: BackendEvent[] = [];
  await settle(backend.connect(id, rateSps, (e) => events.push(e)));
  await vi.advanceTimersByTimeAsync(300);
  return { backend, dev, events, workers };
}

const frames = (dev: FakeUsbDevice, from = 0) =>
  dev.outTransfers.slice(from).map((t) => classify(w, t.data));

const rates = (events: BackendEvent[]) =>
  events.flatMap((e) =>
    e.type === "status" && e.data.state === "rate"
      ? [(e.data as { rate_sps: number }).rate_sps]
      : [],
  );

const pdStates = (events: BackendEvent[]) =>
  events.flatMap((e) => (e.type === "pd_state" ? [e.data] : []));

describe("a trigger session stops a running PD capture first", () => {
  it("stops the sniffer with the reason trigger before the engine opens", async () => {
    const { backend, dev, events, workers } = await realMeter(50);
    await settle(backend.pdStart());
    await vi.advanceTimersByTimeAsync(300);
    expect(frames(dev)).toContain("start_pd");
    await settle(backend.triggerOpen());
    expect(frames(dev)).toContain("stop_pd");
    expect(pdStates(events).at(-1)).toEqual({ capturing: false, reason: "trigger" });
    expect(workers[0]?.posted[0]?.method).toBe("open");
    const after = dev.outTransfers.length;
    await vi.advanceTimersByTimeAsync(500);
    expect(frames(dev, after).filter((f) => f.startsWith("pd_poll"))).toEqual([]);
    await settle(backend.triggerClose());
    await settle(backend.disconnect());
  });

  it("sends no stop without a running capture", async () => {
    const { backend, dev, events } = await realMeter(50);
    await settle(backend.triggerOpen());
    expect(frames(dev)).not.toContain("stop_pd");
    expect(pdStates(events)).toEqual([]);
    await settle(backend.triggerClose());
    await settle(backend.disconnect());
  });
});

describe("a trigger session caps the queue at 50 SPS", () => {
  it("caps 1000 SPS to 50 SPS and restores 1000 SPS on close", async () => {
    const { backend, dev, events } = await realMeter(1000);
    const before = dev.outTransfers.length;
    await settle(backend.triggerOpen());
    expect(frames(dev, before)).toContain("start_queue:2");
    expect(rates(events)).toEqual([50]);
    const mid = dev.outTransfers.length;
    await settle(backend.triggerClose());
    expect(frames(dev, mid)).toContain("start_queue:3");
    expect(rates(events)).toEqual([50, 1000]);
    await settle(backend.disconnect());
  });

  it("leaves 50 SPS or less as it is", async () => {
    const { backend, dev, events } = await realMeter(10);
    const before = dev.outTransfers.length;
    await settle(backend.triggerOpen());
    await settle(backend.triggerClose());
    expect(frames(dev, before).filter((f) => f.startsWith("start_queue"))).toEqual([]);
    expect(rates(events)).toEqual([]);
    await settle(backend.disconnect());
  });

  it("a failed open changes no rate", async () => {
    const { backend, events } = await realMeter(1000, (fake) => {
      fake.failOpen = true;
    });
    let error: UiError | null = null;
    try {
      await settle(backend.triggerOpen());
    } catch (e) {
      error = e as UiError;
    }
    expect(error).not.toBeNull();
    expect(rates(events)).toEqual([]);
    await settle(backend.disconnect());
  });
});

describe("the stream's polling pauses while the automatic list runs", () => {
  it("polls nothing during Detect and resumes after it", async () => {
    const { backend, dev, workers } = await realMeter(50, (fake) => {
      fake.holdDetect = true;
    });
    await settle(backend.triggerOpen());
    const detect = backend.triggerDetect();
    await vi.advanceTimersByTimeAsync(200);
    const during = dev.outTransfers.length;
    await vi.advanceTimersByTimeAsync(1_000);
    expect(frames(dev, during).filter((f) => f === "get_data:adc_queue")).toEqual([]);
    workers[0]?.release();
    await settle(detect);
    const after = dev.outTransfers.length;
    await vi.advanceTimersByTimeAsync(500);
    expect(frames(dev, after).filter((f) => f === "get_data:adc_queue").length).toBeGreaterThan(0);
    await settle(backend.triggerClose());
    await settle(backend.disconnect());
  });
});

describe("a rate above 50 SPS is refused while the trigger panel is open", () => {
  const refusal = "The rate stays at 50 samples/s or less while the trigger panel is open.";

  async function rateError(backend: Backend, rateSps: number): Promise<UiError | null> {
    try {
      await settle(backend.setRate(rateSps));
      return null;
    } catch (e) {
      return e as UiError;
    }
  }

  it("refuses 1000 SPS with the desktop's usage and sends nothing", async () => {
    const { backend, dev, events } = await realMeter(50);
    await settle(backend.triggerOpen());
    const before = dev.outTransfers.length;
    const error = await rateError(backend, 1000);
    expect(error?.kind).toBe("usage");
    expect(error?.message).toBe(refusal);
    expect(frames(dev, before).filter((f) => f.startsWith("start_queue"))).toEqual([]);
    expect(rates(events)).toEqual([]);
    await settle(backend.triggerClose());
    await settle(backend.disconnect());
  });

  it("takes 50 SPS or less while open, and 1000 SPS again after the close", async () => {
    const { backend, events } = await realMeter(50);
    await settle(backend.triggerOpen());
    expect(await rateError(backend, 10)).toBeNull();
    expect(rates(events)).toEqual([10]);
    await settle(backend.triggerClose());
    expect(await rateError(backend, 1000)).toBeNull();
    expect(rates(events)).toEqual([10, 1000]);
    await settle(backend.disconnect());
  });

  it("allows 1000 SPS with no trigger session open", async () => {
    const { backend, events } = await realMeter(50);
    expect(await rateError(backend, 1000)).toBeNull();
    expect(rates(events)).toEqual([1000]);
    await settle(backend.disconnect());
  });
});
