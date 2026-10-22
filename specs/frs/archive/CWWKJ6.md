---
title: Honest gaps, quiet noise and fresh temperatures
milestone: M_R4WG4Q
status: archived
archived_at: 2026-10-22T07:41:09Z
id: fr_01M5FS9Q2MHBK7PA14RXCWWKJ6
created_at: 2026-10-21T17:49:13.000Z
changelog_category: Fixed
---

# Honest gaps, quiet noise and fresh temperatures

## Summary

The real-app QA slot showed three things that read wrong.
A gap shorter than a minute left plain empty space in the overview strip, which looked like a drawing glitch.
With nothing on the output, about a milliamp of current noise filled the amps axis.
It looked like a real signal.
The CSV kept a temperature from a minute earlier while the meter reported none at 1000 SPS.

## Requirement

The overview strip shows every gap the chart shows, a flat signal reads flat on the chart and the strip, and the TEMP column never carries a stale temperature.

## Acceptance Criteria

- AC-CWWKJ6.1: The overview strip marks every gap the main chart shades: a gap shorter than the collapse threshold gets a faint band, with its length when the label fits; collapsed gaps keep their break marks.
- AC-CWWKJ6.2: With zero-based axes off, each value axis spans at least 0.1 V, 0.02 A or 0.1 W, centred on the data, so noise around a flat signal stays small; larger swings autoscale as before.
- AC-CWWKJ6.3: The overview strip's envelope uses the same minimum span per series, so an idle output reads as a flat line.
- AC-CWWKJ6.4: Recordings, exports and `powerz stream` leave TEMP empty on rows more than 2 s after the latest valid snapshot temperature.
- AC-CWWKJ6.5: Light and dark visual checks at ~900 and ~1600 px show the strip gap band and a flat idle chart on the demo device.

## Technical Design

Strip: breaks for every session gap, not only collapsed ones; uncollapsed gaps paint the band and label without slanted strokes, using the main chart's gap list. Axes: a uPlot scale range function per axis that widens the auto range to the unit's minimum span around its midpoint, shared with the strip painter. TEMP: the store's temperature log and the CLI record the session time of each valid snapshot, and a row more than 2000 ms later gets no temperature.

## Testing

Vitest for the minimum-span range and the strip's break list; Rust unit tests for the TEMP age bound in the store, recording, export and CLI stream; Playwright for the strip band; visual check.

## Notes

The strip already uses one vertical scale per series across the session; the left part looked flat because 50 SPS puts fewer samples in each pixel column than 1000 SPS.
