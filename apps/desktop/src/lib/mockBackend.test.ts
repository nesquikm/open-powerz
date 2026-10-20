import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { BackendEvent, Batch } from "./backend";
import { MockBackend } from "./mockBackend";

function batches(events: BackendEvent[]): Batch[] {
  return events.flatMap((e) => (e.type === "batch" ? [e.data] : []));
}

describe("MockBackend (scripted demo backend for the browser build)", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("lists one demo entry per simulator scenario", async () => {
    const ids = (await new MockBackend().listDevices()).map((d) => d.id);
    expect(ids).toEqual(["demo:idle", "demo:load-5v-1a", "demo:pd-9v-2a", "demo:noisy-negative"]);
  });

  it("streams the scenario's values in batches at 20–60 Hz", async () => {
    const backend = new MockBackend();
    const events: BackendEvent[] = [];
    await backend.connect("demo:load-5v-1a", 50, (e) => events.push(e));
    vi.advanceTimersByTime(1000);
    await backend.disconnect();

    expect(events[0]).toMatchObject({ type: "status", data: { state: "connected", rate_sps: 50 } });
    const got = batches(events);
    expect(got.length).toBeGreaterThanOrEqual(20);
    expect(got.length).toBeLessThanOrEqual(60);
    const samples = got.flatMap((b) => b.samples);
    expect(samples.length).toBe(50);
    for (const s of samples) {
      expect(s.vbus_v).toBeCloseTo(5, 0);
      expect(s.ibus_a).toBeCloseTo(1, 0);
    }
    expect(got.at(-1)?.temp_c).toBeTypeOf("number");
    expect(events.at(-1)).toEqual({ type: "status", data: { state: "disconnected", error: null } });
  });

  it("reports no temperature while streaming at 1000 SPS, like the firmware", async () => {
    const backend = new MockBackend();
    const events: BackendEvent[] = [];
    await backend.connect("demo:idle", 1000, (e) => events.push(e));
    vi.advanceTimersByTime(500);
    await backend.disconnect();
    expect(batches(events).every((b) => b.temp_c === null)).toBe(true);
  });

  it("changes the rate of a running session and refuses a second connect", async () => {
    const backend = new MockBackend();
    const events: BackendEvent[] = [];
    await backend.connect("demo:pd-9v-2a", 10, (e) => events.push(e));
    await expect(backend.connect("demo:idle", 10, () => {})).rejects.toMatchObject({
      kind: "usage",
    });
    await backend.setRate(1000);
    expect(events).toContainEqual({
      type: "status",
      data: { state: "rate", rate_sps: 1000, rate_locked: false },
    });
    await backend.disconnect();
    await expect(backend.setRate(50)).rejects.toMatchObject({ kind: "usage" });
  });

  it("stops sending once disconnected", async () => {
    const backend = new MockBackend();
    const events: BackendEvent[] = [];
    await backend.connect("demo:idle", 50, (e) => events.push(e));
    await backend.disconnect();
    const count = events.length;
    vi.advanceTimersByTime(2000);
    expect(events.length).toBe(count);
  });
});
