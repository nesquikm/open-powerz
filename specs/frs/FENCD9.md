---
title: Timeline navigation
milestone: M_R4WG4Q
status: active
archived_at: null
id: fr_01M5EFBG8FXCPFV2S8C2FENCD9
created_at: 2026-10-21T05:36:22.371Z
changelog_category: Added
---

# Timeline navigation

## Summary

Moving around a long session was hard, so the chart now works like a video editor's timeline.
An overview strip shows the whole session, with a window you can drag or resize.
A playhead marks a moment and reads every series at that time.
Editor keys step, shuttle and zoom, and long gaps shrink to a narrow labelled break.
It stays smooth at 1000 samples a second with hours of data.

## Requirement

Add editor-style navigation to the chart: an overview strip with a draggable, resizable viewport window, pointer and keyboard zoom and pan, a playhead with a value readout, J/K/L shuttle, a zoom slider, the window buttons as presets of one view model, and collapsed long gaps, all smooth on long 1000 SPS sessions.

## Acceptance Criteria

- AC-FENCD9.1: An overview strip under the chart shows the whole session (the VBUS and IBUS min/max envelope) with a viewport window marking the main chart's range; both follow new data while the chart is live.
- AC-FENCD9.2: Dragging the viewport window pans the chart; dragging either of its edges resizes the range; clicking the strip outside the window centres the view there; each of these pauses the chart.
- AC-FENCD9.3: Over the plot, the mouse wheel and a trackpad pinch zoom around the pointer, and Shift+wheel and a horizontal trackpad scroll pan; each pauses the chart.
- AC-FENCD9.4: On the plot, a click places the playhead, a plain drag scrubs it, a middle-button drag pans, an Alt/Option-drag zooms to the dragged range and a double-click fits the whole session.
- AC-FENCD9.5: The playhead is drawn as a vertical line, and the readout shows the time and every visible series' value at the pointer while it is over the plot, otherwise at the playhead.
- AC-FENCD9.6: ← and → move the playhead by one pixel's worth of time (with Shift, a tenth of the view), and the view follows when the playhead would leave it.
- AC-FENCD9.7: L plays the playhead forwards and J backwards, at 1×, 2×, 4× and 8× real time on repeated presses (a press in the other direction reverses at 1×); K stops; playing forwards into the newest data returns to live; L no longer returns to live (End does).
- AC-FENCD9.8: + (or =) and − zoom in and out around the playhead, or the view centre without one; Home jumps to the session start; Shift+Z fits the whole session.
- AC-FENCD9.9: A zoom slider shows and sets the view's span on a logarithmic scale from 10 ms to the whole session.
- AC-FENCD9.10: The 10 s, 1 min, 10 min and Whole buttons set the span of the same view: live they follow the newest data, paused they keep the playhead (or the view centre) in view and stay paused.
- AC-FENCD9.11: In the main chart and the overview strip, a gap longer than one minute is collapsed to a narrow break of at most 32 px labelled with its length; times on both sides stay true, and the time axis marks the break.
- AC-FENCD9.12: With a 10-million-sample history in the mocked backend, a viewport drag, a wheel zoom and a shuttle step each render in at most 100 ms, and with the 1000 SPS demo device the chart still redraws 20 to 60 times a second with the overview strip shown.
- AC-FENCD9.13: Every new control works from the keyboard and has an accessible name: the viewport window is a slider that arrow keys move and Shift+arrow keys resize, the zoom slider is a range input, and the shortcut overlay lists the new keys.

## Technical Design

A pure timeline module holds the view model: span, end, live or paused, playhead and shuttle speed, with every gesture a transition. A piecewise map converts session time to display time, shrinking long gaps; uPlot's custom scale distribution draws with it. The overview strip is a second small uPlot fed by the whole-session answer, with the viewport window drawn over it. Shuttle steps run on animation frames through the redraw scheduler, and history requests keep the single in-flight rule.

## Testing

Vitest for every timeline transition, the gap-collapse map and its inverse, shuttle speeds and stepping, the zoom slider scale and the presets. Playwright for the strip window drag and resize, wheel and pinch, scrub, keys and slider, gap collapse on the outage mock, timings on the 10-million-sample mock, and axe on the new controls.

## Notes

From the milestone hardware review, which asked for DaVinci Resolve style navigation. The user chose editor-style scrubbing on plain drag, J/K/L for the shuttle with End for going live, and collapsing long gaps everywhere.
