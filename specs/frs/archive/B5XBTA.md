---
title: A click on the overview strip right after connect is ignored
milestone: M_7PKVA6
status: archived
archived_at: 2026-11-05T07:00:44Z
id: fr_01M6HJDD77ZZP4KBG32WB5XBTA
created_at: 2026-11-03T20:43:05.320Z
changelog_category: Fixed
kind: bug
---

# A click on the overview strip right after connect is ignored

## Summary

Right after a stream starts, a click on the overview strip under the chart does nothing.
The chart stays Live and the view does not move.
The click keeps the width of the view on screen.
In the first moment that view is narrower than the narrowest view the chart allows, so the chart drops it.
This fix gives the click at least the narrowest view, as wheel panning already does.
It surfaced as a flaky end-to-end test.

## Requirement

### Steps to reproduce
1. Connect a demo meter with a seeded history.
2. Within the first moment of streaming, click the overview strip well left of its window.

### Expected
The chart pauses and centres the view on the clicked time.

### Actual
The chart stays Live and the view does not move (the timeline overview-strip click test failed 3 of 12 runs on WebKit, 2026-11-04).

## Acceptance Criteria

- AC-B5XBTA.1: Regression: a pure test clicks the strip with a view narrower than the session's narrowest allowed view. The result is exactly the narrowest view, centred on the clicked time and kept inside the session. A view at least that wide keeps its width as today. It fails before the fix and passes after it.
- AC-B5XBTA.2: On WebKit, a click on the strip while the live view is still narrower than the narrowest allowed view pauses the chart and centres the view on the clicked time.
- AC-B5XBTA.3: The timeline spec's overview-strip click test passes 20 runs out of 20 on WebKit, and every chart, timeline and pd-* spec passes on WebKit.

## Technical Design

### Root cause
The strip's click maps the click to a range with the current view's span. The chart's range gesture ignores any range narrower than the session's narrowest span, so a click while the live view is a few ms wide changes nothing.

### Fix
The click uses at least the session's narrowest span, the same clamp the wheel pan uses, before it fits the range inside the session.

## Testing

A Vitest test on the click mapping with a span below the minimum, a WebKit e2e that clicks right after connect, and the repeated timeline test plus the chart, timeline and pd-* WebKit set.

## Notes

Found while running the wide WebKit set for EWQ6FQ; the user chose to fix the app rather than the test. Drags of a window narrower than the minimum are left as they are.
