/**
 * FR AXHK2T (AC-AXHK2T.2, .4, .5): the demo backend integrates its raw samples as the
 * Rust backend does, for segment marks and E.D. (`runProgress`, current run only) and
 * for the ruler's edge parts (`rangeStats` counts `t0..t1` itself, never across an
 * outage). Checked against independent sums over the scenarios' own formulas.
 */

import { afterEach, describe, expect, it, vi } from "vitest";
import type { RunProgressQuery } from "./backend";
import { MOCK_OUTAGE, MOCK_RECORDING_FILE, MockBackend } from "./mockBackend";

/** The `load-5v-1a` demo reading at `tMs` (the scenario's formula). */
function load(tMs: number): { v: number; i: number } {
  const t = tMs / 1000;
  return { v: 5 + 0.01 * Math.sin(t * 7), i: 1 + 0.02 * Math.sin(t * 3) };
}

/** Running charge and energy of `load` at each whole ms `0..n-1`. */
function running(n: number): { ah: number[]; wh: number[] } {
  const ah = [0];
  const wh = [0];
  for (let t = 1; t < n; t++) {
    const a = load(t - 1);
    const b = load(t);
    const h = 1 / 3_600_000;
    ah.push(ah[t - 1]! + ((a.i + b.i) / 2) * h);
    wh.push(wh[t - 1]! + ((a.v * a.i + b.v * b.i) / 2) * h);
  }
  return { ah, wh };
}

/** Where `values` (one per ms) first reach `pct` of their last value. */
function firstReach(values: number[], pct: number): number {
  const target = (values[values.length - 1]! * pct) / 100;
  const k = values.findIndex((c) => c >= target);
  if (k <= 0) return Math.max(0, k);
  return k - 1 + (target - values[k - 1]!) / (values[k]! - values[k - 1]!);
}

const q = (over: Partial<RunProgressQuery> = {}): RunProgressQuery => ({
  kind: "charge",
  step_pct: 10,
  ed_points: 0,
  ...over,
});

let backend: MockBackend | null = null;
afterEach(async () => {
  await backend?.disconnect().catch(() => {});
  backend = null;
});

describe("MockBackend run progress over an opened recording (AC-AXHK2T.4, .5)", () => {
  it("marks each step where the file's running total first reaches it", async () => {
    backend = new MockBackend(null);
    await backend.openRecordingFile(() => {});
    const n = MOCK_RECORDING_FILE.rows;
    const ref = running(n);
    for (const kind of ["charge", "energy"] as const) {
      const p = await backend.recordingFileRunProgress(q({ kind }));
      expect(p).not.toBeNull();
      if (p === null) return;
      expect(p.samples).toBe(n);
      expect(p.marks_total_positive).toBe(true);
      expect(p.marks.map((m) => m.percent)).toEqual([10, 20, 30, 40, 50, 60, 70, 80, 90]);
      const values = kind === "charge" ? ref.ah : ref.wh;
      for (const m of p.marks) expect(m.t_ms).toBeCloseTo(firstReach(values, m.percent), 0);
    }
  });

  it("samples E.D. at evenly spaced times, clamped to the run", async () => {
    backend = new MockBackend(null);
    await backend.openRecordingFile(() => {});
    const last = MOCK_RECORDING_FILE.rows - 1;
    const p = await backend.recordingFileRunProgress(q({ ed_points: 5 }));
    expect(p?.energy_total_positive).toBe(true);
    expect(p?.ed_t_ms).toEqual([0, last / 4, last / 2, (3 * last) / 4, last]);
    expect(p?.ed_pct[0]).toBe(0);
    expect(p?.ed_pct[4]).toBeCloseTo(100, 9);
    const ref = running(MOCK_RECORDING_FILE.rows).wh;
    const mid = ref[Math.floor(last / 2)]!;
    expect(p?.ed_pct[2]).toBeCloseTo((100 * mid) / ref[last]!, 3);
    const window = await backend.recordingFileRunProgress(
      q({ ed_points: 3, ed_t0_ms: 99_999, ed_t1_ms: 1_000 }),
    );
    expect(window?.ed_t_ms).toEqual([1_000, (1_000 + last) / 2, last]);
  });

  it("refuses with no file open", async () => {
    backend = new MockBackend(null);
    await expect(backend.recordingFileRunProgress(q())).rejects.toMatchObject({
      kind: "usage",
    });
  });
});

describe("MockBackend run progress over a meter's session (AC-AXHK2T.4, .5)", () => {
  it("flags a total that is not positive and marks nothing", async () => {
    for (const device of ["demo:idle", "demo:noisy-negative"]) {
      backend = new MockBackend(null);
      await backend.connect(device, 1000, () => {});
      await backend.seedSyntheticHistory(5_000);
      const p = await backend.runProgress(q({ ed_points: 100 }));
      expect(p?.marks_total_positive, device).toBe(false);
      expect(p?.marks, device).toEqual([]);
      expect(p?.energy_total_positive, device).toBe(false);
      expect(p?.ed_t_ms, device).toEqual([]);
      expect(p?.ed_pct, device).toEqual([]);
      await backend.disconnect();
    }
  });

  it("counts only the run after the last outage", async () => {
    backend = new MockBackend("outage");
    await backend.connect("demo:load-5v-1a", 1000, () => {});
    const after = MOCK_OUTAGE.startMs + MOCK_OUTAGE.beforeMs + MOCK_OUTAGE.gapMs;
    await vi.waitFor(
      async () => {
        const p = await backend!.runProgress(q({ ed_points: 2, ed_t0_ms: 0, ed_scope: "run" }));
        expect(p?.run_t0_ms).toBeGreaterThanOrEqual(after);
        expect(p?.samples).toBeGreaterThan(1);
      },
      { timeout: 2_000, interval: 20 },
    );
    const p = await backend.runProgress(q({ ed_points: 2, ed_t0_ms: 0, ed_scope: "run" }));
    expect(p!.samples).toBeLessThan(MOCK_OUTAGE.beforeMs);
    expect(p!.ed_t_ms[0]).toBe(p!.run_t0_ms);
    for (const m of p!.marks) expect(m.t_ms).toBeGreaterThanOrEqual(after);
  });

  it("E.D. is a share of the whole session, flat across an outage (AC-AXHK2T.5)", async () => {
    backend = new MockBackend("outage");
    await backend.connect("demo:load-5v-1a", 1000, () => {});
    const end = MOCK_OUTAGE.startMs + MOCK_OUTAGE.beforeMs;
    const after = end + MOCK_OUTAGE.gapMs;
    await vi.waitFor(
      async () => {
        const p = await backend!.runProgress(q());
        expect(p?.run_t0_ms).toBeGreaterThanOrEqual(after);
        expect(p?.samples).toBeGreaterThan(100);
      },
      { timeout: 2_000, interval: 20 },
    );
    const run = await backend.runProgress(q({ ed_points: 0, ed_scope: "run" }));
    const p = await backend.runProgress(q({ ed_points: 5, ed_t0_ms: end - 10, ed_t1_ms: after }));
    expect(p!.energy_total_positive).toBe(true);
    // The session holds the 20 s before the outage and the run after it.
    expect(p!.ed_total_wh).toBeGreaterThan(run!.energy_wh * 2);
    expect(p!.ed_t_ms[0]).toBe(end - 10);
    // Inside the outage nothing is added: the share is flat until the run starts.
    expect(p!.ed_pct[1]).toBeCloseTo(p!.ed_pct[3]!, 9);
    expect(p!.ed_pct[1]).toBeGreaterThan(p!.ed_pct[0]!);
    expect(p!.ed_pct[3]).toBeLessThan(100);
    // The marks still follow the run alone.
    expect(p!.marks).toEqual(run!.marks);
    expect(p!.energy_wh).toBeCloseTo(run!.energy_wh, 12);
  });

  it("each meter answers from its own history", async () => {
    backend = new MockBackend(null);
    await backend.connect("demo:load-5v-1a", 1000, () => {});
    await backend.seedSyntheticHistory(4_000);
    const p = await backend.meter("demo:load-5v-1a").runProgress(q());
    expect(p?.marks).toHaveLength(9);
  });
});

describe("MockBackend ruler edge parts (AC-AXHK2T.1, .2)", () => {
  it("count the time from each cursor to its nearest sample", async () => {
    backend = new MockBackend(null);
    await backend.openRecordingFile(() => {});
    // Only the sample at 1001 ms is in range; a quarter and three quarters of a ms
    // reach towards its neighbours.
    const s = await backend.recordingFileRangeStats(1_000.25, 1_001.75);
    expect(s?.samples).toBe(1);
    expect(s!.charge_ah).toBeGreaterThan(0);
    const hours = 1.5 / 3_600_000;
    expect(s!.charge_ah / hours).toBeCloseTo(load(1_001).i, 3);
  });

  it("add up over neighbouring ranges", async () => {
    backend = new MockBackend(null);
    await backend.openRecordingFile(() => {});
    const whole = await backend.recordingFileRangeStats(100.3, 9_000.6);
    const a = await backend.recordingFileRangeStats(100.3, 4_321.5);
    const b = await backend.recordingFileRangeStats(4_321.5, 9_000.6);
    expect(a!.charge_ah + b!.charge_ah).toBeCloseTo(whole!.charge_ah, 12);
    expect(a!.energy_wh + b!.energy_wh).toBeCloseTo(whole!.energy_wh, 12);
  });

  it("never reach across an outage", async () => {
    backend = new MockBackend("outage");
    await backend.connect("demo:load-5v-1a", 1000, () => {});
    const end = MOCK_OUTAGE.startMs + MOCK_OUTAGE.beforeMs;
    const after = end + MOCK_OUTAGE.gapMs;
    await vi.waitFor(
      async () => expect((await backend!.runProgress(q()))?.run_t0_ms).toBeGreaterThan(end),
      { timeout: 2_000, interval: 20 },
    );
    // From the last second before the outage into the outage: no part after it.
    const into = await backend.rangeStats(end - 1_000, after - 1_000);
    const before = await backend.rangeStats(end - 1_000, end);
    expect(into!.charge_ah).toBeCloseTo(before!.charge_ah, 12);
    expect(into!.charge_ah).toBeLessThan(0.0003);
  });
});
