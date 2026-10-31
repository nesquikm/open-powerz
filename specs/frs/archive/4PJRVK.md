---
title: PD analyzer, replug and CLI fixes
milestone: M_3KPN7D
status: archived
archived_at: 2026-10-31T13:10:58Z
id: fr_01M65KVDYC49QF1V7N1Y4PJRVK
created_at: 2026-10-30T05:17:20.204Z
changelog_category: Fixed
---

# PD analyzer, replug and CLI fixes

## Summary

The PD analyzer and the reconnect logic shipped in M3 with a list of follow-ups.
Long captures can grow without limit, and a failing reconnect retries too often. Piping the CLI into another tool prints an error.
This fixes those and tidies the PD library's public API before the web edition. It also makes the demo device's PD and power readings agree.

## Requirement

PD capture, PD files, replug recovery and the CLI handle long sessions, damaged files, closed pipes and repeated failures cleanly, and the PD library's API is ready for outside use.

## Acceptance Criteria

- AC-4PJRVK.1: A reconnect whose open keeps failing backs off (growing delay with a cap) instead of retrying every poll.
- AC-4PJRVK.2: `powerz watch` and `powerz pd` end quietly with exit 0 when their output pipe closes.
- AC-4PJRVK.3: The backend record store and the UI column store for a PD capture are capped (oldest dropped, with a notice) or re-decode on demand; type/SOP filter lists update incrementally.
- AC-4PJRVK.4: PD status samples in flight at a Clear are dropped; an e2e check shows a timeline badge splitting on zoom.
- AC-4PJRVK.5: The demo device's PD scenario and its measurement stream agree (9 V negotiated means 9 V measured).
- AC-4PJRVK.6: Vendor PD import notices count skipped rows; a .pdcap damaged on its first entry opens as an empty capture with the damage notice; `pd decode` handles a file error with a clear message.
- AC-4PJRVK.7: An automatic reconnect releases an opened capture's memory as Connect does.
- AC-4PJRVK.8: powerz-pd's public enums are non-exhaustive, `Issue` implements Display, and one visibility rule applies to sub-decoders, with the API docs updated.

## Technical Design

Back-off in the replug loop; broken-pipe handling in the CLI writer; bounded stores with a drop counter; simulator PD/measurement link via one scenario state.

## Testing

Unit tests for back-off, the bounded stores and the file errors; CLI tests with a closed pipe; simulator tests for the linked demo; Playwright for the timeline badge.

## Notes

Source: the M_NE3C7K M7 follow-ups.

## Implementation notes

- Status log and PdStatusBuffer are unbounded — M7b, with parked meters
- Second-meter opened-file release (F1) and the showInFullView reverse leak — left for M7b; outside this FR's ACs
- No CHANGELOG entry for pd decode exit 2 and the pdClear shape — left for M7b; outside this FR's ACs
- FirstSeen filter lists need a look — left for M7b; outside this FR's ACs
- pdStore shifts 100k columns — left for M7b; outside this FR's ACs
- dropped() is not surfaced on attach — left for M7b; outside this FR's ACs
- limit() is test-only — left for M7b; outside this FR's ACs
- mockBackend #openedPd on connect needs a look — left for M7b; outside this FR's ACs
- Floor test gaps remain — left for M7b; outside this FR's ACs
- Simulator leftovers remain — left for M7b; outside this FR's ACs
