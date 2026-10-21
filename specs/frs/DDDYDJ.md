---
title: Live and paused chart states
milestone: M_R4WG4Q
status: active
archived_at: null
id: fr_01M5EFBG8ET52NA3DK5SDDDYDJ
created_at: 2026-10-21T05:36:22.371Z
changelog_category: Added
---

# Live and paused chart states

## Summary

The Live button looked like a toggle but could never be switched off.
Live now means the chart follows the newest data.
Clicking it pauses the chart on what it shows, while the stream and any recording go on.
Any pan or zoom pauses it too, and the button then says how to go back.

## Requirement

The chart has two states, live and paused, with one button and the End key moving between them; pausing never stops the stream or a recording.

## Acceptance Criteria

- AC-DDDYDJ.1: While live the chart follows the newest data and the button reads "Live" and is pressed.
- AC-DDDYDJ.2: Clicking the button while live pauses the chart on exactly the range on screen; the stream and any recording keep running.
- AC-DDDYDJ.3: Any pan or zoom (wheel, drag, arrow keys) pauses the chart too.
- AC-DDDYDJ.4: While paused the button reads "Paused · Go live", is not pressed, and its tooltip says how to return to live.
- AC-DDDYDJ.5: Clicking the button while paused, or pressing End (on the chart, or anywhere outside a text field or a select), returns the chart to live.
- AC-DDDYDJ.6: The screen-reader status says "Paused: <from> to <to>" when the button pauses the chart and "Live: <window>" when it returns to live.

## Technical Design

Pause is one more chart gesture: from live it fixes a history view on the shown range, from paused it changes nothing. The button runs a toggle that picks pause or live from the current view, and takes its label, pressed state and tooltip from the view. End joins the chart keys and the global shortcuts; a focused select keeps Home and End for its options. The announcement says Paused for the pause gesture and History for a zoom or pan.

## Testing

Vitest: the pause gesture, End, the toggle and the button text (chartView), End in the shortcut map and the overlay, and a select keeping End (shortcuts). Playwright: live.spec.ts clicks Live during a stream and a recording, zooms with the wheel and returns with End; flows.spec.ts covers the arrow-key pause.

## Notes

From the milestone hardware review: "What does 'live' button do? Can't stop it". Timeline navigation (FENCD9) later gives L to the shuttle, so End is the key for going live.
