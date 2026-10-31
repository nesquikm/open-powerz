---
title: Recorder pause, auto-stop and limits
milestone: M_7PKVA6
status: active
archived_at: null
id: fr_01M69QVAS7BY12FMV0Y1AXHK2R
created_at: 2026-10-31T19:48:09.149Z
changelog_category: Added
---

# Recorder pause, auto-stop and limits

## Summary

The recorder can only start and stop today.
This adds pause and resume, and an automatic stop when current stays low.
It also adds a row limit, a file name prefix and a default folder.
The app shows how long its memory can hold the current stream.
All of this runs on the computer and sends nothing new to the meter.

## Requirement

The desktop recorder gains pause, conditional auto-stop, a row limit, a file prefix and folder, and a memory-depth estimate.

## Acceptance Criteria

- AC-AXHK2R.1: A Pause button stops writing rows while the meter stream and chart go on. Resume continues in the same file, and the time in between is one gap in the summary.
- AC-AXHK2R.2: Charge and energy in the file do not count the paused time, and the integrator does not bridge it.
- AC-AXHK2R.3: With auto-stop on, a recording ends when current stays below a set value for a set time. It keeps the file and says why it ended. A value of 0 A or an empty field cannot be saved.
- AC-AXHK2R.4: Auto-stop runs in the backend on every record, so a slow page cannot delay it, and paused time does not count toward it.
- AC-AXHK2R.5: A recording ends at the row limit (default 10,000,000), keeping the file and saying why.
- AC-AXHK2R.6: A prefix and a default folder set the file's name and place. An unwritable folder is refused at start, and the prefix refuses path separators.
- AC-AXHK2R.7: The recorder shows how long the chart's memory holds the current rate, in plain words, and updates when the rate changes.
- AC-AXHK2R.8: Each setting persists. Tile recordings honour them. A README line and the how-to describe them. `powerz stream` does not change.

## Technical Design

`Recorder` gains `paused` and `Limits { below_a, for_ms, max_rows }`, checked in the worker's record path. Pause calls `break_run` and records one gap. New `recording_pause` and `recording_resume` commands. The end reason joins `RecordingSummary`. Depth comes from `LIVE_CAPACITY` divided by the rate.

## Testing

Rust unit tests with hand-fed records for each stop rule, Vitest for the state, Playwright on the demo device.

## Notes

The vendor's "system vs zero time" already exists as the CSV's two time columns.
