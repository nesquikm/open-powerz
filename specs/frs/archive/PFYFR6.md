---
title: The README screenshot's stats disagree with its zoomed chart
milestone: M_SPYPSC
status: archived
archived_at: 2026-11-10T15:55:45Z
id: fr_01M731VQ6Y6BEKKF8YN0PFYFR6
created_at: 2026-11-10T15:40:16.932Z
changelog_category: Fixed
kind: bug
---

# The README screenshot's stats disagree with its zoomed chart

## Summary

The desktop screenshot in the README shows a chart zoomed into the voltage staircase.
Its stats table still shows the whole file, so the numbers do not match what the chart shows.
Version 0.12.1 can show stats for the shown range.
Now the screenshot is retaken with the stats set to the shown range.

## Requirement

### Steps to reproduce
1. Open the README and look at the desktop screenshot.
2. Compare the stats table with the zoomed chart.

### Expected
The stats describe the part of the recording the chart shows.

### Actual
The stats describe the whole file.

## Acceptance Criteria

- AC-PFYFR6.1: The README desktop screenshot is a real window capture of this release's app with the real staircase recording open, zoomed to the staircase, with the stats set to the shown range.
- AC-PFYFR6.2: The image is 1800 pixels wide or less and 400 KB or less, and shows no serial number or UID.

## Technical Design

### Root cause
The screenshot was taken before the shown-range stats existed.

### Fix
Build this branch in a temporary worktree, open the recording, set light theme, VBUS only, zoom about 21 s to 1:35, stats on Shown range, capture the window, scale and optimise it, replace the image.

## Testing

The README screenshot test from the sibling FR checks the file; the capture itself is checked by eye.

## Notes

Retaken from a real window capture. The File tab has no overview strip and no option to show one; only the live Meter chart draws it.
