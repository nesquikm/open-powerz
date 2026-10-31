---
title: Import recordings and vendor database files
milestone: M_7PKVA6
status: active
archived_at: null
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

A `powerz-record` reader yields rows from CSV. A desktop `recording_file` module feeds them into a `history`-style store for bucket reads. `rusqlite` opens the vendor file read-only. Reuse `RecordChart` for the view.

## Testing

Rust tests with our own generated files for each damage case and large-file streaming. Fixtures are written by us, never copied vendor files.

## Notes

Schema from the vendor notes, `[static]`. No real vendor `.db` exists here, so compatibility stays unproven against the vendor app.
