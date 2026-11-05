---
title: Import recordings and vendor database files
milestone: M_7PKVA6
status: archived
archived_at: 2026-11-05T07:00:44Z
id: fr_01M69QVAS7BY12FMV0Y1AXHK2S
created_at: 2026-10-31T19:48:09.149Z
changelog_category: Added
---

# Import recordings and vendor database files

## Summary

The app writes recordings but cannot read them back.
This adds opening a CSV recording, in our layout or the vendor layout, for charts and statistics.
It also adds reading and writing the vendor's SQLite database format.
Damaged rows are skipped and counted.
Opening a file never touches the meter or a running session.

## Requirement

The desktop app opens CSV and vendor `.db` recordings in a read-only view, and exports the session to a vendor-compatible `.db`.

## Acceptance Criteria

- AC-AXHK2S.1: "Open recording" reads a CSV with the vendor header and shows its chart and statistics in a read-only view.
- AC-AXHK2S.2: A file with `invalid,,` rows, empty fields or short rows opens. Damaged rows are skipped and the view states how many.
- AC-AXHK2S.3: A file with a wrong header, a binary file or an empty file is refused in plain words.
- AC-AXHK2S.4: Files up to 10,000,000 rows open without freezing the UI, with progress and Cancel. The chart reads buckets from the backend.
- AC-AXHK2S.5: A vendor `.db` with the data and parameter tables opens the same way, in read-only mode. A table of another shape is refused. SQL names from the file are never pasted into a statement unquoted.
- AC-AXHK2S.6: "Export .db" writes the session in the vendor's table layout, in batches, and our own import reads it back with equal values.
- AC-AXHK2S.7: The view is clearly marked as a file, and a running stream or recording keeps going behind it.
- AC-AXHK2S.8: A CSV round trip (record, import) returns equal values and statistics. A README line and the how-to describe it.

## Technical Design

A `powerz-record` reader yields rows from CSV. A desktop `recording_file` module feeds them into a `history`-style store for bucket reads. `rusqlite` opens the vendor file read-only. `RecordingFileView.svelte` is the view, with its own plot, sharing `SeriesLegend.svelte`, the `chartSeries` plot helpers and `HistoryRequester` with the other charts.

## Testing

Rust tests with our own generated files for each damage case and large-file streaming. Fixtures are written by us, never copied vendor files.

## Notes

Schema from the vendor notes, `[static]`. No real vendor `.db` exists here, so compatibility stays unproven against the vendor app.

## Implementation notes

- A row is damaged when it starts `invalid,,`, is short, is not text, runs past 4 KB, or has an empty or unreadable ElapsedTime, VBUS, IBUS, DP, DM, CC1 or CC2. A time that goes backwards or a value out of range counts too. TEMP, CHARGE, ENERGY and PWR may be empty.
- Min, max and average come from the rows. Charge and energy show the file's last CHARGE and ENERGY, since a recording with a pause or a break does not count that time and the rows alone cannot show it. Without those columns they are integrated from the rows.
- A file past 10,000,000 rows opens with its first 10,000,000, and the view says how many were left out.
- In a `.db` the data table is the one named `X` beside a table `X_param`, of type table, holding the columns of the vendor layout. Anything else is refused. It opens immutable, so nothing lands beside it, with no schema functions and identifiers quoted. An unreadable ElapsedTime falls back to the Unix column.
- The `.db` export writes `Unix` as its primary key, so two rows with one Unix value are moved 1 µs apart. The table is `powerz_MM_dd_hh_mm_ss`, and no serial goes in it. The param row's meaning is `[inferred]`: the table name, the first Unix, the local start time, and the rate in SPS.
- The export writes to a temporary name beside the target and renames at the end, so a failed one leaves the old file alone.
- The file opens in a tab that exists only while a file is open, with no meter needed. One file at a time; opening another replaces it. Opening is available with no meter connected. It sends nothing to any meter.
- A row with DP, DM, CC1 and CC2 all empty is a meter-memory row from `powerz records get`, not a damaged one. It opens with those lines at 0. One or some of them empty is damage.
- With two `X` and `X_param` pairs, the first by name opens. Extra columns are ignored, and the `_param` table only has to exist.
- A file with no readable row is refused in plain words. An empty session writes an empty table.
- An unreadable Unix, TEMP, CHARGE or ENERGY reads as empty. A corrupt `.db` page refuses the whole file.
- A `.db` with a non-empty `-wal`, or any `-journal`, beside it is still being written, and is refused. Immutable would miss those rows.
- A `.db` progress total is the highest rowid, so it may run ahead of the rows. A table without rowids shows a plain moving bar.
- A temporary export file can remain after a crash or kill.
- A user-chosen file name shows even with serials hidden. Default names hold no serial.
- A file changes its line scale at most once: fine until the first row that needs coarse, then coarse to the end. Rows after that lose the 0.1 mV digit.
- A time past what a record holds (about 1193 hours) counts as left out, with the rows after it.
