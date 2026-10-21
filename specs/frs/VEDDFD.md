---
title: Resync the UI with a running session on load
milestone: M_R4WG4Q
status: active
archived_at: null
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

On load, the UI asks the backend for the running session, if any, and re-attaches to it: the stream moves to the new page, and the connection, rate, recording, chart and statistics show the session as it is.

## Acceptance Criteria

- AC-VEDDFD.1: On load the UI asks the backend whether a session is running; with none, it starts disconnected exactly as before.
- AC-VEDDFD.2: With a session running, the UI re-attaches without reconnecting: the meter stays connected, new batches reach the new page, and the picker, the status badge, the rate and the rate-lock note show that session.
- AC-VEDDFD.3: An open recording keeps running through the reload, and the UI shows it as recording, with its elapsed time and row count picked up from the backend.
- AC-VEDDFD.4: The chart and the statistics refill from the session history and the backend's statistics, so "Whole session" shows everything recorded before the reload.
- AC-VEDDFD.5: After a re-attach only the new page receives events, and Disconnect, Record, Stop, rate changes and Export work on the re-attached session as on a fresh one.

## Technical Design

The worker's event sink becomes swappable: the device manager keeps a handle to the current sink and replaces it on attach, so the old page's channel gets nothing more. A new attach command takes the page's channel and returns the running session's snapshot (device, model, firmware, serial, rate and lock, session id, recording state with elapsed time and rows), or nothing. The UI calls it on load before listing devices, rebuilds its session and recording state from the snapshot, and lets the chart refill from the history it already queries. The mock backend gains the same command so a page reload can be tested.

## Testing

Rust: attach on a manager with and without a running session, the sink swap (the old sink gets nothing after attach), and the recording snapshot. Vitest: session and recording state rebuilt from a snapshot, and the attach call in the backend seam and the mock. Playwright: reload the page while streaming, and while recording, then check the badge, the recording timer, the chart history and that Stop and Disconnect still work.

## Notes

From the hardware review: a webview reload, in development or in a release build, left a stuck "already connected" state. The user chose to re-attach seamlessly rather than stop the session. An export running during a reload still completes; its progress is not shown again.
