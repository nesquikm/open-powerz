---
title: Chart and layout polish
milestone: M_R4WG4Q
status: active
archived_at: null
id: fr_01M5F1DHSYV05XH7MSWAAKNVND
created_at: 2026-10-21T10:51:53.535Z
changelog_category: Fixed
---

# Chart and layout polish

## Summary

Polish items from the QA pass, mostly visible on standard-density screens.
Thin chart lines lost their colour, and the top bar jumped during an export.
A notice pushed the overview strip off screen, and the hover readout showed ranges like 4.990 to 4.990.
The window preset was forgotten on reload, WebKit logged a resize warning, and narrow windows cramped two controls.

## Requirement

The chart and layout stay legible and stable at standard pixel density and in narrow windows, and the chosen window survives a reload.

## Acceptance Criteria

- AC-AKNVND.1: At device pixel ratio 1, solid series are drawn at least 1.5 px wide and dashed series use whole-pixel dash patterns, so VBUS reads blue even on a grid line and the dashed series keep their colours.
- AC-AKNVND.2: The export progress appears without moving the rate picker or the Record button in the top bar.
- AC-AKNVND.3: At 1600 by 900, any number of notices together never push the overview strip below the bottom of the window: notices stack compactly in a layer that does not shift the main layout.
- AC-AKNVND.4: The hover readout shows a single value when a range's minimum and maximum format to the same text.
- AC-AKNVND.5: The chosen window (10 s, 1 min, 10 min or Whole session) is remembered for each viewer and restored after a reload, like the legend.
- AC-AKNVND.6: WebKit logs no ResizeObserver loop error at 600 px.
- AC-AKNVND.7: At 600 px the device picker shows the full device name in its tooltip, and the hover readout stays on one line.

## Technical Design

Series widths and dash patterns come from the device pixel ratio when the plot is built and when it changes. Export progress takes a reserved, fixed-width slot. Notices stack in one compact layer that does not change the main layout's height. The readout formatter compares formatted strings. The window preset is saved next to the legend preference. The resize observer defers its redraw to the next animation frame. The picker gets a title and a shorter label at narrow widths, and the readout a no-wrap style.

## Testing

Vitest: the readout formatter, window persistence, width and dash choice per pixel ratio. Playwright: pixel colour of VBUS at DPR 1, top-bar positions during an export, overview bounds with a notice, reload keeps the window, no WebKit page error at 600 px.

## Notes

QA findings 10 to 15 and 17 on build 9787adb. The 600 px items stay cheap, since 900 px is the supported minimum.
