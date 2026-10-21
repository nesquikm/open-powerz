---
title: Recording and CSV export
milestone: M_R4WG4Q
status: active
archived_at: null
id: fr_01M5DDP0TVSC9Q6DHQ6C0P7HSJ
created_at: 2026-10-20T20:07:53.038Z
changelog_category: Added
---

# Recording and CSV export

## Summary

The command-line recorder moves into a shared library that both apps use.
In the desktop app, the user picks a rate and starts recording with one key.
Rows are written to the chosen file while recording, in the vendor-compatible layout.
The whole session can also be exported later, with the same plain number format.

## Requirement

Extract the CLI's CSV recorder into a shared crate, and add rate selection, record-to-file and session export to the desktop app on top of it.

## Acceptance Criteria

- AC-0P7HSJ.1: A new crate `crates/powerz-record` holds the CSV header, row formatting (elapsed time, Unix time, plain decimals with at most 8 significant digits) and the charge/energy integrator; `powerz-cli` uses it, and every existing CLI unit, snapshot and end-to-end test passes unchanged.
- AC-0P7HSJ.2: The rate picker offers 2, 10, 50 and 1000 SPS; changing it while connected restarts the queue at the new rate; when the rate is locked (no authentication) the picker is disabled and reads "Rate set on the meter: N SPS — authentication unavailable".
- AC-0P7HSJ.3: Start recording (button or Space) opens a save dialog proposing `powerz-YYYYMMDD-HHMMSS.csv`, then writes the header and every following row to that file as samples arrive, flushed at least once per second; Stop (button or Space) closes the file and shows the number of rows written and any gaps.
- AC-0P7HSJ.4: For the same queue records, the recorded file is byte-for-byte what `powerz stream` writes: same header and columns, time counted from the first recorded sample, CHARGE and ENERGY counted from the recording start.
- AC-0P7HSJ.5: Export session writes the full session history in the same layout without stopping the stream; it runs off the UI thread with a progress indicator, and exports 1 million samples in at most 5 s.
- AC-0P7HSJ.6: Unplugging the meter while recording closes the file with every row received and shows a notice; a write error stops the recording, shows a plain-language error and keeps the rows already written.
- AC-0P7HSJ.7: While recording, the status bar shows a recording indicator with elapsed time and row count.

## Technical Design

`powerz-record` exposes `CSV_HEADER`, `sig8`, `format_elapsed`, `Integrator` and a `CsvRecorder` that wraps any `Write`, takes queue records and owns the time origin and integration; the CLI's stream command becomes a thin loop over it. The desktop `recording` module holds an optional recorder fed by the worker on its own thread, so file I/O never touches the UI. The save dialog comes from `tauri-plugin-dialog`; an `e2e` build feature reads a target directory from an environment variable instead, for tests. Export replays raw history records through a fresh `CsvRecorder` on a background thread, reporting progress events.

## Testing

The CLI suite proves the extraction is behaviour-neutral. Golden-file tests compare a desktop recording with `powerz stream` output for the same simulator records. Export timing over 1 million synthetic records. Fault tests: disconnect while recording, and a writer that fails mid-file.

## Notes

AC-0P7HSJ.1's "unchanged" applies to the CLI's snapshot and end-to-end tests: the CLI's record unit tests (CSV header, row formatting, integrator) moved with that code into `crates/powerz-record`, where they run as that crate's unit tests.

Confirmed in the milestone review (2026-10-21): the rate cannot change while a recording is open: a queue restart can restart the device clock, and one file keeps one rate. A user disconnect ends a recording as stopped, an unplug as unplugged.
