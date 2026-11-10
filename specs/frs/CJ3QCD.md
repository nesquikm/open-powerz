---
title: Small stale and mismatched readings in the browser's meter views
milestone: M_ZFZQE7
status: active
archived_at: null
id: fr_01M70N4CYAK860D6Y2A1CJ3QCD
created_at: 2026-11-09T17:19:12.000Z
changelog_category: Fixed
kind: bug
---

# Small stale and mismatched readings in the browser's meter views

## Summary

A few views in the browser showed old or mismatched values.
After an upload or a remove, the startup logo tab still showed the logo read before it.
Showing the stored logo took about forty seconds with no sign of progress.
The rate picker kept a refused rate on show, and an opened record's charge differed from its list row.
Now each of these shows what the meter holds.

## Requirement

### Steps to reproduce
1. Read the startup logo, then upload or remove one.
2. Press Show stored logo.
3. With the trigger panel open, pick 1000 SPS.
4. Open an offline record and compare its charge with the list.

### Expected
The tab drops the old read, the stored-logo read shows progress, the picker shows the rate in use, and the record shows the list's totals.

### Actual
The old logo and size stay, the read shows nothing for about forty seconds, the picker shows 1000 SPS, and the record shows a charge with the opposite sign and a slightly smaller value.

## Acceptance Criteria

- AC-CJ3QCD.1: Regression: after an upload or a remove that succeeded, the logo tab no longer shows the earlier read until the logo is read again.
- AC-CJ3QCD.2: While the browser reads the stored logo, the tab shows how many of the read's chunks are done of the total.
- AC-CJ3QCD.3: Regression: after a refused rate change, the rate picker shows the rate the session streams at.
- AC-CJ3QCD.4: Regression: an opened record shows the same charge and energy as its row in the record list.

## Technical Design

### Root cause
The logo tab kept its last read across writes; the read had no progress callback; the picker's one-way value does not reset when the rate stays the same; the opened record took its totals from the last sample, which runs one interval short and carries the current's sign, while the list shows the meter's stored totals.

### Fix
Clear the read on a finished write; pass chunk progress from the web logo read; resync the picker after a refused change; the opened record takes its list row's charge and energy.

## Testing

Vitest on the logo panel state, the web logo read, the rate decision and the record stats.

## Notes

Found by the web parity HIL (2026-11-09, steps P3, P8 and P12). The detect wording difference (P9) is not a wording bug: both use the same text, the meter answered differently in the two runs.

## Hardware evidence (parity HIL 3)

- [x] AC-CJ3QCD.1: the parity HIL 3 (74904fd, 2026-11-10, qa-runs/WEB.parity-hil3-20261110-0216, P8) the "holds" line and stored preview cleared after the upload and the remove.
- [x] AC-CJ3QCD.2: the parity HIL 3 (74904fd, 2026-11-10, qa-runs/WEB.parity-hil3-20261110-0216, P8a) showed "Reading chunk N of 470" (8→465, 24 s).
- [x] AC-CJ3QCD.3: the parity HIL 3 (74904fd, 2026-11-10, qa-runs/WEB.parity-hil3-20261110-0216, P12) after the refused 1000 SPS the picker showed 50 SPS again.
- [x] AC-CJ3QCD.4: the parity HIL 3 (74904fd, 2026-11-10, qa-runs/WEB.parity-hil3-20261110-0216, P3) the opened A01.d showed 0.037251 Ah and 0.188148 Wh, as its list row.
