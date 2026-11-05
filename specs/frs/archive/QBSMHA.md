---
title: The top bar never paints a frame wider than the window
milestone: M_7PKVA6
status: archived
archived_at: 2026-11-05T14:30:00Z
id: fr_01M6NSAT92JB2KG11ZZYQBSMHA
created_at: 2026-11-05T13:20:00.000Z
changelog_category: Fixed
kind: bug
---

# The top bar never paints a frame wider than the window

## Summary

Right after the app opens, the top bar's content changes as the meter list and settings arrive.
The bar refit itself only after the browser had already painted that frame.
So for one frame, or longer on a slow machine, the bar stuck out and the window showed a sideways scrollbar.
The bar now refits as soon as its content changes, before the frame is painted.
The check is cheap, so the live recording timer and export count do not slow the page.

## Requirement

### Steps to reproduce
1. Open the app in a 1280 by 800 window, dark theme, no meter connected.
2. Watch the first frames.

### Expected
No frame is wider than the window.

### Actual
One frame paints the page 1319 px wide: the bar's theme and About controls stick out by 39 px. A CI runner caught it at 1331 px.

## Acceptance Criteria

- AC-QBSMHA.1: Regression: from the first frame of a page load at 1280 by 800 and at 900 by 600, in both themes, no animation frame shows the document wider than the window, in Chromium and WebKit.
- AC-QBSMHA.2: A change of the bar's content is checked before the next paint, and the bar is refit only when it no longer fits; a refit for a bar that shrank still comes from the resize observer.
- AC-QBSMHA.3: WebKit still logs no ResizeObserver loop error (AC-AKNVND.6 holds).
- AC-QBSMHA.4: While recording and exporting at 1000 SPS the change adds at most a few layout reads a second and no extra refits.

## Technical Design

### Root cause
`TopBar.svelte` fits on mount and then only from a ResizeObserver whose refit waits for `setTimeout(0)`, which runs after the paint. The bar's content grows just after mount, so that frame is painted overflowing.

### Fix
A MutationObserver on the bar (child list, subtree, text) checks `fitsOneRow` as a microtask and calls `fit()` only when it does not fit. The bar's own class changes are not observed, so a fit cannot set it off again.

## Testing

A per-frame overflow probe in the layout spec in both browsers and themes; the polish spec for the loop error; refit and layout-read counts before and after while recording and exporting.

## Notes

Source: PR 12 CI run 2 on 2d3c4ca, macOS shard 1, layout spec at 1280 by 800. Measured cost after the fix: 1.6 to 2.0 extra layout reads a second and no extra refits while recording and exporting at 1000 SPS. User-visible, so a CHANGELOG Fixed line.
