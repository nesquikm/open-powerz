---
title: Window opens off screen and some layout rough edges
milestone: M_7PKVA6
status: active
archived_at: null
id: fr_01M6H9Z5C26Q2ZNPNFKREWQ6FQ
created_at: 2026-11-03T18:15:29.923Z
changelog_category: Fixed
kind: bug
---

# Window opens off screen and some layout rough edges

## Summary

In the r3 tester run the app window opened far above every screen, so the app seemed not to start.
This fix moves a window that lands off every screen back onto the main screen at startup.
It also fixes four layout rough edges.
They are the main tabs at phone width, control grouping in Chart options, and a wrapped panel title.
Two end-to-end tests that went stale when later features added controls are fixed too.

## Requirement

### Steps to reproduce
1. Quit the app with its window on a second display, unplug that display, and start the app again (r3: the window opened at y = −987).
2. Narrow the window to 375 px and look at the main tab row.
3. Open Chart options at 1280 px.
4. Look at the Meter settings panel title at 1280 px.
5. Run the flows and keyboard end-to-end specs on WebKit.

### Expected
The window opens on a screen; every tab is reachable at 375 px; each chart option reads as one unit with its info button; the title fits on one line; the specs pass.

### Actual
The window opens off screen; "Open recording" covers the PD analyzer tab; info buttons float between controls; the title wraps; two specs fail.

## Acceptance Criteria

- AC-EWQ6FQ.1: At startup, a window whose frame has less than a 100 × 40 px piece of its title strip on any connected screen moves to the centre of the main screen, keeping its size but shrunk to fit that screen's work area. A window that is visible enough stays where it is. A pure test covers the off-every-screen, partly visible, larger-than-screen and no-screen-reported cases.
- AC-EWQ6FQ.2: At 375 px wide the four main tabs and the Open recording and Add meter buttons are all fully visible and clickable, with no horizontal scroll. When the row is too narrow, the two buttons move to their own line under the tabs; at 900 px and wider the row stays on one line as today.
- AC-EWQ6FQ.3: In Chart options each control sits beside its own info button with at most 4 px between them, and the gap to the next control's group is at least 16 px, at 1280 px and 375 px.
- AC-EWQ6FQ.4: The Meter settings panel title stays on one line at 1280 px and 900 px wide, with its info, Refresh and Edit buttons still visible.
- AC-EWQ6FQ.5: Regression: the rate-locked test in the flows spec passes. It went stale when Recorder options and Open recording made the button name "Record" match three buttons; it now names the Record button exactly.
- AC-EWQ6FQ.6: Regression: the keyboard spec's menu-focus test passes. It went stale when Export .db joined the ⋯ menu disabled while there is nothing to export; the menu skips disabled items by design, so the test expects focus on the first enabled item.
- AC-EWQ6FQ.7: The layout, flows, keyboard, info-popover and chart e2e specs pass on WebKit, and the next tester run confirms the window opens on screen after its display is removed.
- AC-EWQ6FQ.8: At 375 px each series' style row in Chart options fits the panel: its colour, width, transparency and Reset button are fully visible and clickable, nothing is cut off at the right edge, and the page does not scroll sideways (found in the review screenshots, 2026-11-04).

## Technical Design

### Root cause
macOS restores the main window's last frame, and the app never checks it against the screens now connected. The 375 px tab row has no wrap rule. Chart options lays controls and info buttons out with one uniform gap. Two specs predate later buttons and menu items.

### Fix
A pure frame-fit function in the Tauri backend, run in setup against the monitor list, moves the window when needed. The tab row wraps its action group below a width. Chart options wraps each control and its info button in one group. The panel header lets the title take the line. The two tests are corrected.

## Testing

Rust unit tests for frame fit, WebKit layout specs at 375, 900 and 1280 px, the two corrected specs, and a tester runbook step.

## Notes

Stale-test causes: flows since commit 6769a1d (Recorder options) and 9d71d0a (Open recording); keyboard since 9d71d0a (Export .db in the menu). Choices are the user's: actions wrap below the tabs; a tight pair with a gap per chart option. The user added AC .8 after the screenshot review.
