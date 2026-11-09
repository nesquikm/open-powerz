---
title: Change meter settings in the browser
milestone: M_ZFZQE7
status: active
archived_at: null
id: fr_01M6ZX9Y8BB9F6MCWZ2DW8SHX9
created_at: 2026-11-09T10:22:48.075Z
changelog_category: Added
---

# Change meter settings in the browser

## Summary

The meter takes a settings change only on its virtual serial port, so the desktop app sends it there.
The web edition can reach that port through Web Serial in Chrome and Edge.
With it, the settings panel can change the meter's settings in the browser too.
The safeguards stay as on the desktop: two matching reads, one write and a read-back check.
Calibration is never written.

## Requirement

A WebUSB session can change the same writable settings as the desktop app. The write and both reads go over the meter's serial port opened with Web Serial; the browser asks for the port the first time and reuses the granted one afterwards.

## Acceptance Criteria

- AC-W8SHX9.1: A settings change reads SETTINGS twice, sends one type-0x0A settings write and reads SETTINGS again, all over the meter's serial port; two reads that differ refuse the write and nothing is sent.
- AC-W8SHX9.2: The write frame is built by the protocol core from the values read and the requested changes, byte for byte as the desktop app builds it; only the six writable fields can change, an unknown key or an out-of-range value is refused before anything is sent, and calibration and auto-rotate are carried as read.
- AC-W8SHX9.3: The read-back must show every changed field at its new value and every other field as read, else the change fails naming the field; nothing is retried. Undecoded bytes that moved come back as a warning, as on the desktop.
- AC-W8SHX9.4: The first change asks the browser for the meter's serial port (a picker filtered to POWER-Z ports, opened from the user's click); a granted port is reused without asking. A cancelled picker, a busy port or a browser without Web Serial ends the change with a plain message and sends nothing.
- AC-W8SHX9.5: After a change that took, a queue stream restarts at the session's rate, as the desktop app does.
- AC-W8SHX9.6: The serial link refuses every frame except SETTINGS reads and the settings write; it never sends the ADCNEW control frames.
- AC-W8SHX9.7: A HID session refuses the change with the basic-readings wording; the web build switches the settings write feature on.

## Technical Design

A Web Serial transport frames binary replies with the native CDC framing helper exported through wasm and drops stream frames, as the native CDC transport does. The settings write sequence mirrors the native write: wasm builds the request from the decoded block and the changes, and a wasm verifier compares the two blocks and returns the warnings. The serial link is shared per meter with the trigger feature.

## Testing

Wasm parity tests for the request, verifier and framing; Vitest over a fake serial port for the sequence, refusals, port picking and the queue restart.

## Notes

First run on the real meter needs the user's OK in the HIL tester's tab.
