---
title: Change meter settings in the browser
milestone: M_ZFZQE7
status: archived
archived_at: 2026-11-10T07:04:51Z
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
- AC-W8SHX9.2: The write frame is built by the protocol core from the values read and the requested changes, byte for byte as the desktop app builds it; only the writable fields can change (the panel's six and the startup duration, as on the desktop), an unknown key or an out-of-range value is refused before anything is sent, and calibration and auto-rotate are carried as read.
- AC-W8SHX9.3: The read-back must show every changed field at its new value and every other field as read, else the change fails naming the field; nothing is retried. Undecoded bytes that moved come back as a warning, as on the desktop.
- AC-W8SHX9.4: The first change asks the browser for the meter's serial port (a picker filtered to POWER-Z ports, opened from the user's click); a granted port is reused without asking. A cancelled picker, a busy port or a browser without Web Serial ends the change with a plain message and sends nothing.
- AC-W8SHX9.5: After a change that took, a queue stream restarts at the session's rate, as the desktop app does.
- AC-W8SHX9.6: The serial link refuses every frame except SETTINGS reads and the settings write; it never sends the ADCNEW control frames.
- AC-W8SHX9.7: A HID session refuses the change with the basic-readings wording; the web build switches the settings write feature on.

## Technical Design

A Web Serial transport frames binary replies with the native CDC framing helper exported through wasm; no stream is started on that port, so only replies arrive. The settings write sequence mirrors the native write: wasm builds the request from the decoded block and the changes, and a wasm verifier compares the two blocks and returns the warnings. The port is opened for one change and closed after it; the trigger feature reuses the same port picking.

## Testing

Wasm parity tests for the request, verifier and framing; Vitest over a fake serial port for the sequence, refusals, port picking and the queue restart.

## Notes

First run on the real meter needs the user's OK in the HIL tester's tab. The demo meter takes the change over its own CDC twin, so the page works without hardware.

## Hardware evidence (parity HIL 1, 3 and 4)

- [x] AC-W8SHX9.1 and .3: the parity HIL (a93c363, 2026-11-09, qa-runs/WEB.parity-hil-20261109-2013, P5) changed brightness 50→60→50→55→50 with a read-back check each time; the parity HIL 4 (5198343, 2026-11-10, qa-runs/WEB.parity-hil4-20261110-1037, P5) made 9 of 9 logged Applies succeed in 308–313 ms. A failed read-back was not forced on hardware.
- [x] AC-W8SHX9.2, the device half: the parity HIL 3 (74904fd, 2026-11-10, qa-runs/WEB.parity-hil3-20261110-0216, P5) and the parity HIL 4 (5198343, 2026-11-10, qa-runs/WEB.parity-hil4-20261110-1037, P5) re-read SETTINGS after the writes: every other field and the calibration raw values (606×5, 996668×5, 80) unchanged.
- [x] AC-W8SHX9.4: the parity HIL (a93c363, 2026-11-09, qa-runs/WEB.parity-hil-20261109-2013, P5) opened the serial picker on the first change and reused the granted port afterwards; P6 showed "No serial port was chosen, so nothing was sent." on a dismissed picker; the parity HIL 3 (74904fd, 2026-11-10, qa-runs/WEB.parity-hil3-20261110-0216, P6) and the parity HIL 4 (5198343, 2026-11-10, qa-runs/WEB.parity-hil4-20261110-1037, P6) showed the in-use message with `powerz … watch --source cdc` on the port, and nothing was written.
- [x] AC-W8SHX9.5: the parity HIL (a93c363, 2026-11-09, qa-runs/WEB.parity-hil-20261109-2013, P5) and the parity HIL 4 (5198343, 2026-11-10, qa-runs/WEB.parity-hil4-20261110-1037, P5) kept the queue stream at 50 SPS after each change.
- [x] AC-W8SHX9.7, the HID half: the parity HIL (a93c363, 2026-11-09, qa-runs/WEB.parity-hil-20261109-2013, P4) over HID the settings panel refused with the not-available-over-HID wording.
