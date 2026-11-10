import type { MinMaxAvg, RangeStats, StatsSnapshot } from "./backend";
import type { TimeRange } from "./chartView";
import { DASH, formatFixed } from "./format";

export { MINUS } from "./format";

/** `value` with `decimals` places, a true minus sign for negatives and never a
 * negative zero; {@link DASH} when missing. */
export function formatSignedNumber(value: number | null | undefined, decimals: number): string {
  return formatFixed(value, decimals);
}

/** `value` with `decimals` places and its unit, a true minus sign for negatives and
 * never a negative zero (e.g. `−0.120 Ah`); `— unit` when missing. */
export function formatSigned(
  value: number | null | undefined,
  decimals: number,
  unit: string,
): string {
  return `${formatSignedNumber(value, decimals)} ${unit}`;
}

/** Elapsed time as h:mm:ss, hours unpadded. */
export function formatStatsElapsed(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  return `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
}

/** Min / max / avg strings of one quantity. */
export interface MinMaxAvgView {
  min: string;
  max: string;
  avg: string;
}

/** Every string the stats panel shows. */
export interface StatsView {
  elapsed: string;
  charge: string;
  energy: string;
  vbus: MinMaxAvgView;
  ibus: MinMaxAvgView;
  power: MinMaxAvgView;
}

function mmaView(v: MinMaxAvg | null, unit: string): MinMaxAvgView {
  return {
    min: formatSigned(v?.min, 3, unit),
    max: formatSigned(v?.max, 3, unit),
    avg: formatSigned(v?.avg, 3, unit),
  };
}

/** Formats a stats snapshot; before any data (`null`) it reads as zeros and dashes. */
export function statsView(s: StatsSnapshot | null): StatsView {
  return {
    elapsed: formatStatsElapsed(s?.elapsed_ms ?? 0),
    charge: formatSigned(s?.charge_ah ?? 0, 3, "Ah"),
    energy: formatSigned(s?.energy_wh ?? 0, 3, "Wh"),
    vbus: mmaView(s?.vbus ?? null, "V"),
    ibus: mmaView(s?.ibus ?? null, "A"),
    power: mmaView(s?.power ?? null, "W"),
  };
}

/** A formatted value is the missing-value placeholder. */
export function isMissing(text: string): boolean {
  return text.startsWith(DASH);
}

/** What the live Stats panel counts over (AC-PBDEQE.5). */
export const LIVE_STATS_SCOPE = "Since connect or Reset";

/** Range stats over `range` as a stats snapshot, so the stats view formats them like
 * the whole file's (AC-PBDEQE.3). */
export function rangeStatsSnapshot(stats: RangeStats, range: TimeRange): StatsSnapshot {
  return {
    elapsed_ms: range.t1Ms - range.t0Ms,
    samples: stats.samples,
    charge_ah: stats.charge_ah,
    energy_wh: stats.energy_wh,
    vbus: { min: stats.vbus_min_v, max: stats.vbus_max_v, avg: stats.vbus_avg_v },
    ibus: { min: stats.ibus_min_a, max: stats.ibus_max_a, avg: stats.ibus_avg_a },
    power: { min: stats.power_min_w, max: stats.power_max_w, avg: stats.power_avg_w },
  };
}
