---
title: A hidden chart's draw loop hangs the window
milestone: M_7PKVA6
status: archived
archived_at: 2026-11-05T07:00:44Z
id: fr_01M6G7RV20PSJZJH8C7VC6VR9K
created_at: 2026-11-03T08:17:51.168Z
changelog_category: Fixed
kind: bug
---

# A hidden chart's draw loop hangs the window

## Summary

With a meter streaming, leaving the Meter tab made the window slower every second until it stopped responding.
The chart on the hidden tab kept drawing, and its dashed lines were laid out millions of pixels long.
The same layout fault put PD markers off the plot after a return to the Meter tab.
This fix stops hidden charts from drawing and keeps every draw loop bounded by the plot's size.

## Requirement

### Steps to reproduce
1. Start the app, connect a meter or any demo device and let it stream.
2. Open the PD analyzer tab and wait 40 to 60 seconds.

### Expected
The window stays smooth on every tab, at any session length.

### Actual
On the real meter the web content runs at 100% CPU and the UI stops answering (r3 report, Part P, both attempts). On the demo device the PD tab issues about 19 million canvas path calls a second and drops from 43 to 18 frames a second within a minute.

## Acceptance Criteria

- AC-C6VR9K.1: Regression: a WebKit e2e streams a demo device on the PD analyzer tab for 60 s and asserts at most 100 000 canvas path calls a second and at least 50 frames a second. It fails before the fix and passes after it.
- AC-C6VR9K.2: Regression: the seven failing tests in the PD timeline e2e spec (markers at their session time, legend, badge split, marker click, focus, Control-click and Meta-click) fail before the fix and pass after it.
- AC-C6VR9K.3: A chart whose host is hidden or smaller than its axes skips drawing and takes no part in inset alignment. Shown again, it draws once at its real size, with the same insets as the sub-chart.
- AC-C6VR9K.4: The dash builder clips each segment to the plot box before dashing. A pure test with a 10-million-pixel segment draws no more pieces than the box's diagonal allows, and the dashes inside keep the phase they would have had unclipped.
- AC-C6VR9K.5: Every per-frame canvas loop over data (dashed series, PD status dots, PD markers, overview envelope) has an iteration cap tied to the plot's pixel size. Hitting it ends that loop early, and a unit test pins the cap.
- AC-C6VR9K.6: Every chart, timeline and pd-* e2e spec passes on WebKit.
- AC-C6VR9K.7: A tester runbook keeps the real meter on the PD analyzer tab for 10 minutes. Web content CPU stays below 30%, and the UI answers clicks within 1 s.

## Technical Design

### Root cause
App.svelte keeps the Meter tab's Chart mounted while another tab shows, so its uPlot is sized 1 × 1. SharedInsets (plotInsets.ts, added in commit 0f0b9a1) still pads that plot to match the sub-chart, which makes its bbox negative (height −132), and x positions land near −1.3 million px. dashedPaths in chartSeries.ts hands those points to forEachDashPiece (dashPath.ts), which walks each segment one dash at a time, about 1.5 million canvas calls per frame. Insets learned while hidden also misplace PD markers after the return.

### Fix
Skip drawing and alignment for an unlaid plot, clip before dashing, and cap each loop.

## Testing

A pure dash-clip test, plot-insets unit tests with an unlaid plot, the WebKit e2e from the first criterion, and the full chart, timeline and pd-* WebKit set.

## Notes

Found in r3 on build 3d2ae16 and reproduced on the demo device. Bisect: the first bad commit is 0f0b9a1. The meter probe was one read-only CLI stream. The Detect sequence's reset is the PDM text command (protocol doc, PDM text session), not binary RESET.

CI caveat (9QTPNQ): CI runners without a GPU can pace frames below 50 fps even on an idle page. The e2e reads the frame rate as the median of at least three 2 s streaming windows, interleaved with as many idle windows of the same page (the stream held), idle first. Where the idle median is under 50 fps it skips only the frame-rate half of AC-C6VR9K.1, with an annotation and a log line giving the idle median; the path-call bound always runs. A developer Mac runs both halves.

## Hardware evidence (r4 to r6)

- [x] AC-C6VR9K.7: r4 (334d005, 2026-11-04) part C2. Ten minutes on the PD analyzer tab with the real meter at 50 SPS: web content CPU max 6.6 %, mean 1.82 % (61 samples), and 20 tab clicks answered in 48 ms at most. The window was visible and never covered, but not the frontmost app.
