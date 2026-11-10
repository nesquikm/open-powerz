import { formatSessionTime } from "./chartSeries";
import { rangeDecimals, type TimeRange } from "./chartView";

/** What the File tab's stats cover: the whole file or the range on screen. */
export type StatsScope = "file" | "range";

/** The File tab's range format: "<start> to <end>", with as many decimals as the
 * range's span needs. */
export function rangeSpanText(range: TimeRange): string {
  const decimals = rangeDecimals(range.t1Ms - range.t0Ms);
  return `${formatSessionTime(range.t0Ms, decimals)} to ${formatSessionTime(range.t1Ms, decimals)}`;
}

/** The File tab's stats scope label (AC-PBDEQE.2): "Whole file", or "Shown range"
 * with the range's start and end in the File tab's range format. */
export function statsScopeLabel(scope: StatsScope, range: TimeRange | null): string {
  if (scope === "file" || range === null) return "Whole file";
  return `Shown range, ${rangeSpanText(range)}`;
}

/** The scope in effect (AC-PBDEQE.3): Shown range only while the chart is zoomed
 * (`range` is the zoomed range, `null` when not zoomed). */
export function effectiveScope(scope: StatsScope, range: TimeRange | null): StatsScope {
  return scope === "range" && range !== null ? "range" : "file";
}
