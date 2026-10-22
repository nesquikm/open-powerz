import type { HistoryGap, HistorySeries, Sample, Series, SeriesBuckets } from "./backend";
import { LIVE_WINDOWS } from "./chartData";

/**
 * Backfill after a re-attach (FR VEDDFD): the new page's live buffer starts empty,
 * so history covering the longest live window before its first sample fills it, and
 * the 10 s, 1 min and 10 min windows show the samples from before the reload straight
 * away.
 */

/** Every channel the live buffer stores (power is derived). */
export const BACKFILL_SERIES = [
  "vbus",
  "ibus",
  "dp",
  "dm",
  "cc1",
  "cc2",
] as const satisfies readonly Series[];

/** Most buckets the backfill query asks for. */
export const BACKFILL_BUCKETS = 4000;

/** The longest live window with a fixed span. */
const LONGEST_WINDOW_MS = Math.max(...LIVE_WINDOWS.map((w) => w.ms ?? 0));

/** The history range to backfill: the longest live window up to the first live
 * sample, never before the session start. */
export function backfillRange(firstLiveMs: number): [number, number] {
  return [Math.max(0, firstLiveMs - LONGEST_WINDOW_MS), firstLiveMs];
}

type SampleKey = Exclude<keyof Sample, "t_ms">;

/** Power has no sample field (the live buffer derives it). */
const SAMPLE_KEY: Partial<Record<HistorySeries, SampleKey>> = {
  vbus: "vbus_v",
  ibus: "ibus_a",
  dp: "dp_v",
  dm: "dm_v",
  cc1: "cc1_v",
  cc2: "cc2_v",
};

/** The history answer as samples before `beforeMs`, in time order: two per bucket
 * time, one carrying each channel's min and one its max. A channel without a bucket
 * at that time is NaN (no point). */
export function backfillSamples(answer: readonly SeriesBuckets[], beforeMs: number): Sample[] {
  const byTime = new Map<number, { min: Sample; max: Sample }>();
  const at = (t_ms: number): { min: Sample; max: Sample } => {
    let pair = byTime.get(t_ms);
    if (!pair) {
      const blank = (): Sample => ({
        t_ms,
        vbus_v: NaN,
        ibus_a: NaN,
        cc1_v: NaN,
        cc2_v: NaN,
        dp_v: NaN,
        dm_v: NaN,
      });
      pair = { min: blank(), max: blank() };
      byTime.set(t_ms, pair);
    }
    return pair;
  };
  for (const { series, buckets } of answer) {
    const key = SAMPLE_KEY[series];
    if (!key) continue;
    for (const b of buckets) {
      if (!(b.t_ms < beforeMs)) continue;
      const pair = at(b.t_ms);
      pair.min[key] = b.min;
      pair.max[key] = b.max;
    }
  }
  const times = [...byTime.keys()].sort((a, b) => a - b);
  const out: Sample[] = [];
  for (const t of times) {
    const pair = byTime.get(t);
    if (pair) out.push(pair.min, pair.max);
  }
  return out;
}

/** The history queries a backfill needs, as the backend seam provides them. */
export interface BackfillSource {
  historyRange(
    series: readonly Series[],
    t0Ms: number,
    t1Ms: number,
    buckets: number,
  ): Promise<readonly SeriesBuckets[]>;
  historyGaps(t0Ms: number, t1Ms: number): Promise<HistoryGap[]>;
}

/** The span drawn at the finer resolution: the 1 min window, which is what the 10 s
 * and 1 min windows show of the backfill. */
const FINE_SPAN_MS = LIVE_WINDOWS.find((w) => w.id === "1m")?.ms ?? 60_000;

/** The samples to seed before `firstLiveMs`, and the run breaks among them, so the
 * backfilled line breaks where the history does (AC-272WSN.3). The last minute is
 * asked at a finer resolution than the rest of the 10 min, so the short windows do
 * not draw coarse buckets beside live samples. A failed gap query still backfills. */
export async function fetchBackfill(
  firstLiveMs: number,
  source: BackfillSource,
): Promise<{ samples: Sample[]; gaps: HistoryGap[] }> {
  const [t0Ms, t1Ms] = backfillRange(firstLiveMs);
  const fineMs = Math.max(t0Ms, t1Ms - FINE_SPAN_MS);
  const ask = (from: number, to: number): Promise<readonly SeriesBuckets[]> =>
    to > from
      ? source.historyRange(BACKFILL_SERIES, from, to, BACKFILL_BUCKETS)
      : Promise.resolve([]);
  const [coarse, fine, gaps] = await Promise.all([
    ask(t0Ms, fineMs),
    ask(fineMs, t1Ms),
    source.historyGaps(t0Ms, t1Ms).catch((): HistoryGap[] => []),
  ]);
  const before = backfillSamples(coarse, fineMs).filter((s) => s.t_ms >= t0Ms);
  const last = backfillSamples(fine, firstLiveMs).filter((s) => s.t_ms >= fineMs);
  return { samples: [...before, ...last], gaps };
}
