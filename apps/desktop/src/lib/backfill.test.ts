import { describe, expect, it } from "vitest";
import type { Bucket, Sample, Series, SeriesBuckets } from "./backend";
import {
  BACKFILL_BUCKETS,
  BACKFILL_SERIES,
  backfillRange,
  backfillSamples,
  fetchBackfill,
} from "./backfill";
import { LIVE_WINDOWS, LiveBuffer, liveRange } from "./chartData";

/** Buckets of `series` at `times`, min = base + i, max = base + i + 0.5. */
function buckets(series: Series, times: number[], base: number): SeriesBuckets {
  const out: Bucket[] = times.map((t_ms, i) => ({ t_ms, min: base + i, max: base + i + 0.5 }));
  return { series, buckets: out };
}

/** A history answer for every backfill series over the same bucket times. */
function answer(times: number[]): SeriesBuckets[] {
  const bases: Record<Series, number> = { vbus: 5, ibus: 1, dp: 0.6, dm: 0.3, cc1: 1.6, cc2: 0 };
  return BACKFILL_SERIES.map((s) => buckets(s, times, bases[s]));
}

describe("backfill (FR VEDDFD)", () => {
  it("asks for every stored channel the live buffer keeps (AC-VEDDFD.6)", () => {
    expect([...BACKFILL_SERIES].sort()).toEqual(["cc1", "cc2", "dm", "dp", "ibus", "vbus"]);
  });

  it("queries the longest live window back from the first live sample (AC-VEDDFD.6)", () => {
    const longest = Math.max(...LIVE_WINDOWS.map((w) => w.ms ?? 0));
    expect(longest).toBe(600_000);
    expect(backfillRange(900_000)).toEqual([300_000, 900_000]);
    // Never before the session start.
    expect(backfillRange(7_000)).toEqual([0, 7_000]);
  });

  it("turns each bucket into samples carrying its min and max, in time order (AC-VEDDFD.6)", () => {
    const times = [0, 100, 200, 300];
    const samples = backfillSamples(answer(times), 1_000);
    // Sorted by time.
    const ts = samples.map((s) => s.t_ms);
    expect(ts).toEqual([...ts].sort((a, b) => a - b));
    // Starts at the first bucket, so the window reaches back to the session start.
    expect(samples[0]?.t_ms).toBe(0);
    for (const [i, t] of times.entries()) {
      const at = samples.filter((s) => s.t_ms === t);
      expect(at.length).toBeGreaterThan(0);
      const vbus = at.map((s) => s.vbus_v);
      expect(Math.min(...vbus)).toBeCloseTo(5 + i);
      expect(Math.max(...vbus)).toBeCloseTo(5 + i + 0.5);
      const ibus = at.map((s) => s.ibus_a);
      expect(Math.min(...ibus)).toBeCloseTo(1 + i);
      expect(Math.max(...ibus)).toBeCloseTo(1 + i + 0.5);
      const cc1 = at.map((s) => s.cc1_v);
      expect(Math.max(...cc1)).toBeCloseTo(1.6 + i + 0.5);
    }
  });

  it("maps buckets by series name, not by position (AC-VEDDFD.6)", () => {
    const reversed = [...answer([0, 50])].reverse();
    const samples = backfillSamples(reversed, 1_000);
    const first = samples.filter((s) => s.t_ms === 0);
    expect(Math.min(...first.map((s) => s.vbus_v))).toBeCloseTo(5);
    expect(Math.min(...first.map((s) => s.dm_v))).toBeCloseTo(0.3);
  });

  it("keeps only buckets before the first live sample, so nothing overlaps it (AC-VEDDFD.6)", () => {
    const samples = backfillSamples(answer([0, 100, 200, 300, 400]), 250);
    expect(samples.length).toBeGreaterThan(0);
    expect(samples.every((s) => s.t_ms < 250)).toBe(true);
    expect(new Set(samples.map((s) => s.t_ms))).toEqual(new Set([0, 100, 200]));
  });

  it("leaves a channel the answer lacks at a bucket time as no value (AC-VEDDFD.6)", () => {
    const partial = answer([0, 100]).map((sb) =>
      sb.series === "dp" ? { series: sb.series, buckets: sb.buckets.slice(0, 1) } : sb,
    );
    const at100 = backfillSamples(partial, 1_000).filter((s) => s.t_ms === 100);
    expect(at100.length).toBeGreaterThan(0);
    expect(at100.every((s) => Number.isNaN(s.dp_v))).toBe(true);
    expect(Math.min(...at100.map((s) => s.vbus_v))).toBeCloseTo(6);
  });

  it("gives nothing for an empty answer (AC-VEDDFD.6)", () => {
    expect(backfillSamples([], 1_000)).toEqual([]);
    expect(backfillSamples(answer([]), 1_000)).toEqual([]);
  });

  it("seeded before the first live sample, the 10 s window spans the pre-reload history (AC-VEDDFD.6)", () => {
    const times = Array.from({ length: 70 }, (_, i) => i * 100); // 0..6.9 s
    const buf = new LiveBuffer(10_000);
    buf.push(backfillSamples(answer(times), 7_000));
    const live: Sample[] = [7_000, 7_020, 7_040].map((t_ms) => ({
      t_ms,
      vbus_v: 5,
      ibus_a: 1,
      cc1_v: 1.6,
      cc2_v: 0,
      dp_v: 0.6,
      dm_v: 0.3,
    }));
    buf.push(live);
    expect(liveRange("10s", buf)).toEqual([0, 7_040]);
    expect(liveRange("1m", buf)).toEqual([0, 7_040]);
    expect(liveRange("10m", buf)).toEqual([0, 7_040]);
  });
});

describe("fetchBackfill (FR VEDDFD, AC-272WSN.3)", () => {
  it("asks for the buckets and the run breaks of the same range, and breaks the backfilled line there (AC-VEDDFD.6)", async () => {
    const asked: string[] = [];
    const got = await fetchBackfill(7_000, {
      historyRange: (series, t0, t1, n) => {
        asked.push(`range ${series.join(",")} ${t0}-${t1} ${n}`);
        return Promise.resolve(answer([0, 1_000, 5_000]));
      },
      historyGaps: (t0, t1) => {
        asked.push(`gaps ${t0}-${t1}`);
        return Promise.resolve([{ from_ms: 1_000, to_ms: 5_000 }]);
      },
    });
    expect(asked).toEqual([
      `range ${BACKFILL_SERIES.join(",")} 0-7000 ${BACKFILL_BUCKETS}`,
      "gaps 0-7000",
    ]);
    expect(got.gaps).toEqual([{ from_ms: 1_000, to_ms: 5_000 }]);
    expect(got.samples.map((s) => s.t_ms)).toEqual([0, 0, 1_000, 1_000, 5_000, 5_000]);
    // Seeded into a buffer with the break marked, the line does not bridge it.
    const buf = new LiveBuffer(100);
    buf.push([sampleAt(7_000)]);
    buf.prepend(got.samples);
    for (const g of got.gaps) buf.markGap(g);
    const frame = buf.window(0, 7_000, 1_000);
    const at = frame.x.indexOf(5_000);
    expect(at).toBeGreaterThan(0);
    expect(frame.y.vbus.slice(0, at).some((v) => v === null)).toBe(true);
  });

  it("asks the last minute at a finer resolution than the rest of the 10 min (AC-VEDDFD.6)", async () => {
    const asked: string[] = [];
    const got = await fetchBackfill(900_000, {
      historyRange: (_series, t0, t1, n) => {
        asked.push(`range ${t0}-${t1} ${n}`);
        // Each answer also holds a bucket at the other's range, which must not leak in.
        return Promise.resolve(
          answer(t0 === 300_000 ? [300_000, 839_000, 850_000] : [800_000, 840_000, 899_000]),
        );
      },
      historyGaps: (t0, t1) => {
        asked.push(`gaps ${t0}-${t1}`);
        return Promise.resolve([]);
      },
    });
    expect(asked.sort()).toEqual([
      "gaps 300000-900000",
      `range 300000-840000 ${BACKFILL_BUCKETS}`,
      `range 840000-900000 ${BACKFILL_BUCKETS}`,
    ]);
    // Coarse buckets before the last minute, fine ones within it, in time order.
    expect(got.samples.map((s) => s.t_ms)).toEqual([
      300_000, 300_000, 839_000, 839_000, 840_000, 840_000, 899_000, 899_000,
    ]);
  });

  it("a failed gap query still backfills the samples (AC-VEDDFD.6)", async () => {
    const got = await fetchBackfill(7_000, {
      historyRange: () => Promise.resolve(answer([0, 1_000])),
      historyGaps: () => Promise.reject(new Error("no gaps")),
    });
    expect(got.samples).toHaveLength(4);
    expect(got.gaps).toEqual([]);
  });
});

describe("LiveBuffer.prepend (FR VEDDFD)", () => {
  it("puts older samples before the oldest one, wrapping the ring (AC-VEDDFD.6)", () => {
    const buf = new LiveBuffer(5);
    // Fill and wrap so the head is not at slot 0, then leave room for two.
    buf.push([1, 2, 3, 4, 5, 6, 7].map(sampleAt));
    buf.clear();
    buf.push([10, 11, 12].map(sampleAt));
    buf.prepend([5, 6, 7, 8, 9, 10, 11].map(sampleAt));
    // Only samples older than the first fit, and only the newest of them that fit.
    expect(buf.length).toBe(5);
    expect(buf.firstMs).toBe(8);
    expect(buf.window(0, 100, 1_000).x).toEqual([8, 9, 10, 11, 12]);
  });

  it("does nothing when the ring is full (AC-VEDDFD.6)", () => {
    const buf = new LiveBuffer(3);
    buf.push([10, 11, 12].map(sampleAt));
    buf.prepend([1, 2].map(sampleAt));
    expect(buf.window(0, 100, 1_000).x).toEqual([10, 11, 12]);
  });
});

function sampleAt(t_ms: number): Sample {
  return { t_ms, vbus_v: 5, ibus_a: 1, cc1_v: 1.6, cc2_v: 0, dp_v: 0.6, dm_v: 0.3 };
}
