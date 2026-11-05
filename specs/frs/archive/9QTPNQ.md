---
title: UI tests that assumed a Mac's font and frame rate hold on every runner
milestone: M_7PKVA6
status: archived
archived_at: 2026-11-05T10:30:00Z
id: fr_01M6NE1EV3M0PGPKJF8H9QTPNQ
created_at: 2026-11-05T10:06:00.000Z
changelog_category: Fixed
kind: bug
---

# UI tests that assumed a Mac's font and frame rate hold on every runner

## Summary

Four UI tests failed on CI runners while the app behaved as designed.
On Linux the wider system font makes the top bar move Export and the rate-locked note into its overflow menu, and two tests only looked for them in the bar.
Two frame-rate tests compared against a fixed sixty frames a second, which a runner without a graphics card cannot reach even when the page is idle.
The menu tests now find the control wherever the bar put it.
The frame-rate tests compare against the same page's idle rate, measured in turns with streaming, and say plainly when a runner is too slow to measure.

## Requirement

### Steps to reproduce
1. Run the UI suite with the wide-font switch on (or on CI Linux), and on a CI runner without a GPU.

### Expected
Each test checks its behaviour on every runner, or says in the report why a measurement could not run.

### Actual
PR 12 CI on 017c826: the exporting-with-progress top-bar test waits 60 s for an inline Export button that the bar moved into its menu; the rate-locked test finds the note hidden; the 1000 SPS frame-rate test measures 32 to 45 fps against a fixed floor of 57; the hidden-tab smoothness tests fall under 50 fps on macOS Chromium.

## Acceptance Criteria

- AC-9QTPNQ.1: Regression: the top-bar "recording and exporting with progress" test starts the export from the bar's Export button when it shows, and otherwise from the overflow menu's Export item. It still checks one row, no clipping and no growth, and passes with the wide font in both browsers.
- AC-9QTPNQ.2: Regression: the rate-locked test checks the notice in the bar when the bar shows it, and otherwise in the overflow menu, and passes with the wide font in both browsers.
- AC-9QTPNQ.3: The frame-rate tests measure through one shared helper: at least three idle and three streaming windows of about 2 s each, interleaved and idle first, idle meaning the same page with the mock's stream held. The 1000 SPS test requires the streaming median to be at least 95% of the idle median. AC-SX1VTC.5 is amended to say so.
- AC-9QTPNQ.4: The hidden-tab smoothness tests keep the 50 fps and 100 000 path calls a second bounds. When the idle median is under 50 fps, only the frame-rate half is skipped, with a test annotation and a log line giving the idle median; the path-call half always runs. C6VR9K's Notes record this.
- AC-9QTPNQ.5: Every measurement prints its idle and streaming medians and raw windows to the log. On a developer Mac both frame-rate tests run unskipped and pass in Chromium and WebKit five times in a row with no retries.
- AC-9QTPNQ.6: On Linux WebKit the 1000 SPS test records its result, met or missed, in an annotation and a log line instead of failing, because the same cost shows on the build before (SX1VTC Notes); Chromium on every OS and WebKit on macOS still fail on it.

## Technical Design

### Root cause
The tests assumed a Mac: inline top-bar controls at 900 and 1280 px with the system font, and a 60 Hz frame rate on any machine. The fit levels of the top bar move Export and the note into the menu when room is short; CI runners pace frames below 60 Hz even when idle.

### Fix
A helper starts a top-bar action inline or through the menu; another checks the note in either place. A shared helper interleaves idle windows (the mock backend's stream held) with streaming ones and compares medians; it gives the baseline and the skip decision.

## Testing

The four specs with and without the wide font in both browsers; the full UI suite.

## Notes

Source: PR 12 CI on 017c826, e2e-ui on all three OSes. Test-only, so no CHANGELOG line. Archived on the shipped M_7PKVA6 by orchestrator decision; the frame-rate changes were approved by the orchestrator before any bound changed.
