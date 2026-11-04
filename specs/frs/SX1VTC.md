---
title: Dashed series crawl while the chart scrolls
milestone: M_7PKVA6
status: active
archived_at: null
id: fr_01M6GAZQF63V7RGRP7MWSX1VTC
created_at: 2026-11-03T09:14:02.598Z
changelog_category: Fixed
kind: bug
---

# Dashed series crawl while the chart scrolls

## Summary

While a chart scrolls, the dashes of dashed series slide along their lines instead of moving with the data.
That makes an unpleasant ripple.
The dash pattern restarts at the first point drawn, and that point changes every frame.
This fix restarts the pattern at fixed points in data time, so each dash stays on its samples.
It covers the main chart, the sub-chart, the recording view and the E.D. line.

## Requirement

### Steps to reproduce
1. Connect a demo device and let the live chart scroll.
2. Watch any dashed or dotted series, such as IBUS or a CC line.

### Expected
Each dash moves left with the data, like the line under it.

### Actual
The dashes stand still or slide along the line while the line moves, so they shimmer (user report through pz-ee, 2026-11-03).

## Acceptance Criteria

- AC-SX1VTC.1: Regression: a pure test draws one series on two frames whose view is shifted by Δt. Every dash that is fully in view on both frames covers the same samples, within 0.5 px. It fails before the fix and passes after it.
- AC-SX1VTC.2: The dash pattern restarts at phase 0 at grid times fixed in data time. The grid step is a 1-2-5 multiple chosen so each cell spans at least four pattern lengths on screen. Inside a cell, dashes are laid by screen arc length from the cell's grid time, and the last dash before a grid time may be cut short.
- AC-SX1VTC.3: Live scroll, pan and Go live keep dashes glued to the data. A zoom or a y-axis rescale changes the layout only inside each cell, without any crawl after it.
- AC-SX1VTC.4: The main chart, the sub-chart, the recording view and the E.D. line all behave this way. Exported images keep their dashes. Legend swatches are unchanged, and every series keeps its own pattern.
- AC-SX1VTC.5: Per-frame dash work stays bounded by the plot size (the caps from C6VR9K hold). A WebKit e2e at 1000 SPS shows no frame-rate drop against the build before this fix, within 5%.
- AC-SX1VTC.6: Regression: a WebKit e2e takes two frames of a scrolling demo stream and compares the pixels at the same data time. They match within 1 px. A frame pair goes to qa-runs/DASH-visual/ for review.
- AC-SX1VTC.7: Every chart, timeline and pd-* e2e spec passes on WebKit.

## Technical Design

### Root cause
forEachDashPiece in dashPath.ts starts the pattern at phase 0 on the first point of each call, and dashedPaths calls it with the points in view. A scroll changes the first point every frame, so the phase slides. The E.D. line strokes with a canvas dash set in screen space, which slides the same way.

### Fix
The dash builder gets each point's data time and a grid step, and restarts the phase wherever a segment crosses a grid time. The E.D. line uses the same builder. The live window scrolls in whole device pixels and reaches back to the grid time left of the view, so the first cell is laid from its start.

## Testing

Pure dash tests for the grid restart and the two-frame shift, a WebKit pixel e2e, a frame-rate comparison, and the chart, timeline and pd-* WebKit set.

## Notes

The anchoring method is the user's choice: grid restart plus arc length inside each cell. Decimated live frames may still move slightly where a bucket's min and max change. That is data, not dash phase.

## Implementation notes

Inside a cell dashes are laid by arc length from an interpolated crossing point. Sample x positions get a tiny nudge before rounding, so half-pixel samples round the same way every frame.
