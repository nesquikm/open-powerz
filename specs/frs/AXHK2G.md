---
title: The app log records errors and firmware updates
milestone: M_7PKVA6
status: active
archived_at: null
id: fr_01M69QVAS7BY12FMV0Y1AXHK2G
created_at: 2026-10-31T19:48:09.149Z
changelog_category: Fixed
kind: bug
---

# The app log records errors and firmware updates

## Summary

The app log records connects, session ends and replugs.
It misses errors that leave the session running, and it misses firmware updates.
Some lines also lack the meter model or repeat the same id.
This fix adds the missing events, tidies the wording and limits the file size.
The log still never holds a serial number or UID.

## Requirement

### Steps to reproduce
1. Run a session in which a recording write fails, or run a firmware update.
2. Read `app.log`.

### Expected
Each error and each firmware stage has a timestamped line.

### Actual
The log shows nothing for them, so it cannot serve as evidence.

## Acceptance Criteria

- AC-AXHK2G.1: Regression: `apps/desktop/src-tauri/src/devices.rs` › `a_stream_error_that_keeps_the_session_reaches_the_log` reproduces the failure; it fails before the fix and passes after it.
- AC-AXHK2G.2: A stream error that keeps the session, a failed recording write, a failed rate change, and a failed settings write or record delete each add one line naming the meter and the error kind.
- AC-AXHK2G.3: A firmware update adds a line at start, one for each stage it enters (not one per write percent), and one for its outcome: finished, cancelled or failed with the error kind.
- AC-AXHK2G.4: Every line that names a meter carries its model. A `connected` line after a replug uses the new id.
- AC-AXHK2G.5: A replug at the same id logs `replug <id>`; one at a new id logs `replug <old> to <new>`.
- AC-AXHK2G.6: A log over 5 MB moves to `app.log.1` when the app starts, replacing any earlier one.
- AC-AXHK2G.7: A test scans the lines from a full demo session and flash and finds no serial or UID value.

## Technical Design

### Root cause
Only `log_sink` and the replug path in `devices.rs` call `AppLog::line`, so events outside those paths never reach the file.

Add a small `SessionLog` wrapper holding id and model, used by the sink, the worker's failure paths and the flash command. `firmware_flash` logs from its progress closure, one line per stage change. Rotation runs in `AppLog::open`.

## Testing

Unit tests over a temporary log file for each event, rotation and the serial scan.

## Notes

Source: AC-4PJRVM.5 partial and the M7a re-run wording items. The docs line on the log is updated with the new events.
