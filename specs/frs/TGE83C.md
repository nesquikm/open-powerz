---
title: Desktop PD analyzer view
milestone: M_NE3C7K
status: active
archived_at: null
id: fr_01M5HVWJ41WV7P8G50RPTGE83C
created_at: 2026-10-22T13:12:57.217Z
changelog_category: Added
---

# Desktop PD analyzer view

## Summary

The desktop app gets a PD analyzer view for the connected meter.
It starts and stops a capture and lists every message and event in a fast table.
Selecting a row shows the decoded fields and the raw bytes.
GoodCRC messages are hidden by default so the conversation is easy to read, and other filters narrow it further.
The view stays smooth with very long captures and works with the keyboard and a screen reader.

## Requirement

The desktop app shows a live PD capture from the connected meter or the demo device as a filterable packet table with a details pane.

## Acceptance Criteria

- AC-TGE83C.1: A PD view has Start and Stop for the connected meter. The demo device offers the simulator's PD scenario, and both buttons are disabled with a reason when no device is connected.
- AC-TGE83C.2: The packet table shows, per row, the index, time since capture start, SOP, message name, message ID, direction, object count, spec revision, VBUS and IBUS at that time, and the summary line. Event rows are styled differently from message rows.
- AC-TGE83C.3: GoodCRC rows are hidden by default and a toggle shows them. Filters by message type and by SOP narrow the table without dropping captured data.
- AC-TGE83C.4: Selecting a row shows its decoded field tree and its raw bytes in hex, with the bytes of the selected field highlighted.
- AC-TGE83C.5: While capturing, the table follows new rows. Scrolling up pauses following, and a Follow button resumes it.
- AC-TGE83C.6: With 100,000 records the table scrolls and filters without a frame longer than 50 ms in the mocked-backend perf test, using a virtual list.
- AC-TGE83C.7: Clear empties the capture after the user confirms. The table and details work with the keyboard alone, rows have screen reader labels, and the axe check passes.
- AC-TGE83C.8: The view follows the light, dark and system themes, and Playwright tests on the mocked backend cover capture, filters, details and follow.

## Technical Design

The backend runs the sniffer session from the capture core FR in the device worker and sends batched records to the UI, like the measurement stream. The UI keeps records in a column store and renders a virtual table. Decoding happens in the backend with the PD decoder, and the UI receives summaries plus field trees on demand. Record indices never restart, not even on Clear, so rows sent before a Clear are recognised and dropped.

## Testing

Rust worker tests against the simulator's PD scenario, Vitest for the record store and filters, and Playwright with axe on the mocked backend, one browser project per run with one worker locally.

## Notes

The real-meter view check rides on the capture core FR's pass-through session.
