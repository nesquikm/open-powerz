---
title: The live chart leaks memory in WebKit
milestone: M_R4WG4Q
status: archived
archived_at: 2026-10-22T07:41:09Z
id: fr_01M5FBT776XX9ZXVCXPX4NSVC9
created_at: 2026-10-21T13:53:34.440Z
changelog_category: Fixed
kind: bug
---

# The live chart leaks memory in WebKit

## Summary

While the chart shows live data, the page's memory kept growing in WebKit, the engine the macOS app uses.
It grew about 11 MB a second at 50 SPS and over 20 MB a second at 1000 SPS.
The chart made new drawing objects every frame.
WebKit leaves their native memory out when it decides to collect garbage, so it piled up.
The fix draws straight onto the canvas and reuses the chart's buffers.
Every series keeps its dash pattern.

## Requirement

A live session of any length keeps the page's memory bounded in WebKit, on macOS, Linux and in tests, while every series keeps its dash pattern.

### Steps to reproduce
1. Open the app in WebKit (the macOS app, or the Playwright WebKit browser) and connect the demo device.
2. Leave the chart live at 50 or 1000 SPS and watch the page process's memory.

### Expected
Memory climbs while the chart warms up for about a minute, then grows at most slowly (under 1 MB a second).

### Actual
Memory grows linearly: about 0.5 GB in 40 s at 50 SPS, and past 5 GB in 2 minutes at 1000 SPS. Solid lines leak as fast as dashed ones; with all series hidden it grows far more slowly. Chromium stays flat.

## Acceptance Criteria

- AC-4NSVC9.1: Regression: `e2e/ui/memory.spec.ts › a live session keeps the page's memory flat` reproduces the reported failure; it fails before the fix and passes after it.
- AC-4NSVC9.2: In the Playwright WebKit browser, after 60 s of warm-up at a 1000 SPS live stream, the page process grows by less than 120 MB over the next 2 minutes (under 1 MB/s; the leak grew about 35 MB/s); the test runs in CI on WebKit and locally only when asked for.
- AC-4NSVC9.3: Every dashed series keeps its distinct dash pattern in the plot and the legend.
- AC-4NSVC9.4: In the shipped macOS app, the web content process grows by less than 50 MB over a 10-minute 1000 SPS demo stream after warm-up; the measurement is recorded in the Notes.
- AC-4NSVC9.5: The hardware-in-the-loop notes describe a one-hour 50 SPS soak of the app with the memory figure to record.

## Technical Design

### Root cause
Every frame, uPlot and the old dash builder made new Path2D objects per series. WebKit keeps their geometry in native memory it does not report to its garbage collector, which runs only about every 37 s, so hundreds of MB piled up per cycle. Dashes were not the cause: solid series leaked as fast.

One uPlot paths function serves every series and strokes straight on the canvas context, with no Path2D and no canvas dash; dashed lines are split into pieces in device pixels, continuing the pattern across points. The live and overview frames are refilled in place. The legend swatches keep their SVG dashes.

## Testing

Vitest: the dash splitter keeps the pattern across points and gaps, and returns nothing for an empty run. Playwright, WebKit only: the memory slope test, gated by an environment flag locally. By hand: the app measurement and the soak.

## Notes

Mock page, WebKit, 1000 SPS: 22.7 MB/s before the fix; after it, 0.5 MB/s with no plateau in 330 s, partly the mock's in-page history.

AC .4, real meter at 1000 SPS (automation cannot set the demo picker); warm-up lasts until the plateau: 188 → 539 MB by ~6 min, then +21 MB over the next 5 min.
