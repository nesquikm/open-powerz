---
title: M2 final polish
milestone: M_R4WG4Q
status: archived
archived_at: 2026-10-22T07:41:09Z
id: fr_01M5FYVBBPGVSWXSV8TQSX3EDV
created_at: 2026-10-21T19:26:25.712Z
changelog_category: Fixed
---

# M2 final polish

## Summary

The last fixes from the final M2 QA run.
In mid-width windows the top bar still reflowed when an export started.
In narrow windows the notices covered the statistics, and the range status could read the same time twice.
The overview strip drew slow stretches as dots, and the disconnect banner hid an axis label.
One short hole remained at 1000 samples per second under a save sheet. The poll thread now gets a higher scheduling class on macOS.

## Requirement

The layout stays stable and covers no data at every supported width, status text and the overview strip read correctly at any zoom and rate, and the meter poll thread is scheduled ahead of ordinary work on macOS.

## Acceptance Criteria

- AC-SX3EDV.1: Starting an export moves neither the rate picker nor the Record button, and changes no other top-bar item's position or visibility, at window widths of 900, 1000, 1100, 1200, 1300 and 1600 px: while an export runs, its progress fills the disabled Export button's own box, and the exported count is in the progress tooltip and its status text.
- AC-SX3EDV.2: When the Export button lives in the "⋯" menu, a running export shows a thin progress line along the top bar's bottom edge, and the bar's items keep their places.
- AC-SX3EDV.3: Below 1200 px wide, notices sit in the page between the chart card and the Stats panel: they cover no reading, trace, axis, overview strip or statistic, and the chart card keeps its position and size while they show, at 900×900 and 900×1400 in light and dark themes.
- AC-SX3EDV.4: At 1200 px and wider, notices keep the floating stack over the side panel and AC-AKNVND.3 still holds.
- AC-SX3EDV.5: The range status writes both ends with the decimals the visible span needs: one decimal from 1 s up, two from 100 ms to 1 s, three below 100 ms, so "Paused: 11.200 s to 11.240 s" shows two different times.
- AC-SX3EDV.6: The overview strip draws each series as a continuous line wherever samples are sparser than one per pixel column, at 10 and 50 SPS and after a rate change, and still leaves every gap and break it marks unbridged.
- AC-SX3EDV.7: After a disconnect, the "Meter disconnected, showing the last session" banner sits in the line above the plot and covers no axis label, tick label or trace; the hover readout takes that line while the pointer is over the plot.
- AC-SX3EDV.8: On macOS the meter worker's poll thread runs at the user-interactive quality-of-service class from its first poll; other systems are unchanged.
- AC-SX3EDV.9: Plan tasks for the deferred items (plot height at 900×900, the hole a rate switch leaves, no label on narrow gap bands) are recorded under M7.

## Technical Design

The export progress overlays the Export button's own box, absolutely positioned, so the measured row width never changes; at the export-in-menu level it becomes a bar along the bar's bottom edge. Below the layout breakpoint the notice stack is part of the grid, in a row between the main area and the side panel, sized so the main area's rows do not take free space from it. Range decimals come from one pure span helper. The strip fills empty pixel columns between neighbouring columns by linear interpolation, and stops at any column its breaks cover. The banner moves into the hover row. The worker thread calls the macOS QoS function from the libc crate.

## Testing

Vitest: the decimals helper, the column-bridging function (gaps left open), and the fit levels. Playwright: top-bar boxes before and during an export at each width, the notice and chart-card boxes at 900 px in both themes, the strip's pixel columns at 10 SPS, and the banner box against the axis. Rust: a unit test reads the thread's QoS class back on macOS.

## Notes

QA findings N1–N4, N6, R1 and R3 on build 09aec0d. R1: a 24 ms hole at 1000 SPS during a 30 s save sheet. 2026-10-22, real meter, build 55c7b88: no hole while a save sheet was open 30 s (30,021 rows at 1 ms). Over 355 s at 1000 SPS, one 22 ms hole, while the window was off screen.
