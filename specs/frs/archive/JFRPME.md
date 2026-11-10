---
title: The browser's session history forgets the temperature
milestone: M_ZFZQE7
status: archived
archived_at: 2026-11-10T07:04:51Z
id: fr_01M7128S28V1XMSQCE4VJFRPME
created_at: 2026-11-09T21:08:47.000Z
changelog_category: Fixed
kind: bug
---

# The browser's session history forgets the temperature

## Summary

In the web edition a session export lost the meter's temperature.
Export CSV left the TEMP column empty and Export .db wrote TEMP as NULL, while the desktop app fills both.
Now the browser keeps the temperature in its session history as the desktop app does, so both exports carry it, and a reopened export shows it again.

## Requirement

### Steps to reproduce
1. Connect a KM003C over WebUSB in the web edition and let it stream for a few seconds.
2. Press Export CSV, then Export .db.

### Expected
Each row's TEMP holds the meter's temperature at that time, as in the desktop app's exports.

### Actual
TEMP is empty in the CSV and NULL in the .db for every row.

## Acceptance Criteria

- AC-JFRPME.1: Regression: Export CSV in the web edition writes each sample's temperature in TEMP, the latest snapshot reading not older than the snapshot age limit; rows without a current reading keep TEMP empty.
- AC-JFRPME.2: Regression: Export .db in the web edition writes the same temperatures in TEMP, NULL where the CSV is empty.
- AC-JFRPME.3: The history keeps one temperature entry per change, not one per sample or snapshot, and Clear history drops them with the samples.
- AC-JFRPME.4: A web export opened again with Open recording reads the same TEMP values back.

## Technical Design

### Root cause
The web session used the temperature only for the live batch and a recording in progress; its history kept the six readings and no temperature, so the session exports had none to write.

### Fix
A temperature log beside the history, as the desktop's `TempLog`: entries of (first sample index, °C, last session time it applies to), extended while the same reading repeats before it expires. Both session exports look each sample's temperature up in it, in increasing index order.

## Testing

Vitest for the temperature log (lookup, expiry, merge, clear) and for both session exports over the demo meter with temperatures, read back with the CSV and .db readers.

## Notes

Found while fixing AHTZW2 (2026-11-09).

## Hardware evidence (parity HIL 3 and 4)

- [x] AC-JFRPME.1 and .2: the parity HIL 3 (74904fd, 2026-11-10, qa-runs/WEB.parity-hil3-20261110-0216, P14) the record CSV had TEMP on every row and the session .db TEMP 30.20–30.80 °C; the parity HIL 4 (5198343, 2026-11-10, qa-runs/WEB.parity-hil4-20261110-1037, P14) only the first 20 rows (the first 0.38 s after the reconnect) had no TEMP, in both files.
