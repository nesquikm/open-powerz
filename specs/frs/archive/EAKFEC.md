---
title: Open a PD capture and import a vendor PD log in the browser
milestone: M_ZFZQE7
status: archived
archived_at: 2026-11-10T07:04:51Z
id: fr_01M7147BC8BDNJ1126ZVEAKFEC
created_at: 2026-11-09T21:42:57.000Z
changelog_category: Added
---

# Open a PD capture and import a vendor PD log in the browser

## Summary

The web edition's PD analyzer can now open a saved PD capture (.pdcap) and import a vendor PD log (.sqlite), as the desktop app does.
Both buttons used to look live and then fail with "Desktop app only".
The files are read with the desktop app's own rules, so the same rows, timeline and details show up in both apps, and a capture saved in the browser opens again in the browser.

## Requirement

In the web edition, Open capture and Import vendor log in the PD analyzer open the browser's file picker and show the chosen file in the PD view with no meter, exactly as the desktop app shows that file. Save capture downloads a .pdcap file that Open capture reads back. No button in the web edition looks available and then refuses with "Desktop app only" on click.

## Acceptance Criteria

- AC-EAKFEC.1: In the web edition Open capture opens the browser's file picker for .pdcap files and Import vendor log for .sqlite files; a dismissed picker changes nothing.
- AC-EAKFEC.2: A .pdcap file opens with the same rows, status samples, dropped count, damaged line and record details as the desktop app shows for that file.
- AC-EAKFEC.3: A vendor PD log opens with the same rows, status samples, record details and skipped, oversized and truncated counts as the desktop app's import of that file.
- AC-EAKFEC.4: A file that is no pdcap file or no vendor PD log is refused with the desktop's message, and the capture shown before stays.
- AC-EAKFEC.5: The opened capture answers record details, the cable identity it holds, Clear and release with no meter, as on the desktop.
- AC-EAKFEC.6: A capture saved with Save capture in the browser opens again in the browser with the rows it was saved with.
- AC-EAKFEC.7: No control in the web edition stays live and then refuses with "Desktop app only" on click: a feature the browser cannot offer is shown disabled as "Desktop app only".

## Technical Design

The desktop PD capture (`PdCapture`) and the vendor log row rules move into `powerz-protocol::pd_capture`, shared by the desktop app and the wasm module. The wasm module opens pdcap bytes and takes vendor log rows that sql.js reads with the desktop's SQL. The web backend keeps the opened capture in the page for details, cable identity, Clear and release.

## Testing

Committed synthetic fixtures (`pd-capture.pdcap`, `pd-vendor.sqlite`) with one expected view (`pd-open-expected.json`): a Rust test checks the desktop's open and import against it, a Vitest suite checks the web backend's open and import over real wasm and sql.js against the same file. Vitest for the pickers, refusals, opened-capture commands and the save round trip; a sweep test for live controls that refuse as desktop only.

## Notes

Found by the docs audit (2026-11-10). Same parser as the desktop.

## Hardware evidence (parity HIL 3 and 4)

- [x] AC-EAKFEC.1, the .pdcap half: the parity HIL 3 (74904fd, 2026-11-10, qa-runs/WEB.parity-hil3-20261110-0216, P17) Open capture opened a picker and loaded the saved capture; the parity HIL 4 (5198343, 2026-11-10, qa-runs/WEB.parity-hil4-20261110-1037, P17) a real click opened it and a cancel changed nothing.
- [x] AC-EAKFEC.5, the record-details half: the parity HIL 3 (74904fd, 2026-11-10, qa-runs/WEB.parity-hil3-20261110-0216, P17) after a fresh page load the reopened capture's row #174 details matched those before the save, byte for byte.
- [x] AC-EAKFEC.6: the parity HIL 3 (74904fd, 2026-11-10, qa-runs/WEB.parity-hil3-20261110-0216, P17) 211 records saved as `powerz-pd-20261110-094453.pdcap` reopened as rows #1–#211.
- [ ] AC-EAKFEC.3 (and the .sqlite halves of .1 and .4): not verified on hardware. No vendor PD .sqlite was at hand in the parity HIL 3, so Import vendor log was skipped; the synthetic fixture covers it in Rust and Vitest. Left open until a vendor log is imported on the meter's PC.
