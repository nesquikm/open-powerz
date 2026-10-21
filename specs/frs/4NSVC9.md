---
title: The live chart leaks memory in WebKit
milestone: M_R4WG4Q
status: active
archived_at: null
id: fr_01M5FBT776XX9ZXVCXPX4NSVC9
created_at: 2026-10-21T13:53:34.440Z
changelog_category: Fixed
kind: bug
---

# The live chart leaks memory in WebKit

## Summary

While the chart shows live data, the page's memory keeps growing in WebKit, the engine the macOS app uses.
It grows about 11 MB a second at 50 SPS and 35 MB a second at 1000 SPS.
An hour of streaming would use gigabytes, and macOS then kills the page.
The cause is the dashed chart lines, which WebKit never frees when they are redrawn.
The fix keeps the dashes but draws them as short solid pieces.

## Requirement

A live session of any length keeps the page's memory flat in WebKit, on macOS, Linux and in tests, while every series keeps its dash pattern.

### Steps to reproduce
1. Open the app in WebKit (the macOS app, or the Playwright WebKit browser) and connect the demo device.
2. Leave the chart live at 50 or 1000 SPS and watch the page process's memory.

### Expected
Memory settles after a minute and stays flat.

### Actual
Memory grows linearly: about 0.5 GB in 40 s at 50 SPS, and past 5 GB in 2 minutes at 1000 SPS. With all series hidden, or with dashes turned off, it stays flat. Chromium stays flat.

## Acceptance Criteria

- AC-4NSVC9.1: Regression: `e2e/ui/memory.spec.ts › a live session keeps the page's memory flat` reproduces the reported failure; it fails before the fix and passes after it.
- AC-4NSVC9.2: In the Playwright WebKit browser, the page process grows by less than 50 MB over 2 minutes of a 1000 SPS live stream after 30 s of warm-up; the test runs in CI on WebKit and locally only when asked for.
- AC-4NSVC9.3: Every dashed series keeps its distinct dash pattern in the plot and the legend.
- AC-4NSVC9.4: In the shipped macOS app, the web content process grows by less than 50 MB over a 10-minute 1000 SPS demo stream after warm-up; the measurement is recorded in the Notes.
- AC-4NSVC9.5: The hardware-in-the-loop notes describe a one-hour 50 SPS soak of the app with the memory figure to record.

## Technical Design

### Root cause
uPlot strokes each of the five dashed series every frame with a canvas dash pattern. WebKit keeps native memory for every dashed stroke and does not reclaim it, so memory grows with frames times points. Measured: flat with all series hidden and flat with dashes off.

Dashed series get a custom uPlot paths builder that splits each polyline into its dash pieces, in device pixels and continuing the pattern across points, and strokes them as one undashed path. The canvas never draws with a dash setting. The legend swatches keep their SVG dashes.

## Testing

Vitest: the dash splitter keeps the pattern across points and gaps, and returns nothing for an empty run. Playwright, WebKit only: the memory slope test, gated by an environment flag locally. By hand: the app measurement and the soak.

## Notes

Found while investigating a local out-of-memory during test runs. The leak predates the overview strip. The app measurement waits for the fix, when the window can stay visible for ten minutes.
