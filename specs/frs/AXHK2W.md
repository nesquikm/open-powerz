---
title: Memory trim at 1000 SPS
milestone: M_7PKVA6
status: active
archived_at: null
id: fr_01M69QVAS7BY12FMV0Y1AXHK2W
created_at: 2026-10-31T19:48:09.149Z
changelog_category: Changed
---

# Memory trim at 1000 SPS

## Summary

The macOS app's web view settles near 540 MB after six minutes at 1000 samples per second.
Most of that is steady garbage from the chart.
PD status data also grows without limit during long captures.
This change caps the status data and cuts the chart's repeated allocations.
Charts and data stay the same.

## Requirement

Long sessions at 1000 SPS and long PD captures stay within a known memory bound, and the page's plateau drops.

## Acceptance Criteria

- AC-AXHK2W.1: In the shipped macOS app on the demo device at 1000 SPS, with the window pinned on the built-in display: in the default 10 s live window the web content process's physical footprint is at or below 400 MiB at 6 minutes (median of 2 runs); in every live window (10 s, 1 min, 10 min), after the first collection the footprint stays below 500 MiB and grows by no more than 2 MiB/min over the next 10 minutes. RSS is reported beside it, and the figures are recorded in the Notes.
- AC-AXHK2W.2: The backend status log and the page's `PdStatusBuffer` are capped at 100,000 samples each. The oldest go first, with a notice, and timeline ranges stay correct across the drop.
- AC-AXHK2W.3: A drop reported by the backend also reaches a page that attaches later.
- AC-AXHK2W.4: Dropping the oldest rows from `pdStore` costs no column shift per row: an append at the cap takes constant time, with a test counting moved elements.
- AC-AXHK2W.5: The overview strip and axis redraws reuse buffers. A test shows no new typed array per frame on a steady stream.
- AC-AXHK2W.6: Parked meters' buffers obey the same caps.
- AC-AXHK2W.7: `e2e/ui/memory.spec.ts` still passes on WebKit in CI with no timing loosened.
- AC-AXHK2W.8: A measurement answers two questions, each with its numbers in this FR: do live batches drop at the worker's queue limit in the real app, and does a stalled page pile up without limit in the webview? Tauri's channel send may return before the page processes it; that was read in its source, not tested. If the simulator cannot settle a question, the check moves to the tester's runbook on a .app build, and the FR says so.
- AC-AXHK2W.9: If either answer is yes, it is fixed in this milestone: the live chart never draws a straight line across missing data with no marker, and live data held for a stalled page has a limit that keeps the 400 MB target.
- AC-AXHK2W.10: The session history keeps full resolution for the newest HISTORY_FULL_SAMPLES samples, a named constant of 10 000 000. Older chunks fold into blocks that keep, per series, min, max, sum and count, plus the block's start and end time and its charge and energy sums. Whole session, the ruler's ΔT, ΔAh, ΔWh and averages, segment marks and E.D. stay exact at block edges and lose only placement inside a block.
- AC-AXHK2W.11: The chart marks the fold boundary with a faint tick that says the data before it is coarser, both while live and while paused. The label sits on a small chip at the top of the plot, at the size of the other chart labels, and stays readable over the data in both themes.
- AC-AXHK2W.12: A test feeds a synthetic 30-hour run at 1000 SPS in compressed time. History memory stays under a stated ceiling, and totals, ruler values at block edges, mark times and E.D. match an unfolded reference.
- AC-AXHK2W.13: In the next tester run (20 minutes at 1000 SPS, footprint and RSS), the app process grows at the history's expected rate within 20%, so no other growth is left unexplained.
- AC-AXHK2W.14: Exporting a session that has folded writes only its full-resolution part. Each row keeps its own record's temperature, and the export's row count matches the rows written. The app says, when the export finishes, how much of the oldest part was folded and left out and the time the file starts at. The file itself keeps the plain recorder layout. A test folds a history and checks the rows, temperatures, count and notice.

## Technical Design

A shared ring for columns replaces array `push` and `shift` in `PdStatusBuffer` and `pdStore`. The backend log becomes a bounded deque. The strip keeps preallocated `Float64Array`s. uPlot axis values get cached per tick set. The cap stays the same as `MAX_RECORDS`.

The history folds its oldest full-resolution chunks into run-bounded blocks of per-series sums, extremes and integrator totals; range stats, marks and E.D. walk blocks, then the full-resolution tail. A boundary command feeds the chart's tick (polled while live) and the export, which skips folded records.

## Testing

Vitest with element counters, Rust unit tests for caps, the existing memory spec, and a local measurement of the demo stream.

A compressed 30-hour fold test against an unfolded reference, a folded-export test, and a WebKit fold-tick spec.

## Notes

Source: inventory note on 4NSVC9 and the follow-up on PD status data. Measured on the demo device, no meter needed.

AC.1 was amended on 2026-11-04 with the orchestrator's OK: the 1 min window draws at the batch rate and levels off near 425 MB, while 334d005 levelled off at 500 to 600 MB.

## Implementation notes

Export of a folded session (AC.14) is the user's choice: the full-resolution tail plus a notice; Record keeps a full long session. The notice is in the app, not the file, so the file stays readable as a recorder file.

r3 (3d2ae16): web content RSS 549 MB at 6 min, footprint 184 to 209 MB, so AC.1 passes by footprint; WebKit counts shared and graphics pages as resident. No live batches dropped, and a hidden window piled nothing up, so AC.9 needs no fix. The app process grew about 1 MB/min from the full-resolution history (AC.10).

r4 (334d005, 2026-11-04): footprint 542 MB at 6 min, so AC.1 failed. Do not trust the r3 footprint figures: they were read only at the start and end of the run, and the start value (209 MB) does not match the 69 to 124 MB later runs read for the same process at the start, so r3 most likely read another process. Rerun on the same machine and driver: 3d2ae16 449 MB and 334d005 484 MB at 6 min, so no regression; both pass 400 MB before 6 min. The cause and the fix are in HR2JAG.

HR2JAG final build, 2026-11-04, pinned window 1280×800 on the MacBook screen, demo load-5v-1a at 1000 SPS in the default 10 s live window (smoothing on), web content footprint (RSS beside it). Paired 6-min runs: HR2JAG 384 and 377 MB, 334d005 499 and 593 MB. 40-minute run: 399 MB at 5 min, 528 MB at 9 min, then one collection; after that 173 MB (792 RSS) at 10 min, 176 (814) at 20, 177 (818) at 30, 186 (824) at 40. Slope from 10 to 40 min is 0.32 MB/min. The app process grew 1.23 MB/min from 6 to 40 min (AC.13 expects 1.14). A separate 10 s window run measured 241 MB at 6 min; 1 min window on c2cbb1c, same setup: 401 MB at 6 min (RSS 478), one collection at about 9.5 min, then 423 to 427 MB from 10 to 20 min (RSS 633 to 646), slope 0.51 MB/min; 41 of 41 samples visible. The 10 min window is left to the next tester run.
