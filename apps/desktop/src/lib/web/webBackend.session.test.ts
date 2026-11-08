/**
 * FR QFTCX0: the web backend's live session, over the demo meter and over a fake
 * WebUSB meter answered by the wasm demo meter.
 *
 * - AC-QFTCX0.3: connect → status and batches (samples, temperature, stats) like the
 *   desktop backend; a rate change restarts the queue; history ranges, gaps, range
 *   stats and stats reset over the in-browser history.
 * - AC-QFTCX0.4: passive PD capture start / stop, rows, message details, clear, and
 *   saving the capture downloads a pdcap file.
 * - AC-QFTCX0.5: a recording collects rows in memory and Stop downloads a CSV matching
 *   the desktop recorder's format for the same samples; session export downloads the
 *   whole history as CSV.
 *
 * Runs on Vitest's fake clock (setTimeout and Date).
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { BackendEvent, Batch, PdRow, Sample, Status } from "../backend";
import { CSV_HEADER, CsvRecorder } from "./csv";
import type { PowerzWasm } from "./driver";
import type { FakeUsbDevice } from "./fakeDevices.test";
import { loadWasm } from "./testWasm.test";
import { createWebBackend } from "./webBackend";
import {
  FakeUsbNavigator,
  demoUsbDevice,
  downloadSink,
  sentNames,
  settle,
  type Download,
} from "./webBackendFakes.test";

type Options = Parameters<typeof createWebBackend>[0];
type Backend = Awaited<ReturnType<typeof createWebBackend>>;

const EVENT_TYPES = [
  "batch",
  "gap",
  "status",
  "error",
  "recording",
  "pd_state",
  "pd_rows",
  "pd_status",
];

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

interface Run {
  backend: Backend;
  dev: FakeUsbDevice | null;
  id: string;
  events: BackendEvent[];
  downloads: Download[];
}

const batches = (events: BackendEvent[]): Batch[] =>
  events.flatMap((e) => (e.type === "batch" ? [e.data] : []));
const samplesOf = (events: BackendEvent[]): Sample[] => batches(events).flatMap((b) => b.samples);
const statuses = (events: BackendEvent[]): Status[] =>
  events.flatMap((e) => (e.type === "status" ? [e.data] : []));

/** Connects to a fake WebUSB meter running `scenario` at `rateSps`. */
async function connectUsb(scenario = "load-5v-1a", rateSps = 50): Promise<Run> {
  const nav = new FakeUsbNavigator();
  const dev = demoUsbDevice(w, "123456", scenario);
  nav.devices = [dev];
  const sink = downloadSink();
  const backend = await settle(
    createWebBackend({ wasm: w, usb: nav, download: sink.download } as unknown as Options),
  );
  const list = await settle(backend.listDevices());
  const id = list.find((d) => d.serial === "123456")?.id;
  if (!id) throw new Error("the permitted meter is not listed");
  const events: BackendEvent[] = [];
  await settle(backend.connect(id, rateSps, (e) => events.push(e)));
  return { backend, dev, id, events, downloads: sink.downloads };
}

/** Connects to the built-in demo meter at `rateSps`. */
async function connectDemo(rateSps = 50): Promise<Run> {
  const sink = downloadSink();
  const backend = await settle(
    createWebBackend({ wasm: w, usb: null, download: sink.download } as unknown as Options),
  );
  const demo = (await settle(backend.listDevices())).find((d) => d.demo_scenario !== null);
  if (!demo) throw new Error("no demo meter listed");
  const events: BackendEvent[] = [];
  await settle(backend.connect(demo.id, rateSps, (e) => events.push(e)));
  return { backend, dev: null, id: demo.id, events, downloads: sink.downloads };
}

describe("live session (AC-QFTCX0.3)", () => {
  it("the demo meter streams readings, temperature and stats", async () => {
    const run = await connectDemo(50);
    await vi.advanceTimersByTimeAsync(3_000);
    const connected = statuses(run.events).find((s) => s.state === "connected");
    expect(connected).toMatchObject({ state: "connected", rate_sps: 50, rate_locked: false });
    expect(connected && "model" in connected && connected.model.length).toBeGreaterThan(0);
    expect(run.events.every((e) => EVENT_TYPES.includes(e.type))).toBe(true);

    const all = samplesOf(run.events);
    expect(all.length).toBeGreaterThan(50);
    const last = all.at(-1);
    expect(last?.vbus_v).toBeCloseTo(5, 0);
    expect(last?.ibus_a).toBeCloseTo(1, 0);
    // Sample times rise along the session clock.
    expect(all.every((s, i) => i === 0 || s.t_ms >= (all[i - 1]?.t_ms ?? 0))).toBe(true);

    const temps = batches(run.events).map((b) => b.temp_c);
    expect(temps.some((t) => typeof t === "number" && t > 20 && t < 40)).toBe(true);

    const stats = batches(run.events).at(-1)?.stats;
    expect(stats?.samples).toBe(all.length);
    expect(stats?.vbus?.avg).toBeCloseTo(5, 0);
    expect(stats?.charge_ah).toBeGreaterThan(0);
    await settle(run.backend.stats()).then((s) => expect(s.samples).toBe(all.length));
    await settle(run.backend.disconnect());
  });

  it("a WebUSB meter streams over the queue and stops it on disconnect", async () => {
    const run = await connectUsb("load-5v-1a", 50);
    await vi.advanceTimersByTimeAsync(1_000);
    const names = sentNames(w, run.dev as FakeUsbDevice);
    expect(names).toContain("start_queue:2");
    expect(names).toContain("get_data:adc_queue");
    expect(samplesOf(run.events).length).toBeGreaterThan(0);
    await settle(run.backend.disconnect());
    expect(sentNames(w, run.dev as FakeUsbDevice)).toContain("stop_queue");
    expect(statuses(run.events).at(-1)).toMatchObject({ state: "disconnected" });
  });

  it("a rate change restarts the queue at the new rate", async () => {
    const run = await connectUsb("load-5v-1a", 50);
    await vi.advanceTimersByTimeAsync(500);
    const dev = run.dev as FakeUsbDevice;
    const before = sentNames(w, dev).length;
    await settle(run.backend.setRate(10));
    const after = sentNames(w, dev).slice(before);
    const stop = after.indexOf("stop_queue");
    const start = after.indexOf("start_queue:1");
    expect(stop).toBeGreaterThanOrEqual(0);
    expect(start).toBeGreaterThan(stop);
    expect(statuses(run.events)).toContainEqual({
      state: "rate",
      rate_sps: 10,
      rate_locked: false,
    });
    const count = samplesOf(run.events).length;
    await vi.advanceTimersByTimeAsync(1_000);
    expect(samplesOf(run.events).length).toBeGreaterThan(count);
    await settle(run.backend.disconnect());
  });

  it("history ranges, gaps, range stats and stats reset work over the browser history", async () => {
    const run = await connectDemo(50);
    await vi.advanceTimersByTimeAsync(3_000);
    const b = run.backend;
    const all = samplesOf(run.events);
    const t0 = all[0]?.t_ms ?? 0;
    const t1 = all.at(-1)?.t_ms ?? 0;

    const [vbus, ibus] = await settle(b.historyRange(["vbus", "ibus"] as const, t0, t1, 20));
    expect(vbus?.series).toBe("vbus");
    expect(ibus?.series).toBe("ibus");
    expect(vbus?.buckets.length).toBeGreaterThan(0);
    expect(vbus?.buckets.length).toBeLessThanOrEqual(22);
    for (const bucket of vbus?.buckets ?? []) {
      expect(bucket.min).toBeLessThanOrEqual(bucket.max);
      expect(bucket.min).toBeGreaterThan(4);
      expect(bucket.max).toBeLessThan(6);
    }

    expect(await settle(b.historyGaps(t0, t1))).toEqual([]);
    expect(await settle(b.historyFoldBoundary())).toBeNull();

    const range = await settle(b.rangeStats(t0, t1));
    expect(range?.samples).toBe(all.length);
    expect(range?.vbus_avg_v).toBeCloseTo(5, 0);
    expect(range?.ibus_avg_a).toBeCloseTo(1, 0);
    expect(range?.energy_wh).toBeGreaterThan(0);
    expect(await settle(b.rangeStats(t1 + 60_000, t1 + 120_000))).toBeNull();

    await settle(b.resetStats());
    const reset = await settle(b.stats());
    expect(reset.samples).toBe(0);
    expect(reset.charge_ah).toBe(0);
    // The history stays: only the stats restart.
    expect((await settle(b.rangeStats(t0, t1)))?.samples).toBe(all.length);

    // The per-meter commands reach the same session.
    const meter = b.meter(run.id);
    expect(await settle(meter.historyGaps(t0, t1))).toEqual([]);
    await settle(b.disconnect());
  });
});

describe("PD analyzer (AC-QFTCX0.4)", () => {
  it("captures passively, shows rows and details, clears and saves a pdcap file", async () => {
    const run = await connectUsb("pd-9v-2a", 50);
    const dev = run.dev as FakeUsbDevice;
    await settle(run.backend.pdStart());
    expect(sentNames(w, dev)).toContain("start_pd");
    await vi.advanceTimersByTimeAsync(3_000);
    expect(run.events).toContainEqual({ type: "pd_state", data: { capturing: true } });

    const rows: PdRow[] = run.events.flatMap((e) => (e.type === "pd_rows" ? e.data.rows : []));
    expect(rows.length).toBeGreaterThan(0);
    expect(rows.map((r) => r.index)).toEqual(rows.map((_, i) => i));
    const caps = rows.find((r) => r.kind === "message" && r.name === "Source_Capabilities");
    expect(caps).toBeDefined();
    expect(caps?.direction).toBe("source_to_sink");

    const details = await settle(run.backend.pdDetails(caps?.index ?? 0));
    expect(details.index).toBe(caps?.index);
    expect(details.raw.length).toBeGreaterThan(2);
    expect(details.fields.length).toBeGreaterThan(0);

    await settle(run.backend.pdStop());
    expect(sentNames(w, dev)).toContain("stop_pd");
    expect(run.events).toContainEqual({ type: "pd_state", data: { capturing: false } });

    const saved = await settle(run.backend.pdSave("capture.pdcap"));
    expect(saved).toMatchObject({ path: "capture.pdcap" });
    expect(saved?.rows).toBeGreaterThanOrEqual(rows.length);
    expect(run.downloads.map((d) => d.name)).toEqual(["capture.pdcap"]);
    const text = await (run.downloads[0] as Download).blob.text();
    const lines = text.split("\n").filter((l) => l.length > 0);
    expect(JSON.parse(lines[0] ?? "{}")).toMatchObject({ format: "pdcap", format_version: 1 });
    expect(lines.length).toBeGreaterThan(1);
    // The meter's serial number is in no field of the file.
    expect(text).not.toContain("123456");

    const cleared = await settle(run.backend.pdClear());
    expect(cleared.next_index).toBe(rows.length);
    expect(typeof cleared.status_floor_ms).toBe("number");
    await settle(run.backend.disconnect());
  });
});

describe("recording and export (AC-QFTCX0.5)", () => {
  it("Stop downloads a CSV matching the desktop recorder for the same samples", async () => {
    const run = await connectUsb("load-5v-1a", 50);
    await vi.advanceTimersByTimeAsync(500);
    // Start and stop need no timers: the batches between them are exactly the rows.
    const path = await run.backend.startRecording("session.csv");
    const from = run.events.length;
    await vi.advanceTimersByTimeAsync(2_000);
    const to = run.events.length;
    const summary = await run.backend.stopRecording();
    const recorded = run.events
      .slice(from, to)
      .flatMap((e) =>
        e.type === "batch" ? e.data.samples.map((sample) => ({ sample, temp: e.data.temp_c })) : [],
      );

    expect(path).toBe("session.csv");
    expect(run.events).toContainEqual({ type: "recording", data: { state: "started" } });
    expect(summary.ended).toBe("stopped");
    expect(summary.rows).toBe(recorded.length);
    expect(summary.rows).toBeGreaterThan(50);

    expect(run.downloads.map((d) => d.name)).toEqual(["session.csv"]);
    const text = await (run.downloads[0] as Download).blob.text();
    const lines = text.split("\n").filter((l) => l.length > 0);
    expect(lines[0]).toBe(CSV_HEADER);
    expect(lines.length - 1).toBe(summary.rows);

    const expected = new CsvRecorder();
    for (const r of recorded) expected.push(r.sample, 1_790_000_000_000, r.temp);
    const want = expected
      .text()
      .split("\n")
      .filter((l) => l.length > 0);
    const rows = lines.slice(1).map((l) => l.split(","));
    const wantRows = want.slice(1).map((l) => l.split(","));
    // Every column but the wall clock and the temperature (the live reading).
    const cols = [0, 2, 3, 4, 5, 6, 7, 9, 10, 11];
    expect(rows.map((r) => cols.map((c) => r[c]))).toEqual(
      wantRows.map((r) => cols.map((c) => r[c])),
    );
    for (const r of rows) {
      expect(r).toHaveLength(12);
      expect(r[1]).toMatch(/^\d+\.\d{3}$/);
      expect(r[8]).toMatch(/^(|-?\d+(\.\d+)?)$/);
    }
    await settle(run.backend.disconnect());
  });

  it("session export downloads the whole history as CSV", async () => {
    const run = await connectDemo(50);
    await vi.advanceTimersByTimeAsync(2_000);
    const total = samplesOf(run.events).length;
    const progress: { done: number; total: number }[] = [];
    const result = await settle(run.backend.exportSession("export.csv", (p) => progress.push(p)));
    expect(result).toMatchObject({ path: "export.csv" });
    expect(result?.rows).toBeGreaterThanOrEqual(total);
    expect(progress.at(-1)).toEqual({ done: result?.rows, total: result?.rows });

    expect(run.downloads.map((d) => d.name)).toEqual(["export.csv"]);
    const lines = (await (run.downloads[0] as Download).blob.text())
      .split("\n")
      .filter((l) => l.length > 0);
    expect(lines[0]).toBe(CSV_HEADER);
    expect(lines.length - 1).toBe(result?.rows);
    expect(lines[1]?.split(",")[0]).toBe("0:00:00.000");
    expect(lines[1]?.split(",")[2]).toMatch(/^[45](\.\d+)?$/);
    await settle(run.backend.disconnect());
  });
});
