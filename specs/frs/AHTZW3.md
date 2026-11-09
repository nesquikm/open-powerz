---
title: Open a saved recording in the browser
milestone: M_ZFZQE7
status: active
archived_at: null
id: fr_01M70RPVKDVSA3VKYYT9AHTZW3
created_at: 2026-11-09T18:21:42.000Z
changelog_category: Added
---

# Open a saved recording in the browser

## Summary

The web edition can now open a saved recording, as the desktop app does.
You pick a CSV or a .db file and the app shows its chart and statistics, without a meter.
The file is read with the desktop app's own rules, so the same rows are kept or skipped.

## Requirement

Open recording is available in the web edition. It opens the browser's file picker for CSV and .db files and shows the chosen file in the recording view, as on the desktop.

## Acceptance Criteria

- AC-AHTZW3.1: In the web edition Open recording is offered and opens the browser's file picker for CSV and .db files; a dismissed picker changes nothing.
- AC-AHTZW3.2: A CSV recording, ours or the vendor's, opens with the same row count, skipped count, left-out count and statistics as the desktop app reports for that file.
- AC-AHTZW3.3: A vendor .db recording opens the same way, with the same rows kept and skipped as on the desktop.
- AC-AHTZW3.4: The opened file's chart shows its readings over its time range, as the desktop's recording view does.
- AC-AHTZW3.5: A file that is no recording is refused with the desktop's message, and Cancel ends a running open.

## Technical Design

The CSV reader and the .db row rules move behind the shared recorder crate and reach the browser through the wasm module; the .db file is read with sql.js. The opened rows feed the web history and statistics the live session uses, and the recording-file range and close commands answer from them.

## Testing

Rust tests for the shared .db row rules; Vitest for the web open over CSV and .db fixtures (ours and vendor-shaped) and for the range and close commands; the desktop recording-file tests stay green.

## Notes

Asked for after the web parity HIL re-run (2026-11-09). Same parser as the desktop.
