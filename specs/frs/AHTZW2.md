---
title: Export .db does nothing in the browser
milestone: M_ZFZQE7
status: active
archived_at: null
id: fr_01M70RPVKDVSA3VKYYT9AHTZW2
created_at: 2026-11-09T18:21:42.000Z
changelog_category: Fixed
kind: bug
---

# Export .db does nothing in the browser

## Summary

In the web edition the Export .db button looked live but wrote nothing.
On the Meter tab each click added another notice naming an internal function.
Now the browser writes the same .db file as the desktop app and downloads it.
It has the same tables, columns and rows, so the desktop app and the vendor's tools open it.

## Requirement

### Steps to reproduce
1. Connect a meter in the web edition and let it stream for a while.
2. Press Export .db.

### Expected
A .db file of the session so far downloads, as the desktop app saves one.

### Actual
Nothing downloads; on the Meter tab the notice "exportSessionDb is not available in the browser yet." appears once per click.

## Acceptance Criteria

- AC-AHTZW2.1: Regression: in the web edition Export .db downloads a .db file of the session so far and reports its progress.
- AC-AHTZW2.2: The file has the desktop app's layout: a data table named after the session's local start time with the desktop's columns, and its parameter table.
- AC-AHTZW2.3: Its rows match the desktop export for the same session: elapsed time, Unix time, the readings, and charge and energy counted per measuring run.
- AC-AHTZW2.4: The desktop app's .db reader opens the file and reads every row.

## Technical Design

### Root cause
The web backend never implemented the .db export; the desktop writes it with a native SQLite library that does not build for the browser.

### Fix
SQLite in the browser through sql.js, loaded only for a .db export or open. The table name, the SQL text and the row values come from the shared recorder crate, which the desktop uses too, so both write the same file.

## Testing

Rust tests for the shared table and row builders; Vitest over the demo meter for the downloaded file read back with sql.js; a Rust test reading a browser-written fixture with the desktop's reader.

## Notes

Found by the web parity HIL re-run (2026-11-09, step P14).
