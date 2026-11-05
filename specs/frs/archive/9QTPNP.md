---
title: The live chart's left edge stays on the first sample while the window fills
milestone: M_7PKVA6
status: archived
archived_at: 2026-11-05T10:30:00Z
id: fr_01M6NE1EV3M0PGPKJF8H9QTPNP
created_at: 2026-11-05T10:05:00.000Z
changelog_category: Fixed
kind: bug
---

# The live chart's left edge stays on the first sample while the window fills

## Summary

Right after a stream starts, the live chart window is shorter than its span and grows.
Smooth scrolling lets the window's end trail the newest sample by up to one batch.
The window's start was moved back by the same amount, so it sat before the first sample.
The left edge then wobbled from frame to frame by up to fifty milliseconds.
Now the start stays on the first sample until the window is full, and only the end moves.

## Requirement

### Steps to reproduce
1. Open the app with the demo device and smooth scrolling on (the default).
2. Connect and watch the live chart during the first seconds, before the 10 s window is full.

### Expected
The window grows from the first sample: its start does not move while its end advances.

### Actual
The window's start is before the first sample and moves back and forth with the smoothing lag. The UI suite's "the live chart scrolls as batches arrive" test sees it go from −38 ms to −47 ms, in Chromium and WebKit.

## Acceptance Criteria

- AC-9QTPNP.1: Regression: while the live buffer is shorter than the window span, the smoothed live range starts at the buffer's first sample on every frame, and its end is the smoothed end.
- AC-9QTPNP.2: Once the buffer is longer than the span, the smoothed live range is the full span ending at the smoothed end, never starting before the first sample, as before this fix.
- AC-9QTPNP.3: The UI suite's "the live chart scrolls as batches arrive" test passes in Chromium and WebKit.

## Technical Design

### Root cause
`shownRange` in `Chart.svelte` built the smooth range as `[liveEnd − (range[1] − range[0]), liveEnd]`. While the window fills, `range[0]` is the first sample, and `liveEnd` trails `range[1]` by up to `LIVE_BATCH_INTERVAL_MS`, so the start went before the first sample by that lag. `pixelSnappedRange` returns a not-yet-full range unchanged, so nothing clamped it.

### Fix
A pure `smoothLiveRange(liveEndMs, spanMs, firstMs)` in `chartData.ts` returns `[max(firstMs, liveEndMs − spanMs), liveEndMs]`; `shownRange` uses it.

## Testing

Vitest cases for `smoothLiveRange` (filling, full, at the transition), the chart UI spec in both browsers, and the full UI suite.

## Notes

Source: PR 12 CI on 017c826, e2e-ui on Ubuntu and macOS WebKit; also fails locally in both browsers. Regression from HR2JAG (smooth live end). Archived on the shipped M_7PKVA6 by orchestrator decision; user-visible, so a CHANGELOG Fixed line.
