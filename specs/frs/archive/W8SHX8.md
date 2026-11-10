---
title: Meter settings and offline records in the browser
milestone: M_ZFZQE7
status: archived
archived_at: 2026-11-10T07:04:51Z
id: fr_01M6ZX9Y8BB9F6MCWZ2DW8SHX8
created_at: 2026-11-09T10:22:48.075Z
changelog_category: Added
---

# Meter settings and offline records in the browser

## Summary

The web edition showed "Desktop app only" for the meter settings panel and the offline records view.
Both only read from the meter, so they can work in the browser over WebUSB.
The settings panel lists the meter's settings.
The records view lists the records and opens one in the record chart.
The browser reads exactly what the desktop app reads and nothing else.
A HID session, which gives basic readings only, still refuses both.

## Requirement

Over a WebUSB session the web edition reads the meter's settings and its offline records with the same commands, guards and results as the desktop app. The protocol work runs in the WebAssembly protocol core; the browser code only moves bytes.

## Acceptance Criteria

- AC-W8SHX8.1: Over WebUSB the settings panel shows the same rows (keys, labels, values) the desktop app shows for the same SETTINGS reply; the only request sent is GET_DATA with the SETTINGS attribute.
- AC-W8SHX8.2: Over WebUSB the records view lists the meter's offline records (index, name, interval, points, duration, charge, energy) as the desktop app does; a list request authenticates at level 1 first when the session is below it, and an empty list is not an error.
- AC-W8SHX8.3: Downloading a record reads only that record's own window as listed by the latest list, in reads of at most 256 bytes; a record outside the listed windows, an unlisted record or a record with no samples is refused before any memory read is sent (the fresh list comes first, as on the desktop), with the desktop app's wording.
- AC-W8SHX8.4: The downloaded record (index, name, interval and samples with time, VBUS, IBUS, charge and energy) equals the desktop app's for the same replies, so it opens in the record chart as on the desktop.
- AC-W8SHX8.5: Every memory read the web edition can build is one of the four device-info blocks, a listed record window or (for the logo feature) the logo window; any other address or length is refused by the protocol core before a frame exists.
- AC-W8SHX8.6: A HID session refuses the settings read, the record list and the download with the basic-readings wording, and a WebUSB session whose authentication failed refuses the record list and download with the desktop app's reason; the settings read works without authentication.
- AC-W8SHX8.7: The web build switches these three features on: the settings panel and records view no longer show "Desktop app only", while firmware update and file reveal still do.

## Technical Design

New wasm exports wrap the native settings and record decoders and the record-window guard: a settings decoder giving the panel rows, a record-list decoder that also returns the readable windows, a chunk plan for one record checked against those windows, and a samples decoder. The driver gains settings, list and download commands that run inside its one-request-at-a-time lock between polls; the web session exposes them as the existing meter commands.

## Testing

Rust parity tests in the wasm crate against the native session over the mock transport; Vitest for the driver over the demo meter and recorded byte vectors, including refusals and HID; a Pages e2e check that the panels render with the demo meter.

## Notes

Read-only; no meter writes. The demo meter answers these reads so the page is useful without hardware.

## Hardware evidence (parity HIL 1 and 3)

- [x] AC-W8SHX8.1: the parity HIL (a93c363, 2026-11-09, qa-runs/WEB.parity-hil-20261109-2013, P1) showed the settings rows over WebUSB; the user matched brightness 50%, theme blue and startup logo 3 s on the meter.
- [x] AC-W8SHX8.2: the parity HIL (a93c363, 2026-11-09, qa-runs/WEB.parity-hil-20261109-2013, P2) listed A01.d (1 s, 173 pts, 0:02:52, 0.037251 Ah, 0.188148 Wh); the user confirmed count and duration on the meter. the parity HIL 3 (74904fd, 2026-11-10, qa-runs/WEB.parity-hil3-20261110-0216, P7) listed the new A02.d too.
- [x] AC-W8SHX8.3, the download half: the parity HIL (a93c363, 2026-11-09, qa-runs/WEB.parity-hil-20261109-2013, P3) downloaded A01.d and opened it: 173 samples over 0:00–2:52. The refusals were not exercised on hardware.
- [x] AC-W8SHX8.4: the parity HIL (a93c363, 2026-11-09, qa-runs/WEB.parity-hil-20261109-2013, P3) opened the downloaded record in the record chart; the parity HIL 3 (74904fd, 2026-11-10, qa-runs/WEB.parity-hil3-20261110-0216, P3) showed the same charge and energy as the list (after CJ3QCD).
- [x] AC-W8SHX8.6, the HID half: the parity HIL (a93c363, 2026-11-09, qa-runs/WEB.parity-hil-20261109-2013, P4) over HID the settings panel and records view said they are not available over HID. A failed authentication was not exercised on hardware.
- [x] AC-W8SHX8.7: the parity HIL (a93c363, 2026-11-09, qa-runs/WEB.parity-hil-20261109-2013, P1 and P13) showed settings and records with no "Desktop app only"; the Firmware tab kept Update disabled with "Desktop app only", and no show-in-folder on the web.
