/**
 * FR JQ971R, AC .4: like the backend, the mock backend stops a running PD capture
 * with the reason "trigger" when Detect opens a trigger session; a stop the user
 * asked for and a disconnect carry no reason.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { BackendEvent } from "./backend";
import { MockBackend } from "./mockBackend";

type PdStateData = { capturing: boolean; reason?: string };

function pdStates(events: BackendEvent[]): PdStateData[] {
  return events.flatMap((e) => (e.type === "pd_state" ? [e.data as PdStateData] : []));
}

async function capturing(): Promise<{ backend: MockBackend; events: BackendEvent[] }> {
  const backend = new MockBackend(null, { pdSynthetic: null });
  const events: BackendEvent[] = [];
  await backend.connect("demo:load-5v-1a", 50, (e) => events.push(e));
  await backend.pdStart();
  vi.advanceTimersByTime(100);
  return { backend, events };
}

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

describe("the mock's PD stop reason (AC-JQ971R.4)", () => {
  it("opening the trigger session for Detect stops the capture with the reason trigger", async () => {
    const { backend, events } = await capturing();
    await backend.triggerOpen();
    await backend.triggerDetect();
    const last = pdStates(events).at(-1);
    expect(last).toEqual({ capturing: false, reason: "trigger" });
    await backend.disconnect();
  });

  it("a stop the user asked for carries no reason", async () => {
    const { backend, events } = await capturing();
    await backend.pdStop();
    const last = pdStates(events).at(-1)!;
    expect(last.capturing).toBe(false);
    expect("reason" in last).toBe(false);
    await backend.disconnect();
  });

  it("a disconnect while capturing carries no reason", async () => {
    const { backend, events } = await capturing();
    await backend.disconnect();
    const stops = pdStates(events).filter((s) => !s.capturing);
    expect(stops.length).toBeGreaterThan(0);
    for (const s of stops) expect("reason" in s).toBe(false);
  });

  it("a trigger session without a running capture sends no stop", async () => {
    const backend = new MockBackend(null, { pdSynthetic: null });
    const events: BackendEvent[] = [];
    await backend.connect("demo:load-5v-1a", 50, (e) => events.push(e));
    await backend.triggerOpen();
    expect(pdStates(events)).toEqual([]);
    await backend.disconnect();
  });
});
