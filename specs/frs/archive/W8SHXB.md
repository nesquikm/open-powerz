---
title: Startup logo upload in the browser
milestone: M_ZFZQE7
status: archived
archived_at: 2026-11-10T07:04:51Z
id: fr_01M6ZX9Y8BB9F6MCWZ2DW8SHXB
created_at: 2026-11-09T10:22:48.075Z
changelog_category: Added
---

# Startup logo upload in the browser

## Summary

The startup logo tool now works in the browser over WebUSB.
The user picks a picture or a built-in logo, sees the preview and uploads it.
Removing the logo works too.
The desktop app's limits apply: the write touches only the logo area, and everything is read back.
The startup duration is only ever set to off or three seconds.
A saved settings check after the write works as on the desktop.

## Requirement

A WebUSB session to a KM003C runs the desktop app's logo flow: choose (a browser file picker), preview, read the current logo, upload a picture or a built-in logo, remove, and check that the meter saved its settings. Every write and read goes through the protocol core's logo paths and guards.

## Acceptance Criteria

- AC-W8SHXB.1: Choosing a picture opens the browser's file picker for images; the chosen file's preview (size, mode, pixels) equals the desktop app's preview of the same file, and the built-in logos preview the same as on the desktop.
- AC-W8SHXB.2: Upload and remove refuse before any write when the session is not authenticated, SETTINGS was not read in this session, or the meter is not a KM003C, with the desktop app's reasons.
- AC-W8SHXB.3: An upload writes only 0x4A chunks inside 0x300000–0x31D4FF, then one SET_DATA 0x48 attribute 0x1C with value 2, then reads the area back and compares it byte for byte; remove writes one 16-byte 0xFF chunk, the value 0 and reads it back. Any other address, attribute or value is refused by the protocol core before a frame exists; nothing is retried.
- AC-W8SHXB.4: Progress reports chunks and read-back steps as on the desktop, and the meter's shortcuts and other commands are held while a write runs.
- AC-W8SHXB.5: Reading the current logo (head or full picture) uses only memory reads inside the logo window, paced as on the desktop.
- AC-W8SHXB.6: The settings check after a write reports saved, needs a menu save, or calibration changed, as on the desktop, from SETTINGS reads only.
- AC-W8SHXB.7: A HID session refuses every logo command with the basic-readings wording; the web build switches the logo feature on.

## Technical Design

Wasm exports image loading and logo encoding, the chunk and flag frame builders behind the native logo guards, the read plan over the logo window and the settings-saved check. The driver sequences them inside its lock with the native pacing. The web file picker keeps the chosen bytes under an opaque token that stands in for the desktop path, so the UI calls stay the same.

## Testing

Wasm parity tests for preview, frames and guard refusals; Vitest over the demo meter for upload, remove, read, progress and refusals.

## Notes

Writes the meter's flash: each staged write on the real meter needs the user's OK in the HIL tester's tab.

## Hardware evidence (parity HIL 1, 3 and 4)

- [x] AC-W8SHXB.1, the device half: the parity HIL (a93c363, 2026-11-09, qa-runs/WEB.parity-hil-20261109-2013, P8-choose) opened the image picker; a 1280×800 PNG previewed as 240×150 opaque / 200×125 transparent with chunk counts. Not compared side by side with the desktop app on hardware.
- [x] AC-W8SHXB.3: the parity HIL (a93c363, 2026-11-09, qa-runs/WEB.parity-hil-20261109-2013, P8b) uploaded the built-in Mascot (469 chunks) and read it back ("Logo uploaded: read-back verified."); P8c removed it the same way; the user saw both on the meter after a replug. the parity HIL 3 (74904fd, 2026-11-10, qa-runs/WEB.parity-hil3-20261110-0216, P8) repeated upload, remove and restore.
- [x] AC-W8SHXB.4: the parity HIL (a93c363, 2026-11-09, qa-runs/WEB.parity-hil-20261109-2013, P8b) held rate, mode, device, Disconnect, Record and Export during the write; the parity HIL 3 (74904fd, 2026-11-10, qa-runs/WEB.parity-hil3-20261110-0216, P8) showed "Reading chunk N of 470" progress.
- [x] AC-W8SHXB.5: the parity HIL (a93c363, 2026-11-09, qa-runs/WEB.parity-hil-20261109-2013, P8a), the parity HIL 3 (74904fd, 2026-11-10, qa-runs/WEB.parity-hil3-20261110-0216, P8) and the parity HIL 4 (5198343, 2026-11-10, qa-runs/WEB.parity-hil4-20261110-1037, P4) read the stored logo ("holds a 200 × 200 logo (transparent)") and showed it.
- [x] AC-W8SHXB.6: the parity HIL (a93c363, 2026-11-09, qa-runs/WEB.parity-hil-20261109-2013, P8c) guided the menu save and reported "The meter saved its settings. Startup screen: off."; the parity HIL 3 (74904fd, 2026-11-10, qa-runs/WEB.parity-hil3-20261110-0216, P8) reported "Startup screen: 3 s.".
- [x] AC-W8SHXB.7: the parity HIL 3 (74904fd, 2026-11-10, qa-runs/WEB.parity-hil3-20261110-0216, P4) and the parity HIL 4 (5198343, 2026-11-10, qa-runs/WEB.parity-hil4-20261110-1037, P4) over HID the tab showed the HID note once, with Upload, Remove and Read disabled.
