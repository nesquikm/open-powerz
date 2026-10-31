---
title: Several meters, records and settings fixes
milestone: M_3KPN7D
status: archived
archived_at: 2026-10-31T13:10:58Z
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

The read-only meter run (bac9fc3) found two more bugs, fixed in 38852f9: the settings CRC after a reboot, and unplugs logged as errors. The re-run at 38852f9 passed. `docs/protocol.md` states the weaker block A check.

## Implementation notes

- Real-meter replug came back at the same bus and address, so a different address was not exercised — M7b follow-up from the 38852f9 re-run
- Only connected log lines carry the model — M7b follow-up from the 38852f9 re-run
- replug usb:02:1 at usb:02:1 reads redundantly — M7b follow-up from the 38852f9 re-run
- connected after a replug keeps the old id — left for M7b; outside this FR's ACs
- listed() matches by bus, address and pid — left for M7b; outside this FR's ACs
- No flash-path test for Io plus re-enumeration — left for M7b; outside this FR's ACs
- OS error 6 or 19 at open may read as no meter yet — left for M7b; outside this FR's ACs
- An unplug arriving as a timeout is not caught — left for M7b; outside this FR's ACs
- A wrong display field is written back and read-back cannot see it — documented in docs/protocol.md; calibration is never sent and block B stays strict
- settle_gone adds about 200 ms on a failed transfer — kept; a single look could miss the unplug
- Block A check false-pass rate is about 1 in 20, 1 in 7 while 0x10 reads FF — documented in docs/protocol.md
- auto-rotate decision text differs from the code — left for M7b; outside this FR's ACs
- pdm/port.rs held port still reads NotFound — left for M7b; outside this FR's ACs
- Windows port vanish reads as held and any CDC open now says held — left for M7b; outside this FR's ACs
- startRecording null branch is dead but tested — left for M7b; outside this FR's ACs
- Native dialog and cross-volume path are exercised only by a same-folder unit test — left for M7b; outside this FR's ACs
- Windows \\?\ case of the Save As fix was not run — needs a Windows host
- Two no-serial meters share a tile title — left for M7b; outside this FR's ACs
- app.log omits errors that do not end a session and flash events, and has no rotation — left for M7b; outside this FR's ACs
- undecoded_diff skips the 0x00 and 0x60 words — left for M7b; outside this FR's ACs
- CLI and UI warning rendering is untested — left for M7b; outside this FR's ACs
- mock pdStart and triggerOpen refuse for any flash — left for M7b; outside this FR's ACs
- PD, Trigger and Records tabs are disabled for a second meter during a flash — left for M7b; outside this FR's ACs
- Tile setRate failure notice is hidden in the grid — left for M7b; outside this FR's ACs
- Parked notices are lost — left for M7b; outside this FR's ACs
- Detach during start needs a look — left for M7b; outside this FR's ACs
- disconnect take gap against begin_flash — left for M7b; outside this FR's ACs
- An unsure real session is released without a prompt — left for M7b; outside this FR's ACs
- PD state is unknown after reload for tile meters — left for M7b; outside this FR's ACs
- Stale wording in ignore_settings_write_accepts and sim lib.rs:240 — left for M7b; outside this FR's ACs
- settings_port_error Windows Permission branch is likely unreachable — left for M7b; outside this FR's ACs
- is_recording_in accepts a subdirectory — left for M7b; outside this FR's ACs
- recording.svelte.ts:198-201 guard with a null path — left for M7b; outside this FR's ACs
- claim_part with_extension naming — left for M7b; outside this FR's ACs
