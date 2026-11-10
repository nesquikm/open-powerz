---
title: The File tab stats ignore the zoomed range
milestone: M_NA20AD
status: active
archived_at: null
id: fr_01M72ENP0M5AMJA8HKD2PBDEQE
created_at: 2026-11-10T10:04:47.509Z
changelog_category: Fixed
kind: bug
---

# The File tab stats ignore the zoomed range

## Summary

The stats under an opened recording always cover the whole file.
After a zoom they still do, and nothing says so.
So a VBUS minimum can come from a dip outside the zoomed part.
Now the stats say what they cover, Whole file or Shown range.
A toggle shows the stats of the range on screen, in both editions.
The live Stats panel keeps its meaning and says it counts since the connect or the last Reset.

## Requirement

### Steps to reproduce
1. Open a recording with a VBUS dip near its start in the File tab.
2. Drag across the chart to zoom into a later part with no dip.
3. Read the VBUS minimum under the chart.

### Expected
The stats say which range they cover, and a toggle shows the stats of the zoomed range.

### Actual
The minimum is the dip outside the zoomed range, and nothing says the stats cover the whole file.

## Acceptance Criteria

- AC-PBDEQE.1: The range stats answer the minimum and maximum of VBUS, IBUS and Power over the records in range, beside the averages, in the desktop backend and in the web edition; a folded block counts its kept minimum and maximum.
- AC-PBDEQE.2: The File tab stats carry a scope label: "Whole file" while they cover the file, and "Shown range" with the range's start and end times while they cover the range on screen.
- AC-PBDEQE.3: A Whole file / Shown range toggle picks the scope. Shown range is offered only while the chart is zoomed. While it is chosen, elapsed, charge, energy, minimum, maximum and average come from the range on screen and follow each new zoom; going back to the whole file shows the whole-file stats again.
- AC-PBDEQE.4: Regression: with Shown range chosen on a zoom that leaves out a VBUS dip, the VBUS minimum is the lowest VBUS inside the zoom, not the dip.
- AC-PBDEQE.5: The live Stats panel shows the label "Since connect or Reset", and its values are unchanged.
- AC-PBDEQE.6: The explore-a-session and record-a-csv pages describe the scope label, the toggle and the live panel's meaning.

## Technical Design

### Root cause
The File tab table reads only the file summary. The ruler's range stats had no minimum or maximum to offer a range view.

### Fix
Add min and max fields to the Rust and TS range stats, from records and folded blocks. A pure helper turns range stats and a range into a stats snapshot, so the existing stats view formats both scopes. The File tab view keeps a scope choice and fetches range stats for the shown range, newest answer wins. The live panel gets a static label.

## Testing

Cargo tests cover the min and max of plain and folded ranges. Vitest covers the TS range stats, the snapshot helper and the scope rules. A Playwright spec on the File tab covers the toggle and the regression, on WebKit. Doc pins check both pages.

## Notes

Shown range integrates charge and energy from the rows, while the whole file reads the CHARGE and ENERGY columns, so a range with a paused stretch can differ. The docs say so.
