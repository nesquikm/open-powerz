---
title: Chart ruler, segment marks and energy share
milestone: M_7PKVA6
status: archived
archived_at: 2026-11-05T07:00:44Z
id: fr_01M69QVAS7BY12FMV0Y1AXHK2T
created_at: 2026-10-31T19:48:09.149Z
changelog_category: Added
---

# Chart ruler, segment marks and energy share

## Summary

The chart shows values at one point, but not the change between two points.
A two-cursor ruler now shows time, charge, energy and averages between them.
Percentage marks show when charge or energy reached each share of the total.
An energy share series, a second chart and a reversed current axis are also added.
These features work on the live chart and on an opened recording.

## Requirement

The chart gains a two-cursor ruler, percentage segment marks, an energy share series, a sub-chart and a reversed current axis.

## Acceptance Criteria

- AC-AXHK2T.1: A Ruler mode places two draggable cursors, T1 and T2, and shows delta time, delta Ah, delta Wh, and average V, A and W between them.
- AC-AXHK2T.2: Ruler values match a backend calculation over the raw samples, not the drawn buckets, within 0.1% on a test recording.
- AC-AXHK2T.3: The ruler works by keyboard, and its values are readable by a screen reader.
- AC-AXHK2T.4: Segment marks draw at each chosen step (default 10%) of total charge or energy in the current run, labelled with the percentage.
- AC-AXHK2T.5: An E.D. series shows cumulative energy as a percentage of the session total. The UI labels it as our definition, not the vendor's number, and the docs state the definition.
- AC-AXHK2T.6: A sub-chart stacks a second plot below the main one with its own series choice and the same time range, cursor and zoom.
- AC-AXHK2T.7: A "Reverse current" option flips the current axis without changing data, exports or statistics.
- AC-AXHK2T.8: Each option persists. With all of them off, the chart draws as before. A README line and the how-to describe them.

## Technical Design

A `ruler.ts` module holds cursor state, and a backend `range_stats` command integrates raw history between two times. Marks and E.D. derive from `Integrator` totals in one pure module, drawn in a uPlot draw hook. The sub-chart is a second `Chart` sharing the view store.

## Testing

Vitest for the pure modules, Rust unit tests for `range_stats`, Playwright and a visual check on the demo device.

## Notes

The vendor's E.D. formula was never recovered, so this definition is our call. The Apple battery curve stays out: no Apple device here.

## Implementation notes

Marks and E.D. come from the backend's raw samples (`run_progress`), not from the drawn buckets, so they do not move with the zoom or the plot width.
The ruler counts from each cursor to its nearest sample along the line to the sample just outside, so ΔAh over a steady load is ΔT times the average.
Marks follow the current run, since the last break; E.D. is a share of the whole session's energy, summed over its runs and never across a break.
A cursor out of view stays focusable at the plot's edge, and "Reset ruler" puts both cursors back in view.
