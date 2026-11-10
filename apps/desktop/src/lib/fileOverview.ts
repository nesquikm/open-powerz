// FR 09H204: the File tab's overview strip, from the file's extent and the range on screen.
import type { OverviewState } from "./components/OverviewStrip.svelte";
import type { TimeRange } from "./chartView";

/** What the File tab's strip shows: the range on screen (the whole file when not zoomed)
 * within the whole file; `null` with no file, so the strip clears. File time starts at 0. */
export function fileOverviewState(
  extent: TimeRange | null,
  shown: TimeRange | null,
): OverviewState | null {
  if (extent === null) return null;
  const range = shown ?? extent;
  return {
    range: { t0Ms: range.t0Ms, t1Ms: range.t1Ms },
    session: { firstMs: extent.t0Ms, lastMs: extent.t1Ms },
    originMs: 0,
  };
}
