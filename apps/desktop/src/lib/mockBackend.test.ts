import { readFileSync } from "node:fs";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { BackendEvent, Batch } from "./backend";
import {
  LINUX_PERMISSION_DENIED,
  MAX_SYNTHETIC_SAMPLES,
  MockBackend,
  mockModeFromUrl,
  syntheticFromUrl,
} from "./mockBackend";

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

  it("reports the snapshot temperature while streaming at 1000 SPS, like the worker (AC-X2SPDW.6)", async () => {
    const backend = new MockBackend();
    const events: BackendEvent[] = [];
    await backend.connect("demo:idle", 1000, (e) => events.push(e));
    vi.advanceTimersByTime(1500);
    await backend.disconnect();
    const temps = batches(events).map((b) => b.temp_c);
    expect(temps.length).toBeGreaterThan(0);
    expect(temps.at(-1)).toBeTypeOf("number");
    for (const t of temps.filter((t): t is number => t !== null)) {
      expect(t).toBeGreaterThan(25);
      expect(t).toBeLessThan(35);
    }
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

// ---- FR 0P7HSJ: the mock backend keeps in step with the recording commands ----

describe("MockBackend recording and export (AC-0P7HSJ.3, AC-0P7HSJ.5, AC-0P7HSJ.7)", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("records the samples streamed between start and stop", async () => {
    const backend = new MockBackend();
    const events: BackendEvent[] = [];
    await backend.connect("demo:load-5v-1a", 50, (e) => events.push(e));
    vi.advanceTimersByTime(500);
    const before = batches(events).flatMap((b) => b.samples).length;
    await expect(backend.startRecording("powerz-20261021-090507.csv")).resolves.toBe(
      "powerz-20261021-090507.csv",
    );
    vi.advanceTimersByTime(2000);
    const summary = await backend.stopRecording();
    const after = batches(events).flatMap((b) => b.samples).length;
    await backend.disconnect();

    expect(summary.rows).toBe(after - before);
    expect(summary.gaps).toEqual([]);
    expect(summary.disconnected).toBe(false);
    const progress = events.flatMap((e) =>
      e.type === "recording" && e.data.state === "progress" ? [e.data] : [],
    );
    expect(progress.length).toBeGreaterThanOrEqual(2);
    expect(progress.at(-1)?.rows).toBeLessThanOrEqual(summary.rows);
  });

  it("refuses to record without a session", async () => {
    await expect(new MockBackend().startRecording("x.csv")).rejects.toMatchObject({
      kind: "usage",
    });
    await expect(new MockBackend().stopRecording()).rejects.toMatchObject({ kind: "usage" });
  });

  it("exports the session with progress up to every sample", async () => {
    const backend = new MockBackend();
    const events: BackendEvent[] = [];
    await backend.connect("demo:idle", 50, (e) => events.push(e));
    vi.advanceTimersByTime(1000);
    const total = batches(events).flatMap((b) => b.samples).length;
    const progress: { done: number; total: number }[] = [];
    const result = backend.exportSession("powerz-x.csv", (p) => progress.push(p));
    await vi.runOnlyPendingTimersAsync();
    const done = await result;
    await backend.disconnect();
    expect(done?.path).toBe("powerz-x.csv");
    expect(done?.rows).toBeGreaterThanOrEqual(total);
    expect(progress.at(-1)).toEqual({ done: done?.rows, total: done?.rows });
  });
});

// ---- FR AH2AEJ: the mock backend keeps in step with the history and stats commands ----

describe("MockBackend history and stats (AC-AH2AEJ.2, AC-AH2AEJ.4, AC-AH2AEJ.5, AC-AH2AEJ.6)", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("sends stats with every batch, at least 4 times a second", async () => {
    const backend = new MockBackend();
    const events: BackendEvent[] = [];
    await backend.connect("demo:noisy-negative", 50, (e) => events.push(e));
    vi.advanceTimersByTime(1000);
    const got = batches(events);
    expect(got.length).toBeGreaterThanOrEqual(4);
    let seen = 0;
    for (const b of got) {
      seen += b.samples.length;
      expect(b.stats.samples).toBe(seen);
    }
    const last = got.at(-1)!.stats;
    // The scenario draws negative current: charge and energy are signed.
    expect(last.charge_ah).toBeLessThan(0);
    expect(last.energy_wh).toBeLessThan(0);
    expect(last.ibus?.avg).toBeLessThan(0);
    expect(last.vbus?.avg).toBeCloseTo(5, 0);
    expect(await backend.stats()).toEqual(last);
    await backend.disconnect();
  });

  it("resets the stats from the next sample on and keeps the history", async () => {
    const backend = new MockBackend();
    const events: BackendEvent[] = [];
    await backend.connect("demo:load-5v-1a", 50, (e) => events.push(e));
    vi.advanceTimersByTime(1000);
    await backend.resetStats();
    const zero = await backend.stats();
    expect(zero).toMatchObject({ samples: 0, elapsed_ms: 0, charge_ah: 0, energy_wh: 0 });
    expect(zero.vbus).toBeNull();
    const [whole] = await backend.historyRange(["vbus"], 0, Number.MAX_SAFE_INTEGER, 100);
    expect(whole.series).toBe("vbus");
    expect(whole.buckets.length).toBeGreaterThan(0);
    vi.advanceTimersByTime(1000);
    const after = await backend.stats();
    expect(after.samples).toBe(50);
    expect(after.charge_ah).toBeGreaterThan(0);
    await backend.disconnect();
  });

  it("answers history ranges with at most the requested buckets", async () => {
    const backend = new MockBackend();
    const events: BackendEvent[] = [];
    await backend.connect("demo:pd-9v-2a", 1000, (e) => events.push(e));
    vi.advanceTimersByTime(2000);
    const samples = batches(events).flatMap((b) => b.samples);
    const got = await backend.historyRange(["vbus", "ibus"], 0, Number.MAX_SAFE_INTEGER, 50);
    expect(got.map((s) => s.series)).toEqual(["vbus", "ibus"]);
    for (const s of got) {
      expect(s.buckets.length).toBeGreaterThan(0);
      expect(s.buckets.length).toBeLessThanOrEqual(50);
    }
    const lo = Math.min(...got[0].buckets.map((b) => b.min));
    const hi = Math.max(...got[0].buckets.map((b) => b.max));
    expect(lo).toBeCloseTo(Math.min(...samples.map((s) => s.vbus_v)), 5);
    expect(hi).toBeCloseTo(Math.max(...samples.map((s) => s.vbus_v)), 5);
    await backend.disconnect();
  });

  it("clears the history", async () => {
    const backend = new MockBackend();
    await backend.connect("demo:idle", 50, () => {});
    vi.advanceTimersByTime(1000);
    await backend.clearHistory();
    const [vbus] = await backend.historyRange(["vbus"], 0, Number.MAX_SAFE_INTEGER, 100);
    expect(vbus.buckets).toEqual([]);
    await backend.disconnect();
  });
});

describe("MockBackend synthetic history (AC-AH2AEM.4)", () => {
  it("serves a 10-million-sample history fast enough for a pan step", async () => {
    const backend = new MockBackend();
    await backend.connect("demo:load-5v-1a", 1000, () => {});
    await backend.seedSyntheticHistory(10_000_000);

    const [whole] = await backend.historyRange(["vbus"], 0, Number.MAX_SAFE_INTEGER, 1200);
    expect(whole.buckets.length).toBeLessThanOrEqual(1200);
    expect(whole.buckets.length).toBeGreaterThanOrEqual(1190);
    expect(whole.buckets[0]?.t_ms).toBe(0);
    expect(whole.buckets.at(-1)?.t_ms).toBeGreaterThan(9_990_000 - 10_000);

    // A pan step over a tenth of the history (one million samples).
    const started = performance.now();
    const [vbus, ibus] = await backend.historyRange(["vbus", "ibus"], 4_000_000, 5_000_000, 1200);
    const elapsed = performance.now() - started;
    await backend.disconnect();

    expect(elapsed).toBeLessThan(100);
    expect(vbus.buckets.length).toBeGreaterThanOrEqual(1190);
    expect(vbus.buckets.length).toBeLessThanOrEqual(1200);
    expect(vbus.buckets[0]?.t_ms).toBeGreaterThanOrEqual(4_000_000);
    expect(vbus.buckets.at(-1)?.t_ms).toBeLessThanOrEqual(5_000_000);
    for (const b of vbus.buckets) {
      expect(b.min).toBeLessThanOrEqual(b.max);
      expect(b.min).toBeCloseTo(5, 0);
    }
    for (const b of ibus.buckets) expect(b.max).toBeCloseTo(1, 0);
  });

  it("refuses to seed without a session, like every other history call", async () => {
    await expect(new MockBackend().seedSyntheticHistory(1_000)).rejects.toMatchObject({
      kind: "usage",
    });
  });
});

describe("MockBackend synthetic seed limits (AC-AH2AEM.4)", () => {
  it("honours ?synthetic= only in dev builds", () => {
    expect(syntheticFromUrl("?synthetic=5000", true)).toBe(5_000);
    expect(syntheticFromUrl("?synthetic=5000", false)).toBeNull();
    expect(syntheticFromUrl("?synthetic=abc", true)).toBeNull();
    expect(syntheticFromUrl(undefined, true)).toBeNull();
  });

  it("clamps the seed size", () => {
    expect(MAX_SYNTHETIC_SAMPLES).toBeLessThanOrEqual(20_000_000);
    expect(MAX_SYNTHETIC_SAMPLES).toBeGreaterThanOrEqual(10_000_000);
    expect(syntheticFromUrl("?synthetic=999999999999", true)).toBe(MAX_SYNTHETIC_SAMPLES);
  });

  it("seeds the same values a live sample would carry", async () => {
    const backend = new MockBackend();
    await backend.connect("demo:pd-9v-2a", 1000, () => {});
    await backend.seedSyntheticHistory(5_000);
    const [vbus, cc1] = await backend.historyRange(["vbus", "cc1"], 1_234, 1_234, 1);
    await backend.disconnect();
    expect(vbus.buckets[0]?.min).toBeCloseTo(9 + 0.02 * Math.sin(1.234 * 5), 5);
    expect(cc1.buckets[0]?.max).toBeCloseTo(1.05, 5);
  });
});

describe("MockBackend ?mock= scenarios (AC-AH2AEN.5)", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("reads the mode from the page URL and ignores unknown ones", () => {
    expect(mockModeFromUrl("?mock=permission-denied")).toBe("permission-denied");
    expect(mockModeFromUrl("?synthetic=10&mock=rate-locked")).toBe("rate-locked");
    expect(mockModeFromUrl("?mock=nope")).toBeNull();
    expect(mockModeFromUrl("")).toBeNull();
    expect(mockModeFromUrl(undefined)).toBeNull();
  });

  it("keeps the normal device list in every mode", async () => {
    const normal = await new MockBackend(null).listDevices();
    expect(await new MockBackend("permission-denied").listDevices()).toEqual(normal);
    expect(await new MockBackend("rate-locked").listDevices()).toEqual(normal);
  });

  it("permission-denied: connect rejects with the Linux permission error and udev hint", async () => {
    const backend = new MockBackend("permission-denied");
    const events: BackendEvent[] = [];
    await expect(backend.connect("demo:load-5v-1a", 50, (e) => events.push(e))).rejects.toEqual({
      kind: "permission_denied",
      message: "The system did not allow the app to open the meter.",
      hint:
        "Install the udev rule: `sudo cp packaging/linux/60-powerz.rules /etc/udev/rules.d/`, then run " +
        "`sudo udevadm control --reload-rules` and `sudo udevadm trigger`, and replug the meter " +
        "(see `docs/linux.md`).",
    });
    expect(events).toEqual([]);
    await expect(backend.stats()).rejects.toMatchObject({ kind: "usage" });
  });

  it("rate-locked: reports a locked session at the meter's 50 SPS and refuses rate changes", async () => {
    const backend = new MockBackend("rate-locked");
    const events: BackendEvent[] = [];
    await backend.connect("demo:load-5v-1a", 1000, (e) => events.push(e));
    expect(events[0]).toMatchObject({
      type: "status",
      data: { state: "connected", rate_sps: null, rate_locked: true },
    });
    expect(events[1]).toEqual({
      type: "status",
      data: { state: "rate", rate_sps: 50, rate_locked: true },
    });
    await expect(backend.setRate(10)).rejects.toMatchObject({ kind: "usage" });
    vi.advanceTimersByTime(1000);
    await backend.disconnect();
    expect(batches(events).flatMap((b) => b.samples)).toHaveLength(50);
  });
  it("slow-export: an export stays half done for a long while, then finishes", async () => {
    expect(mockModeFromUrl("?mock=slow-export")).toBe("slow-export");
    const backend = new MockBackend("slow-export");
    await backend.connect("demo:idle", 50, () => {});
    vi.advanceTimersByTime(1000);
    const progress: { done: number; total: number }[] = [];
    let finished = false;
    const result = backend.exportSession("powerz-x.csv", (p) => progress.push(p));
    void result.then(() => (finished = true));
    await vi.advanceTimersByTimeAsync(10_000);
    expect(finished).toBe(false);
    const half = progress.at(-1);
    expect(half && half.total > 0 && half.done > 0 && half.done < half.total).toBe(true);
    await vi.advanceTimersByTimeAsync(60_000);
    const done = await result;
    expect(progress.at(-1)).toEqual({ done: done?.rows, total: done?.rows });
    await backend.disconnect();
  });
});

describe("LINUX_PERMISSION_DENIED (the mock's copy of the backend's Linux permission error)", () => {
  /** `src-tauri/src/ui_error.rs`, the source of truth for the wording. */
  const rust = readFileSync(new URL("../../src-tauri/src/ui_error.rs", import.meta.url), "utf8");

  /** A Rust string literal's value: `\` + newline continues the line, dropping the indent. */
  function rustString(literal: string): string {
    return literal.replace(/\\\r?\n\s*/g, "");
  }

  it("has the hint of ui_error.rs", () => {
    const m = /const LINUX_PERMISSION_HINT: &str = "((?:[^"\\]|\\[\s\S])*)";/.exec(rust);
    expect(m?.[1], "LINUX_PERMISSION_HINT in ui_error.rs").toBeDefined();
    expect(LINUX_PERMISSION_DENIED.hint).toBe(rustString(m?.[1] ?? ""));
  });

  it("has the message and kind of ui_error.rs", () => {
    const m = /fn permission_denied\([\s\S]*?message: "([^"]+)"/.exec(rust);
    expect(m?.[1], "permission_denied() message in ui_error.rs").toBeDefined();
    expect(LINUX_PERMISSION_DENIED.message).toBe(m?.[1]);
    expect(LINUX_PERMISSION_DENIED.kind).toBe("permission_denied");
  });
});

// ---- Review: totals and min/max/avg start together (screenshot timing, not a bug) ----

describe("stats start together (milestone review)", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("0.6 s into a session every field counts the same samples; the totals are just still tiny", async () => {
    const { statsView } = await import("./stats");
    const backend = new MockBackend();
    await backend.connect("demo:noisy-negative", 1000, () => {});
    // The capture script shoots demo-negative about 600 ms after Connect.
    vi.advanceTimersByTime(600);
    const s = await backend.stats();
    await backend.disconnect();

    // One sample stream feeds every field: min/max/avg are filled exactly when samples > 0,
    // and elapsed spans those same samples.
    expect(s.samples).toBeGreaterThan(500);
    expect(s.vbus).not.toBeNull();
    expect(s.elapsed_ms).toBeGreaterThan(500);
    expect(s.elapsed_ms).toBeLessThan(1000);
    // At about -0.2 A and -1 W, 0.6 s is ~-3e-5 Ah and ~-2e-4 Wh: below the 3-decimal display.
    expect(Math.abs(s.charge_ah)).toBeLessThan(0.0005);
    expect(s.charge_ah).toBeLessThan(0);
    expect(s.energy_wh).toBeLessThan(0);
    const view = statsView(s);
    expect(view.elapsed).toBe("0:00:00");
    expect(view.charge).toBe("0.000 Ah");
    expect(view.energy).toBe("0.000 Wh");
  });
});

describe("MockBackend ?mock=outage (AC-272WSN.3, AC-272WSN.4)", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("is a known mode", () => {
    expect(mockModeFromUrl("?mock=outage")).toBe("outage");
  });

  it("starts at a long meter uptime, keeps a stretch before a 40 min outage, and says where the gap is", async () => {
    const { MOCK_OUTAGE } = await import("./mockBackend");
    const backend = new MockBackend("outage");
    const events: BackendEvent[] = [];
    await backend.connect("demo:load-5v-1a", 50, (e) => events.push(e));
    vi.advanceTimersByTime(500);
    const end = MOCK_OUTAGE.startMs + MOCK_OUTAGE.beforeMs;
    const resumed = end + MOCK_OUTAGE.gapMs;
    const gaps = await backend.historyGaps(0, Number.MAX_SAFE_INTEGER);
    expect(gaps).toHaveLength(1);
    expect(gaps[0]?.to_ms).toBe(resumed);
    expect(gaps[0]?.from_ms).toBeLessThan(end);
    expect(gaps[0]?.from_ms).toBeGreaterThanOrEqual(end - 20);
    expect(MOCK_OUTAGE.gapMs).toBe(2_400_000);
    expect(await backend.historyGaps(0, MOCK_OUTAGE.startMs)).toEqual([]);

    const [vbus] = await backend.historyRange(["vbus"], 0, Number.MAX_SAFE_INTEGER, 100);
    expect(vbus.buckets[0]?.t_ms).toBe(MOCK_OUTAGE.startMs);
    // The live stream goes on after the gap, which the backend announced first.
    const first = batches(events).find((b) => b.samples.length > 0)?.samples[0];
    expect(first?.t_ms).toBe(resumed);
    const gapAt = events.findIndex((e) => e.type === "gap");
    const batchAt = events.findIndex((e) => e.type === "batch" && e.data.samples.length > 0);
    expect(gapAt).toBeGreaterThanOrEqual(0);
    expect(gapAt).toBeLessThan(batchAt);
    expect(events[gapAt]).toEqual({ type: "gap", data: { kind: "outage", ...gaps[0] } });
    await backend.disconnect();
  });

  it("other modes have no gap", async () => {
    const backend = new MockBackend(null);
    await backend.connect("demo:load-5v-1a", 50, () => {});
    vi.advanceTimersByTime(500);
    expect(await backend.historyGaps(0, Number.MAX_SAFE_INTEGER)).toEqual([]);
    await backend.disconnect();
  });
});

// ---- FR VEDDFD: a new page re-attaches to the running session ----

describe("MockBackend attach (AC-VEDDFD.1–AC-VEDDFD.5)", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("answers null with no session running (AC-VEDDFD.1)", async () => {
    const backend = new MockBackend(null);
    const events: BackendEvent[] = [];
    await expect(backend.attach((e) => events.push(e))).resolves.toBeNull();
    vi.advanceTimersByTime(500);
    expect(events).toEqual([]);
    // Nor once the session has ended.
    await backend.connect("demo:load-5v-1a", 50, () => {});
    await backend.disconnect();
    await expect(backend.attach(() => {})).resolves.toBeNull();
  });

  it("returns the running session's snapshot without reconnecting (AC-VEDDFD.2)", async () => {
    const backend = new MockBackend(null);
    await backend.connect("demo:load-5v-1a", 50, () => {});
    vi.advanceTimersByTime(500);
    const events: BackendEvent[] = [];
    await expect(backend.attach((e) => events.push(e))).resolves.toEqual({
      device_id: "demo:load-5v-1a",
      model: "KM003C",
      firmware_version: "2.0.6",
      serial: "123456",
      rate_sps: 50,
      rate_locked: false,
      session_id: 1,
      recording: null,
    });
    vi.advanceTimersByTime(500);
    await backend.disconnect();
    expect(events.some((e) => e.type === "status" && e.data.state === "connected")).toBe(false);
    expect(batches(events).flatMap((b) => b.samples).length).toBeGreaterThan(0);
  });

  it("sends every later event to the new page only (AC-VEDDFD.5)", async () => {
    const backend = new MockBackend(null);
    const old: BackendEvent[] = [];
    await backend.connect("demo:load-5v-1a", 50, (e) => old.push(e));
    vi.advanceTimersByTime(500);
    const fresh: BackendEvent[] = [];
    await backend.attach((e) => fresh.push(e));
    const oldCount = old.length;
    vi.advanceTimersByTime(1000);
    await backend.setRate(10);
    await backend.disconnect();
    expect(old.length, "the old page still got events").toBe(oldCount);
    // The stream goes on from where it was: the new page's first sample follows the old page's last.
    const oldLast =
      batches(old)
        .flatMap((b) => b.samples)
        .at(-1)?.t_ms ?? -1;
    const freshFirst = batches(fresh).flatMap((b) => b.samples)[0]?.t_ms ?? -1;
    expect(freshFirst).toBeGreaterThan(oldLast);
    expect(fresh).toContainEqual({
      type: "status",
      data: { state: "rate", rate_sps: 10, rate_locked: false },
    });
    expect(fresh.at(-1)).toEqual({
      type: "status",
      data: { state: "disconnected", error: null },
    });
  });

  it("carries the open recording's rows and elapsed time (AC-VEDDFD.3)", async () => {
    const backend = new MockBackend(null);
    await backend.connect("demo:load-5v-1a", 50, () => {});
    await backend.startRecording("powerz-x.csv");
    vi.advanceTimersByTime(2000);
    const fresh: BackendEvent[] = [];
    const snap = await backend.attach((e) => fresh.push(e));
    expect(snap?.recording?.elapsed_ms).toBe(2000);
    expect(snap?.recording?.rows).toBe(100);
    // The recording runs on and Stop ends it on the re-attached session.
    vi.advanceTimersByTime(1000);
    const progress = fresh.flatMap((e) =>
      e.type === "recording" && e.data.state === "progress" ? [e.data] : [],
    );
    expect(progress.at(-1)?.elapsed_ms).toBeGreaterThan(2000);
    const summary = await backend.stopRecording();
    expect(summary.rows).toBe(150);
    await backend.disconnect();
  });

  it("reports a changed rate and a locked rate as they are (AC-VEDDFD.2)", async () => {
    const backend = new MockBackend(null);
    await backend.connect("demo:pd-9v-2a", 50, () => {});
    await backend.setRate(1000);
    const snap = await backend.attach(() => {});
    expect(snap).toMatchObject({ device_id: "demo:pd-9v-2a", rate_sps: 1000, rate_locked: false });
    await backend.disconnect();

    const locked = new MockBackend("rate-locked");
    await locked.connect("demo:load-5v-1a", 1000, () => {});
    const lockedSnap = await locked.attach(() => {});
    expect(lockedSnap).toMatchObject({ rate_sps: 50, rate_locked: true });
    await locked.disconnect();
  });

  it("keeps the history and stats from before the attach (AC-VEDDFD.4)", async () => {
    const backend = new MockBackend(null);
    await backend.connect("demo:load-5v-1a", 50, () => {});
    vi.advanceTimersByTime(1000);
    await backend.attach(() => {});
    vi.advanceTimersByTime(1000);
    const [vbus] = await backend.historyRange(["vbus"] as const, 0, 1e12, 10_000);
    expect(vbus.buckets[0]?.t_ms).toBe(0);
    expect((await backend.stats()).samples).toBe(100);
    await backend.disconnect();
  });
});

describe("MockBackend rate change during a recording (AC-JFD9XR.2)", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("refuses a rate change while recording, as the real backend does, and keeps the rate", async () => {
    const backend = new MockBackend();
    const events: BackendEvent[] = [];
    await backend.connect("demo:load-5v-1a", 50, (e) => events.push(e));
    await backend.startRecording("powerz-x.csv");
    vi.advanceTimersByTime(500);
    await expect(backend.setRate(1000)).rejects.toEqual({
      kind: "usage",
      message: "Stop the recording to change the rate.",
      hint: null,
    });
    expect(events).not.toContainEqual({
      type: "status",
      data: { state: "rate", rate_sps: 1000, rate_locked: false },
    });
    // Still 50 SPS: about 50 samples a second.
    const before = batches(events).flatMap((b) => b.samples).length;
    vi.advanceTimersByTime(1000);
    const perSecond = batches(events).flatMap((b) => b.samples).length - before;
    expect(perSecond).toBeGreaterThanOrEqual(40);
    expect(perSecond).toBeLessThanOrEqual(60);
    // Once stopped, the rate changes again.
    await backend.stopRecording();
    await backend.setRate(1000);
    expect(events).toContainEqual({
      type: "status",
      data: { state: "rate", rate_sps: 1000, rate_locked: false },
    });
    await backend.disconnect();
  });
});

describe("MockBackend history Power (AC-JFD9XR.4)", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("answers a power series with the per-sample VBUS x IBUS extremes", async () => {
    const backend = new MockBackend();
    const events: BackendEvent[] = [];
    await backend.connect("demo:noisy-negative", 50, (e) => events.push(e));
    vi.advanceTimersByTime(2000);
    const samples = batches(events).flatMap((b) => b.samples);
    const [power] = await backend.historyRange(["power"], 0, Number.MAX_SAFE_INTEGER, 1);
    await backend.disconnect();
    expect(power.series).toBe("power");
    expect(power.buckets).toHaveLength(1);
    const products = samples.map((x) => x.vbus_v * x.ibus_a);
    expect(power.buckets[0]?.min).toBeCloseTo(Math.min(...products), 3);
    expect(power.buckets[0]?.max).toBeCloseTo(Math.max(...products), 3);
  });
});

describe("MockBackend reveal (AC-9CFMD3.1, AC-9CFMD3.3)", () => {
  it("records each reveal request, in order", async () => {
    const backend = new MockBackend();
    expect(backend.revealedPaths()).toEqual([]);
    await backend.revealPath("/tmp/export.csv");
    await backend.revealPath("powerz-20261021-090507.csv");
    expect(backend.revealedPaths()).toEqual(["/tmp/export.csv", "powerz-20261021-090507.csv"]);
  });
});

describe("MockBackend history edge samples (AC-9CFMD3.4)", () => {
  it("answers a view between two samples with the samples just outside it", async () => {
    const backend = new MockBackend();
    await backend.connect("demo:load-5v-1a", 1000, () => {});
    await backend.seedSyntheticHistory(5_000);
    const [vbus] = await backend.historyRange(["vbus"], 1_234.3, 1_234.7, 800);
    await backend.disconnect();
    expect(vbus.buckets.map((b) => b.t_ms)).toEqual([1_234, 1_235]);
  });

  it("adds one sample beyond each edge of a wider view", async () => {
    const backend = new MockBackend();
    await backend.connect("demo:load-5v-1a", 1000, () => {});
    await backend.seedSyntheticHistory(5_000);
    const [vbus] = await backend.historyRange(["vbus"], 100.5, 109.5, 800);
    await backend.disconnect();
    const times = vbus.buckets.map((b) => b.t_ms);
    expect(times[0]).toBe(100);
    expect(times.at(-1)).toBe(110);
    expect(times).toHaveLength(11);
  });
});
