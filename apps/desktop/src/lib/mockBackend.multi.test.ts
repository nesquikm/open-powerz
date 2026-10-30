/**
 * FR 1HHT03: the scripted mock backend runs several demo meters at once, like the
 * Tauri backend: one session per device id, a serial of its own for each simulated
 * meter after the first, and `meter(deviceId)` for the commands of one meter.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { BackendEvent, Batch } from "./backend";
import { MOCK_REPLUG, MockBackend } from "./mockBackend";

const LOAD = "demo:load-5v-1a";
const PD = "demo:pd-9v-2a";

function batches(events: BackendEvent[]): Batch[] {
  return events.flatMap((e) => (e.type === "batch" ? [e.data] : []));
}

function connectedSerial(events: BackendEvent[]): string | null {
  for (const e of events) {
    if (e.type === "status" && e.data.state === "connected") return e.data.serial;
  }
  return null;
}

function connectedSession(events: BackendEvent[]): number | null {
  for (const e of events) {
    if (e.type === "status" && e.data.state === "connected") return e.data.session_id;
  }
  return null;
}

function meanVbus(events: BackendEvent[]): number {
  const v = batches(events).flatMap((b) => b.samples.map((s) => s.vbus_v));
  expect(v.length).toBeGreaterThan(0);
  return v.reduce((a, b) => a + b, 0) / v.length;
}

describe("MockBackend with two meters (FR 1HHT03)", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  async function two(): Promise<{ backend: MockBackend; a: BackendEvent[]; b: BackendEvent[] }> {
    const backend = new MockBackend(null, { pdSynthetic: null });
    const a: BackendEvent[] = [];
    const b: BackendEvent[] = [];
    await backend.connect(LOAD, 50, (e) => a.push(e));
    await backend.connect(PD, 50, (e) => b.push(e));
    return { backend, a, b };
  }

  it("AC-1HHT03.1: connects a second meter, each with its own session and stream", async () => {
    const { backend, a, b } = await two();
    vi.advanceTimersByTime(1000);
    expect(backend.connectedIds()).toEqual([LOAD, PD]);
    expect(connectedSerial(a)).toBe("123456");
    expect(connectedSerial(b)).not.toBeNull();
    expect(connectedSerial(b)).not.toBe(connectedSerial(a));
    expect(connectedSession(b)).not.toBe(connectedSession(a));
    expect(meanVbus(a)).toBeCloseTo(5, 0);
    expect(meanVbus(b)).toBeCloseTo(9, 0);
    await backend.meter(LOAD).disconnect();
    await backend.meter(PD).disconnect();
  });

  it("AC-1HHT03.1: still refuses the same meter twice", async () => {
    const { backend } = await two();
    await expect(backend.connect(LOAD, 50, () => {})).rejects.toMatchObject({ kind: "usage" });
    expect(backend.connectedIds()).toEqual([LOAD, PD]);
  });

  it("AC-1HHT03.3: recording, rate and PD go only to the meter they name", async () => {
    const { backend, a, b } = await two();
    // The rate is set before recording: a recording locks it, as on the real backend.
    await backend.meter(PD).setRate(10);
    await backend.meter(PD).startRecording("pd.csv");
    await backend.meter(LOAD).pdStart();
    vi.advanceTimersByTime(1500);

    expect(b).toContainEqual({ type: "recording", data: { state: "started" } });
    expect(a.some((e) => e.type === "recording")).toBe(false);
    await expect(backend.meter(LOAD).stopRecording()).rejects.toMatchObject({ kind: "usage" });
    const summary = await backend.meter(PD).stopRecording();
    expect(summary.rows).toBeGreaterThan(0);

    expect(b).toContainEqual({
      type: "status",
      data: { state: "rate", rate_sps: 10, rate_locked: false },
    });
    expect(a.some((e) => e.type === "status" && e.data.state === "rate")).toBe(false);

    expect(a.some((e) => e.type === "pd_state")).toBe(true);
    expect(b.some((e) => e.type === "pd_state" || e.type === "pd_rows")).toBe(false);

    // Each meter keeps its own statistics. LOAD's capture plays its 9 V contract for
    // part of each cycle (AC-4PJRVK.5), so its average sits between 5 V and PD's 9 V.
    const loadAvg = (await backend.meter(LOAD).stats()).vbus?.avg;
    expect(loadAvg).toBeGreaterThan(4.5);
    expect(loadAvg).toBeLessThan(8);
    expect((await backend.meter(PD).stats()).vbus?.avg).toBeCloseTo(9, 0);
  });

  it("AC-1HHT03.3: a trigger session belongs to one meter", async () => {
    const { backend } = await two();
    await backend.meter(PD).triggerOpen();
    await expect(backend.meter(LOAD).triggerDetect()).rejects.toMatchObject({ kind: "usage" });
    await expect(backend.meter(PD).triggerDetect()).resolves.toMatchObject({
      apple: "not-supported",
    });
    await backend.meter(LOAD).disconnect();
    // Closing meter A left meter B's trigger session open.
    await expect(backend.meter(PD).triggerList("pd")).resolves.toMatchObject({
      protocol: "pd",
      supported: true,
    });
  });

  it("AC-1HHT03.4: ending one meter's session leaves the other streaming", async () => {
    const { backend, a, b } = await two();
    vi.advanceTimersByTime(500);
    await backend.meter(LOAD).disconnect();
    expect(a.at(-1)).toEqual({ type: "status", data: { state: "disconnected", error: null } });
    const before = batches(b).length;
    vi.advanceTimersByTime(1000);
    expect(batches(b).length).toBeGreaterThan(before + 10);
    expect(b.some((e) => e.type === "status" && e.data.state === "disconnected")).toBe(false);
    expect(backend.connectedIds()).toEqual([PD]);
    await expect(backend.meter(LOAD).setRate(10)).rejects.toMatchObject({ kind: "usage" });
  });

  it("AC-1HHT03.4: once the meter connected last leaves, the plain commands act on the one still running", async () => {
    const { backend, a } = await two();
    await backend.meter(PD).disconnect();
    await backend.setRate(10);
    expect(a).toContainEqual({
      type: "status",
      data: { state: "rate", rate_sps: 10, rate_locked: false },
    });
    vi.advanceTimersByTime(500);
    expect((await backend.stats()).vbus?.avg).toBeCloseTo(5, 0);
    await backend.disconnect();
    expect(a.at(-1)).toEqual({ type: "status", data: { state: "disconnected", error: null } });
  });

  it("AC-1HHT03.1: the plain single-meter commands still work with one meter", async () => {
    const backend = new MockBackend(null, { pdSynthetic: null });
    const events: BackendEvent[] = [];
    await backend.connect(LOAD, 50, (e) => events.push(e));
    await backend.setRate(10);
    await backend.disconnect();
    expect(events).toContainEqual({
      type: "status",
      data: { state: "rate", rate_sps: 10, rate_locked: false },
    });
    expect(events.at(-1)).toEqual({
      type: "status",
      data: { state: "disconnected", error: null },
    });
  });
});

describe("MockBackend with an unplugged meter beside another (review majors 1-3)", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  async function demoThenMeter(): Promise<{
    backend: MockBackend;
    meterId: string;
    demo: BackendEvent[];
    meter: BackendEvent[];
  }> {
    const backend = new MockBackend("replug", { pdSynthetic: null });
    const demo: BackendEvent[] = [];
    const meter: BackendEvent[] = [];
    await backend.connect(LOAD, 50, (e) => demo.push(e));
    const listed = (await backend.listDevices()).find((d) => d.demo_scenario === null);
    await backend.connect(listed!.id, 50, (e) => meter.push(e));
    return { backend, meterId: listed!.id, demo, meter };
  }

  const states = (events: BackendEvent[]): string[] =>
    events.flatMap((e) => (e.type === "status" ? [e.data.state] : []));

  it("an added meter that is unplugged waits and comes back while the first one streams", async () => {
    const { backend, meterId, demo, meter } = await demoThenMeter();
    vi.advanceTimersByTime(MOCK_REPLUG.beforeMs + 100);
    expect(states(meter).slice(-2)).toEqual(["disconnected", "waiting"]);
    vi.advanceTimersByTime(MOCK_REPLUG.awayMs + 100);
    expect(states(meter).at(-1)).toBe("connected");
    expect(states(demo)).toEqual(["connected"]);
    // The meter's commands still name it by the id it was connected with.
    await backend.meter(meterId).setRate(10);
    expect(meter.at(-1)).toEqual({
      type: "status",
      data: { state: "rate", rate_sps: 10, rate_locked: false },
    });
  });

  it("a meter added while another waits leaves that wait alone", async () => {
    const { backend, meter } = await demoThenMeter();
    vi.advanceTimersByTime(MOCK_REPLUG.beforeMs + 100);
    await backend.connect(PD, 50, () => {});
    vi.advanceTimersByTime(MOCK_REPLUG.awayMs + 100);
    expect(states(meter).at(-1)).toBe("connected");
  });

  it("a tile's cancel-wait ends only that meter's wait", async () => {
    const { backend, meterId, meter } = await demoThenMeter();
    vi.advanceTimersByTime(MOCK_REPLUG.beforeMs + 100);
    await backend.meter(meterId).cancelWait();
    vi.advanceTimersByTime(MOCK_REPLUG.awayMs + 100);
    expect(states(meter).at(-1)).toBe("waiting");
    expect(backend.connectedIds()).toEqual([LOAD]);
  });

  it("a reload attaches each running meter by its device id", async () => {
    const backend = new MockBackend(null, { pdSynthetic: null });
    await backend.connect(LOAD, 50, () => {});
    await backend.connect(PD, 50, () => {});
    const a: BackendEvent[] = [];
    const b: BackendEvent[] = [];
    expect((await backend.attach((e) => b.push(e)))?.device_id).toBe(PD);
    expect((await backend.attach((e) => a.push(e), LOAD))?.device_id).toBe(LOAD);
    await expect(backend.attach(() => {}, "demo:idle")).resolves.toBeNull();
    vi.advanceTimersByTime(500);
    expect(meanVbus(a)).toBeCloseTo(5, 0);
    expect(meanVbus(b)).toBeCloseTo(9, 0);
  });
});
