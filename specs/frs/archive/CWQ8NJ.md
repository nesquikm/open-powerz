---
title: The sub-chart alignment test reads both plot areas once they settle
milestone: M_PVFHM6
status: archived
archived_at: 2026-11-07T09:38:21Z
id: fr_01M6TNHT31Y5QGM0A0VECWQ8NJ
created_at: 2026-11-07T09:31:51.000Z
changelog_category: Fixed
kind: bug
---

# The sub-chart alignment test reads both plot areas once they settle

## Summary

A chart test failed on the macOS runner because it measured the main plot before the app had lined it up with the sub-chart.
Turning VBUS off in the sub-chart moves the main plot's left edge by about ten pixels on the next frame.
The test took the main plot's position before that frame and the sub-chart's after it.
It now reads both plots at the same moment, waits until that reading stops changing, and keeps the one pixel limit.
The app already lines the plots up, so only the test changes.

## Requirement

### Steps to reproduce
1. Run the chart-ruler UI suite in Chromium on a slow macOS runner.

### Expected
The sub-chart alignment test passes on every runner, and still fails if the two plot areas really differ by more than one pixel.

### Actual
Main CI run 42357170692 (job 128357764735), and twice on PR 13 run 2: the left edges differed by 10 px, against a 1 px limit.

## Acceptance Criteria

- AC-CWQ8NJ.1: Regression: the test reads the main and sub-chart plot areas in one page read, so a measurement taken between two app layouts cannot pair an old main plot with a new sub-chart.
- AC-CWQ8NJ.2: Before the click and for the final check, the test waits until two reads in a row, a couple of frames apart, agree, and then requires the left edges and the right edges to be within 1 px of each other; the 1 px limit stays.
- AC-CWQ8NJ.3: A real offset still fails: with the sub-chart shifted 10 px by a style the test adds, the alignment check fails.
- AC-CWQ8NJ.4: The app's code does not change.

## Technical Design

### Root cause
Turning the sub-chart on narrows the main plot's natural left inset from 92 to 82 px. Turning VBUS off rebuilds the sub-chart, and the shared-insets alignment pads the main plot back to 92 px one frame (about 11 ms) later. The test read the main box as soon as VBUS showed off, then compared it with the sub-chart box read after the alignment.

### Fix
A helper reads both plot boxes in one page call; the settled-read helper polls it before the click and before the final assertions.

## Testing

The alignment test in Chromium, also with the wide font, the mutation check for a 10 px offset, then one full Chromium suite.

## Notes

Source: main CI after v0.10.0, macOS Chromium. Same class as N502XX and 30GN6B. Test-only, so no CHANGELOG line. Archived on the shipped M_PVFHM6.
