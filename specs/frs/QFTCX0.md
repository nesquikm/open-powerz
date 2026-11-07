---
title: Web backend for the desktop UI
milestone: M_QFTCWW
status: active
archived_at: null
id: fr_01M6VHDG2BG62PZDW7K4QFTCX0
created_at: 2026-11-07T17:39:28.900Z
changelog_category: Added
---

# Web backend for the desktop UI

## Summary

The existing app interface runs in a browser with a new web backend behind the same seam the test build already uses.
The user picks a meter with the browser's device picker, or opens the built-in demo meter, and gets the live readings, the chart, statistics, the passive PD analyzer and recording to a downloadable spreadsheet file.
Features that write to the meter, or that need the file system, show that they are available in the desktop app only.

## Requirement

Add a web backend selected by `VITE_BACKEND=web` that implements the UI's backend interface with the session driver, the demo meter and in-browser history, stats and CSV recording, and marks write features as desktop-only.

## Acceptance Criteria

- AC-QFTCX0.1: With `VITE_BACKEND=web` the UI's backend calls go to the web backend; the Tauri path and the mock path are unchanged, and the Tauri and mock builds contain no WebUSB code.
- AC-QFTCX0.2: The device list shows the demo meter and meters the browser already has permission for; a "Connect a meter…" action opens the WebUSB picker filtered to vendor 0x5FC9 directly in the click handler (user gesture), and the picked meter joins the list; plugging and unplugging a permitted meter updates the list.
- AC-QFTCX0.3: Connecting to a meter or the demo shows live readings, the chart, temperature and statistics with the same events as the desktop backend; rate changes restart the queue; history ranges, gaps, range stats and stats reset work over in-browser history.
- AC-QFTCX0.4: The PD analyzer starts and stops passive capture and shows rows and message details; clearing works; saving the capture downloads the pdcap file.
- AC-QFTCX0.5: Recording collects rows in memory and Stop downloads a CSV whose header and rows match the desktop recorder's format for the same samples; session export downloads the whole history as CSV.
- AC-QFTCX0.6: Firmware update, logo upload, settings writes, record delete and protocol triggering are disabled in the web build with the text "Desktop app only" and a link to the releases page; no web backend function for them sends anything to a device.
- AC-QFTCX0.7: Reading settings, the offline record list and record download work only if they use the safe read set; otherwise they are also marked desktop-only.

## Technical Design

`backend.ts` gains a `web` dispatch next to `mock`, loading `lib/web/webBackend.ts` lazily so other builds tree-shake it. The mock's in-memory `HistoryColumns`, stats and CSV writer move to shared modules used by both. Downloads go through a `Blob` and an anchor click. A `features` capability map lets components hide or disable desktop-only controls without forking.

## Testing

Vitest covers the web backend over the demo transport: connect, batches, rate change, history, PD capture, CSV parity with recorder golden output, and that desktop-only calls reject without transport traffic. Existing mock and Tauri tests prove those paths are unchanged.

## Notes

Offline records and settings read stay desktop-only if they need anything beyond the safe read set.
