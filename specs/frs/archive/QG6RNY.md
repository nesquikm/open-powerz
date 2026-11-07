---
title: Desktop logo view guides the menu save after a logo write
milestone: M_PVFHM6
status: archived
archived_at: 2026-11-07T07:23:18Z
id: fr_01M6QQ9WX7T31XWS5NVJQG6RNY
created_at: 2026-11-06T06:03:59.782Z
changelog_category: Fixed
kind: bug
---

# Desktop logo view guides the menu save after a logo write

## Summary

The desktop app had the same wrong idea of the logo flag as the library.
Its confirmation now says what the write does to the startup duration.
After a write that leaves the settings unsaved, a status panel lists the menu steps.
It then checks the settings by itself until the meter has saved them.
The settings view shows the same steps instead of a bare error.

## Requirement

### Steps to reproduce
1. In the desktop app, on a KM003C, upload a logo, then open the meter settings.

### Expected
The confirmation says the duration becomes 3 s. After the write the app shows the steps for the menu save, checks until the meter has saved its settings and then shows the duration the meter has.

### Actual
The same device run as the library bug: the duration changed without warning and the settings could not be read until a menu save, with only a generic protocol error in the app.

## Acceptance Criteria

- AC-QG6RNY.1: The Upload confirmation says the startup duration goes from its value now to 3 s, and the Remove confirmation says the startup screen is turned off.
- AC-QG6RNY.2: Regression: after a write whose settings need a menu save, the view shows a status panel with the numbered menu-save steps, a line saying not to unplug the meter, and a live check that reads the settings at least 2 s apart, read-only.
- AC-QG6RNY.3: When the check finds the settings saved, the panel says so with the duration read, and the meter's settings are read again; when it finds calibration changed, the panel says so as an error and stops checking.
- AC-QG6RNY.4: When the fresh read after the write is already saved, no panel appears and the view shows the duration from that read.
- AC-QG6RNY.5: The check stops when the view closes or the meter goes away, and no other logo write starts while the panel is checking.
- AC-QG6RNY.6: Regression: reading or changing settings while a block is invalid shows the menu-save steps in the settings view instead of a generic protocol error, and sends no settings write.
- AC-QG6RNY.7: The off-screen hint in the logo view says that Upload turns the startup screen on at 3 s.
- AC-QG6RNY.8: The demo meter does its own menu save a few seconds after a logo write, so the panel can be seen to finish without a real meter.
- AC-QG6RNY.9: The desktop backend reference describes the new check command and the settings-not-saved error kind.

## Technical Design

### Root cause
The view and the backend took the duration from the settings read before the write, and the stale block surfaced as a generic decode error.

### Fix
The write result carries the before and set durations and the fresh-read outcome; the worker keeps the snapshot and answers a new check command. The store polls it while a new saving phase lasts. A stale block becomes its own error kind with the steps as its hint. The demo sim gets an automatic menu save.

## Testing

Rust command tests on the simulator; Vitest for the store's phases and polling with fake timers; Playwright for the panel and the settings view on Chromium and WebKit, also with the wide font.

AC-QG6RNY.8's demo-meter test uses real time, so a slow runner can finish the upload after the demo has already saved (30GN6B). The test accepts that save only when the upload took at least the demo's delay; the exact timing is tested on the simulator clock.

## Notes

Depends on Z39H24. Same device source, 2026-11-06.
