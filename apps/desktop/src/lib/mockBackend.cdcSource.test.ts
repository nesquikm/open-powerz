/** The demo backend's CDC measurement source (FR 1HHT02, AC .4): the demo meter
 * streams through either source, and the mock records which one a session used. */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { BackendEvent, Batch } from "./backend";
import { MockBackend } from "./mockBackend";

function batches(events: BackendEvent[]): Batch[] {
  return events.flatMap((e) => (e.type === "batch" ? [e.data] : []));
}

describe("MockBackend measurement source (AC-1HHT02.4)", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("streams the scenario's values through the CDC source", async () => {
    const b = new MockBackend();
    const events: BackendEvent[] = [];
    await b.connect("demo:load-5v-1a", 50, (e) => events.push(e), "cdc");
    expect(b.connectedSource()).toBe("cdc");
    vi.advanceTimersByTime(1000);
    await b.disconnect();
    expect(events[0]).toMatchObject({ type: "status", data: { state: "connected" } });
    const samples = batches(events).flatMap((x) => x.samples);
    expect(samples.length).toBeGreaterThanOrEqual(45);
    expect(samples.length).toBeLessThanOrEqual(55);
    for (const s of samples) expect(Math.abs(s.vbus_v - 5)).toBeLessThan(0.1);
  });

  it("uses the queue when no source is given", async () => {
    const b = new MockBackend();
    expect(b.connectedSource()).toBeNull();
    await b.connect("demo:load-5v-1a", 50, () => {});
    expect(b.connectedSource()).toBe("queue");
    await b.disconnect();
    expect(b.connectedSource()).toBeNull();
  });
});
