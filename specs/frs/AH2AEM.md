---
title: Live chart
milestone: M_R4WG4Q
status: active
archived_at: null
id: fr_01M5DDP0TWKNS6QSV917AH2AEM
created_at: 2026-10-20T20:07:53.038Z
changelog_category: Added
---

# Live chart

## Summary

A fast chart shows the session as it happens and keeps the whole history explorable.
A live window scrolls with new data, and zooming or panning reaches any earlier moment.
The backend sends only what fits the chart width, so ten million points stay smooth.
Series can be toggled, and hovering shows exact values at that moment.

## Requirement

Add a uPlot chart to the desktop UI with a live scrolling window, zoom and pan over the full session history through backend decimation, selectable series and a hover readout.

## Acceptance Criteria

- AC-AH2AEM.1: The chart has a volts axis (VBUS, D+, D−, CC1, CC2) on the left and an amps axis (IBUS) on the right; power can be shown on its own watts axis and is hidden by default.
- AC-AH2AEM.2: Live mode shows the last 10 s, 1 min, 10 min or the whole session (selectable) and scrolls as batches arrive, redrawing at most 60 and at least 20 times per second while the demo device streams at 1000 SPS.
- AC-AH2AEM.3: Wheel zoom, drag-to-zoom, drag-to-pan and the arrow keys leave live mode and request `history_range` with as many buckets as the plot is wide in pixels; a Live button and the L key return to live mode.
- AC-AH2AEM.4: At most one history request is in flight; a newer zoom or pan supersedes an older pending one; with a 10-million-sample history served by the mocked backend, a pan step renders in at most 100 ms in the Playwright suite.
- AC-AH2AEM.5: Legend toggles show or hide each series, and the choice persists for the session.
- AC-AH2AEM.6: Hovering shows the time and each visible series' value at the nearest sample, or its min–max range when the view is decimated.
- AC-AH2AEM.7: Stream gaps render as breaks in the lines, never as interpolated segments.
- AC-AH2AEM.8: Series colours come from the theme tokens, and every series also has a distinct line dash, so series stay distinguishable without colour in both themes.
- AC-AH2AEM.9: A "Zero-based axes" toggle makes every visible Y axis include zero (negative values stay visible); it is off by default and the choice persists for the session.

## Technical Design

`Chart.svelte` wraps uPlot with a `ResizeObserver`. `src/lib/chartData.ts` keeps the live window in a typed-array ring buffer, reduced to at most two points per pixel, and turns `history_range` min/max buckets into uPlot series. "Whole session" beyond the buffer comes from `history_range`, refreshed at most 4 times a second, which also gives the session's true start; views stay inside the session. Navigation follows FR FENCD9. Zoom and pan requests share one in-flight token on `requestAnimationFrame`, so stale answers drop. Gaps become `null` points after their last sample. Axis and series styles read `THEME_COLORS` from `theme.ts` and re-apply on theme change.

## Testing

Vitest for the ring buffer, client reduction, bucket-to-series mapping, request superseding, gap handling and the zero-based axis range (`zeroBasedRange` in `chartSeries.ts`). The Playwright suite times pan steps on a synthetic 10-million-sample mock (`pan.spec.ts`) and checks live scrolling, the hover readout and the "Zero-based axes" toggle (`chart.spec.ts`).

## Notes

FRs 272WSN, DDDYDJ and FENCD9 refine this chart and win over AC-AH2AEM.3: session time and gaps, a Live button that pauses, and editor-style navigation.

Decimation lives in Rust (FR AH2AEJ): WebKitGTK is too slow to reduce millions of points in the page.

AC-AH2AEM.9 came from the visual review: auto-scaled axes made a ±2% IBUS ripple look huge.
