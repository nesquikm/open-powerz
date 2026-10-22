---
title: PD timeline linked to the power chart
milestone: M_NE3C7K
status: active
archived_at: null
id: fr_01M5HVWJ41WV7P8G50RPTGE83D
created_at: 2026-10-22T13:12:57.217Z
changelog_category: Added
---

# PD timeline linked to the power chart

## Summary

PD messages make most sense next to what the power did at that moment.
This feature puts PD events on the power chart as markers on the same time axis.
Clicking a marker selects the message in the packet table, and selecting a message moves the chart to it.
The CC line voltages captured with each PD packet become chart series too.

## Requirement

PD records and the power chart share one session time base, and selection is linked both ways between the packet table and the chart.

## Acceptance Criteria

- AC-TGE83D.1: Captured PD messages and events appear as markers on the power chart at their session time, coloured by kind: negotiation, reset, error, attach and detach, other. A legend names the colours.
- AC-TGE83D.2: Clicking a marker selects its row in the packet table and scrolls the table to it.
- AC-TGE83D.3: Selecting a row in the packet table moves the chart's playhead to that time and brings it into view, keeping the current zoom when the time is already visible.
- AC-TGE83D.4: The VBUS, IBUS, CC1 and CC2 values from the PD status blocks are plotted, and CC1 and CC2 can be toggled like the other series.
- AC-TGE83D.5: Dense markers merge into a count badge when they are closer than 4 px, and expanding by zooming in shows them separately.
- AC-TGE83D.6: Playwright tests on the mocked backend cover marker to row and row to chart selection.

## Technical Design

PD records carry session time computed the same way as measurement samples, so both share the chart's x axis. Markers render in a separate canvas layer above the plot, and selection is shared state in the UI store.

## Testing

Vitest for time mapping and marker clustering, and Playwright on the mocked backend for both selection directions.

## Notes

Depends on the capture core, decoder and analyzer view FRs.
