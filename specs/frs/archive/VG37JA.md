---
title: WebUSB sign-in to the real meter fails where the native bulk path works
milestone: M_QFTCWW
status: archived
archived_at: 2026-11-08T21:34:13Z
id: fr_01M6Y285JHFZYD6EH9N9VG37JA
created_at: 2026-11-08T17:11:43.646Z
changelog_category: Fixed
kind: bug
---

# WebUSB sign-in to the real meter fails where the native bulk path works

## Summary

The browser edition could not sign in to a real KM003C, while the desktop app could.
The browser driver skipped three steps the desktop path takes.
It never cleared stale data at open and never recovered after a silent first connect.
It also expected a memory read in one piece when the meter sends two.
It now follows the desktop sequence, and an opt-in debug log prints every frame.

## Requirement

### Steps to reproduce
1. Build the web edition at f1b74e8 and open it in Chrome on macOS with a KM003C on the data port (no other program holding it).
2. Click Connect, pick "POWER-Z KM003C" in the WebUSB picker.
3. Without reloading, click Connect again.

### Expected
The meter connects, its device info shows and readings stream, as in the desktop app on the same meter.

### Actual
Try 1: "The meter did not answer in time." Try 2: "memory read returned 0 bytes, expected 64". The console is empty (web HIL run WEB.hil-20261108-1548, step W2).

## Acceptance Criteria

- AC-VG37JA.1: Opening a meter over WebUSB discards every IN transfer already waiting (reads with a 50 ms wait until one times out) before the first request is sent, as the native bulk open does.
- AC-VG37JA.2: The first CONNECT waits 500 ms; when it gets no matching ACCEPT (a timeout, or only replies with another id), the driver recovers the transport once (clears the halt on the bulk OUT and IN endpoints, then discards stale IN data) and sends one more CONNECT that waits 2000 ms. A REJECT or a disconnect fails at once with no retry.
- AC-VG37JA.3: A memory read takes the 20-byte confirmation from the first reply and then reads further IN transfers until it holds the encrypted data rounded up to 16 bytes; a confirmation that already carries the data in the same reply (the demo meter) still decodes.
- AC-VG37JA.4: A WebUSB transfer that ends with status "stall" clears the halt on that endpoint and fails as a timeout, so connect recovery can run; it never resolves as an empty reply.
- AC-VG37JA.5: With the page opened with a debug query parameter or a debug flag set in local storage, the WebUSB transport logs every frame sent and received to the console with its direction, length and its first 8 bytes in hex, and logs open, drain, recovery and close; without either, it logs nothing.
- AC-VG37JA.6: Regression: a fake USBDevice that replays the real meter's behaviour (a stale DISCONNECT waiting at open, a first CONNECT that gets no reply, memory-read data in a separate transfer after its confirmation) connects through the web driver, reads the device info and starts the queue; the same fake fails before the fix and passes after it.

## Technical Design

### Root cause
The native session (bulk transport open, session connect, memory read) drains stale IN data at open, retries an unanswered first CONNECT after clearing both endpoint halts, and reads memory-read data as extra IN transfers after the confirmation. The TS driver did none of these: the WASM decoder expects one joined reply, which only the demo meter gives, so the real 20-byte confirmation decoded as 0 data bytes.

### Fix
Add an optional recover step to the transport seam, a drain at WebUSB open, the two-step CONNECT and a multi-transfer memory read in the driver, stall handling, and the debug log.

## Testing

Vitest over a fake USBDevice that scripts the meter's transfers per endpoint, plus driver tests for the CONNECT retry, the split memory read and the debug log flag.

## Notes

The native rules are in the protocol doc's session flow table. Proven on the device only by the next web HIL run.

## Hardware evidence (web HIL rounds 2 and 3)

- [x] AC-VG37JA.1: rounds 2 and 3 logged "drain: discarded N stale frame(s)" on every open (qa-runs/WEB.hil2-20261108-2223, qa-runs/WEB.hil3-20261109-0101).
- [x] AC-VG37JA.2: round 2 (W2 try 1) logged one recovery after a silent first CONNECT. The fix of 7T1YGJ made the second CONNECT succeed in tests; recovery did not fire in round 3.
- [x] AC-VG37JA.3: both rounds reassembled the memory read from a 20-byte confirmation plus separate data frames.
- [x] AC-VG37JA.5: the ?debug page logged every frame with direction, length and first bytes in both rounds.
