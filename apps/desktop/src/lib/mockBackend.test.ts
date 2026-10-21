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
        "Install the udev rule: sudo cp packaging/linux/60-powerz.rules /etc/udev/rules.d/, then run " +
        "sudo udevadm control --reload-rules and sudo udevadm trigger, and replug the meter " +
        "(see docs/linux.md).",
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
