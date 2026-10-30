---
title: Several meters, records and settings fixes
milestone: M_3KPN7D
status: active
archived_at: null
id: fr_01M65KVDYC49QF1V7N1Y4PJRVM
created_at: 2026-10-30T05:17:20.204Z
changelog_category: Fixed
---

# Several meters, records and settings fixes

## Summary

The multi-meter view, offline records and settings writes shipped in M6 with a few rough edges.
The top bar's buttons act on whichever meter the picker shows. The tile order changes after a reload, and Record asks for a file before it starts.
This makes the controls follow the meter you are looking at and fixes the record and settings edge cases.

## Requirement

The multi-meter UI, record delete and settings write behave predictably in the edge cases found during M6 testing.

## Acceptance Criteria

- AC-4PJRVM.1: Record, Export, rate and Disconnect act on a clearly named meter: per tile in the grid, and on the expanded meter in the full view.
- AC-4PJRVM.2: Record starts recording at once to a default file and asks where to save only at Stop (or uses a setting), not before it starts.
- AC-4PJRVM.3: Tile order stays the same across a page reload.
- AC-4PJRVM.4: PD capture keeps running when its meter leaves the full view, and connecting another meter is allowed while one meter flashes firmware (the flashing meter stays locked).
- AC-4PJRVM.5: app.log records session events (connect, disconnect, replug, errors) so it can serve as evidence.
- AC-4PJRVM.6: Delete refuses a 22+ remaining-entries table before writing the CSV backup; the re-read mismatch message says when the count matches but the entries differ; a desktop delete-all on an empty list sends nothing.
- AC-4PJRVM.7: A settings write whose serial port is held by another program says so in plain words; the read-back also compares the undecoded bytes of the block and reports a difference as a warning.
- AC-4PJRVM.8: The simulator answers a CDC settings write with FINISHED, as the meter does.

## Technical Design

Per-tile control bar component; session-scoped default recording path; stable tile order from the backend session list; log hooks in the session worker.

## Testing

Component and store tests per AC; Playwright with two simulators for controls, order and recording; simulator tests for the delete and settings edge cases.

## Notes

Source: the M_1HHSZZ and M_CD7M3M M7 follow-ups. The hide-serial option is in M7b.
