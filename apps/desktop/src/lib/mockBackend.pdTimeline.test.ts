/**
 * FR TGE83D (AC .1, .4): the mock backend's PD capture on the chart's session time
 * base, which the Playwright suite drives. Like the real backend, every PD row carries
 * `session_ms` on the measurement samples' `t_ms` base, and `pd_status` events carry the
 * PD status blocks (VBUS, IBUS, CC1, CC2) while the capture runs.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { BackendEvent, PdRow, PdStatusSample } from "./backend";
import { MockBackend } from "./mockBackend";

function pdRows(events: BackendEvent[]): PdRow[] {
  return events.flatMap((e) => (e.type === "pd_rows" ? e.data.rows : []));
}

function statusSamples(events: BackendEvent[]): PdStatusSample[] {
  return events.flatMap((e) => (e.type === "pd_status" ? e.data.samples : []));
}

function sampleTimes(events: BackendEvent[]): number[] {
  return events.flatMap((e) => (e.type === "batch" ? e.data.samples.map((s) => s.t_ms) : []));
}

/** Streams for `leadMs`, then captures PD for `ms`. */
async function capturing(
  leadMs: number,
  ms: number,
): Promise<{ backend: MockBackend; events: BackendEvent[] }> {
  const backend = new MockBackend();
  const events: BackendEvent[] = [];
  await backend.connect("demo:load-5v-1a", 50, (e) => events.push(e));
  vi.advanceTimersByTime(leadMs);
  await backend.pdStart();
  vi.advanceTimersByTime(ms);
  return { backend, events };
}

describe("MockBackend PD capture on the session time base (AC-TGE83D.1)", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("rows carry session_ms inside the streamed samples' time range", async () => {
    const { backend, events } = await capturing(3_000, 3_000);
    await backend.disconnect();
    const rows = pdRows(events);
    expect(rows.length).toBeGreaterThan(0);
    const times = sampleTimes(events);
    const first = Math.min(...times);
    const last = Math.max(...times);
    for (const r of rows) {
      expect(r.session_ms).toBeGreaterThanOrEqual(first);
      expect(r.session_ms).toBeLessThanOrEqual(last + 100);
    }
    // The capture started 3 s into the session, so the first record sits there too.
    expect(rows[0]!.session_ms).toBeGreaterThanOrEqual(first + 2_900);
    // One capture run: a constant offset from the time since the capture start.
    const offsets = new Set(rows.map((r) => r.session_ms! - r.elapsed_ms));
    expect(offsets.size).toBe(1);
  });
});

describe("MockBackend PD status blocks (AC-TGE83D.4)", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("sends VBUS, IBUS, CC1 and CC2 status samples while the capture runs", async () => {
    const { backend, events } = await capturing(1_000, 2_000);
    const samples = statusSamples(events);
    expect(samples.length).toBeGreaterThan(10);
    const times = sampleTimes(events);
    const first = Math.min(...times);
    const last = Math.max(...times);
    for (const s of samples) {
      expect(s.session_ms).toBeGreaterThanOrEqual(first);
      expect(s.session_ms).toBeLessThanOrEqual(last + 100);
      expect(Number.isFinite(s.vbus_v)).toBe(true);
      expect(Number.isFinite(s.ibus_a)).toBe(true);
      expect(Number.isFinite(s.cc1_v)).toBe(true);
      expect(Number.isFinite(s.cc2_v)).toBe(true);
    }
    const t = samples.map((s) => s.session_ms);
    expect(t).toEqual([...t].sort((a, b) => a - b));
    // The demo device is a 5 V source with a sink attached on CC1.
    expect(samples.some((s) => Math.abs(s.vbus_v - 5) < 0.3)).toBe(true);
    expect(samples.some((s) => s.cc1_v > 0.2)).toBe(true);
    // At most 60 events a second.
    expect(events.filter((e) => e.type === "pd_status").length).toBeLessThanOrEqual(120);

    await backend.pdStop();
    const n = statusSamples(events).length;
    vi.advanceTimersByTime(1_000);
    expect(statusSamples(events).length).toBe(n);
    await backend.disconnect();
  });

  it("sends no status before the capture starts", async () => {
    const backend = new MockBackend();
    const events: BackendEvent[] = [];
    await backend.connect("demo:load-5v-1a", 50, (e) => events.push(e));
    vi.advanceTimersByTime(1_000);
    expect(statusSamples(events)).toEqual([]);
    await backend.disconnect();
  });
});
