---
title: The stats scope toggle hides which scope is on
milestone: M_NA20AD
status: active
archived_at: null
id: fr_01M72GYR200BPXNFAJ6TEWEV1Z
created_at: 2026-11-10T10:44:41.664Z
changelog_category: Fixed
kind: bug
---


# The stats scope toggle hides which scope is on

## Summary

The Stats scope toggle in the File tab shows two plain buttons.
Nothing shows which one is on, in either theme.
Its Whole file button also shares a name with the zoom bar's reset button.
Now the toggle looks like the header's theme switch, with the chosen part filled.
It reads Stats, then File or Shown range, in the desktop app and the web edition.

## Requirement

### Steps to reproduce
1. Open a recording in the File tab and zoom into part of the chart.
2. Look at the Stats scope toggle under the chart, then press Shown range.

### Expected
The chosen scope is plainly marked, and no stats button shares a name with the zoom reset.

### Actual
Both buttons look the same whichever is chosen, and two buttons are named Whole file.

## Acceptance Criteria

- AC-EWEV1Z.1: The Stats scope toggle is one segmented control with a visible "Stats" label and the segments "File" and "Shown range"; the chosen segment carries aria-pressed true and a filled style like the header theme switch, in the light and dark themes, in the desktop app and the web edition.
- AC-EWEV1Z.2: Regression: no button in the File tab stats toggle is named "Whole file"; the zoom bar's "Whole file" button is the only one with that name.
- AC-EWEV1Z.3: While the chart is not zoomed, the Shown range segment is disabled and looks disabled.
- AC-EWEV1Z.4: The explore-a-session and record-a-csv pages name the toggle's segments File and Shown range, and the doc tests pin those names.

## Technical Design

### Root cause
The toggle reused plain buttons with aria-pressed but had no pressed style, and its first button took the zoom reset's name.

### Fix
Restyle the group in RecordingFileView as the theme switch's segmented control: a shared border, the pressed segment in the text colour on the background colour, a muted disabled segment. Add a visible Stats label, rename the first segment File, keep the group name Stats scope. The scope label above the stats still says Whole file or Shown range.

## Testing

Vitest pins the doc names. The File tab Playwright spec checks the names, aria-pressed and the filled style of the chosen segment on WebKit.

## Notes

Only the toggle changes; the stats and the scope label stay as they are.
