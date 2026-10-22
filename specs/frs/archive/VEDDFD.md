---
title: Resync the UI with a running session on load
milestone: M_R4WG4Q
status: archived
archived_at: 2026-10-22T07:41:09Z
id: fr_01M5ES0NCPRXHHZ3ZMJ3VEDDFD
created_at: 2026-10-21T08:25:02.616Z
changelog_category: Added
---

# Resync the UI with a running session on load

## Summary

A reload of the window used to leave the app believing no meter was connected.
The backend was still streaming, so Connect then failed with a confusing message.
On load the window now asks the backend what is running and picks the session up again.
The stream, any recording and the chart carry on as if nothing happened.

## Requirement

On load, the UI asks the backend for the running session, if any, and re-attaches to it: the stream moves to the new page, and the connection, rate, recording, chart (including the live windows) and statistics show the session as it is.

## Acceptance Criteria

- AC-VEDDFD.1: On load the UI asks the backend whether a session is running; with none, it starts disconnected exactly as before.
- AC-VEDDFD.2: With a session running, the UI re-attaches without reconnecting: the meter stays connected, new batches reach the new page, and the picker, the status badge, the rate and the rate-lock note show that session.
- AC-VEDDFD.3: An open recording keeps running through the reload, and the UI shows it as recording, with its elapsed time and row count picked up from the backend.
- AC-VEDDFD.4: The chart and the statistics refill from the session history and the backend's statistics, so "Whole session" shows everything recorded before the reload.
- AC-VEDDFD.5: After a re-attach only the new page receives events, and Disconnect, Record, Stop, rate changes and Export work on the re-attached session as on a fresh one.
- AC-VEDDFD.6: Straight after a re-attach, the live 10 s, 1 min and 10 min windows show the samples from before the reload, filled from the session history, and the zoom readout matches the span drawn.

## Technical Design

The worker's event sink becomes swappable. On attach the device manager hands the new page's sink to the worker, which swaps it and reads the recording state on its own thread, so the old page gets nothing more and no recording event falls between the two. The attach command returns the running session's snapshot (device, rate and lock, session id, recording progress), or nothing. The UI holds events until it has applied the snapshot, then seeds the live chart buffer from the history and its run breaks over the longest live window before its first sample, the last minute at a finer resolution. The mock backend gains the same command so a reload can be tested.

## Testing

Rust: attach on a manager with and without a running session, the sink swap (the old sink gets nothing after attach), and the recording snapshot. Vitest: session and recording state rebuilt from a snapshot, the attach call in the backend seam and the mock, events held until the snapshot is applied, and the live buffer seeded from history. Playwright: reload the page while streaming, and while recording, then check the badge, the recording timer, the chart history, that the 10 s window spans the full 10 s straight away, and that Stop and Disconnect still work.

## Notes

From the hardware review, a webview reload left a stuck "already connected" state; the user chose to re-attach seamlessly. An export running during a reload completes unseen. Backfilled points are history buckets: their power is the product of the channel extremes, and hover shows one extreme.
