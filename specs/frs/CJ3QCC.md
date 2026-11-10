---
title: The startup logo tab over HID gives the wrong reason
milestone: M_ZFZQE7
status: active
archived_at: null
id: fr_01M70N4CYAK860D6Y2A1CJ3QCC
created_at: 2026-11-09T17:19:12.000Z
changelog_category: Fixed
kind: bug
---

# The startup logo tab over HID gives the wrong reason

## Summary

Over HID the startup logo tab said the logo needs a KM003C, naming a KM003C.
It should say that the logo is not available over HID.
After reconnecting over WebUSB the old HID error also stayed on the tab.
Now the tab gives the HID reason, names the model correctly, and clears old errors on reconnect.

## Requirement

### Steps to reproduce
1. Connect a KM003C over HID in the web edition and open Startup logo.
2. Disconnect, connect over WebUSB and open Startup logo again.

### Expected
Over HID the tab says the logo is not available over HID; over WebUSB it shows no error from before.

### Actual
"The startup logo needs a KM003C; this meter is a POWER-Z KM003C." over HID, and the HID error stays after the WebUSB reconnect.

## Acceptance Criteria

- AC-CJ3QCC.1: Regression: over HID the upload and remove reasons are the HID note for the startup logo, not the model line.
- AC-CJ3QCC.2: A KM003C named with a product prefix counts as a KM003C; a KM002C still gets the model line.
- AC-CJ3QCC.3: Regression: a new connection clears the tab's previous error and logo read.

## Technical Design

### Root cause
The tab compares the model string exactly, and HID sessions report the USB product name; its error was kept across sessions.

### Fix
The tab takes the basic-readings flag and gives the HID reason first; the model check matches the model token; a session change resets the tab's error.

## Testing

Vitest on the logo panel state for HID, prefixed model names and a reconnect.

## Notes

Found by the web parity HIL (2026-11-09, step P4).

## Hardware evidence (parity HIL 3 and 4)

- [x] AC-CJ3QCC.1: the parity HIL 3 (74904fd, 2026-11-10, qa-runs/WEB.parity-hil3-20261110-0216, P4) over HID the Startup logo tab gave the HID note, not the model line.
- [x] AC-CJ3QCC.2, the KM003C half: the parity HIL 3 (74904fd, 2026-11-10, qa-runs/WEB.parity-hil3-20261110-0216, P8) the "POWER-Z KM003C" meter counted as a KM003C (upload and remove ran).
- [x] AC-CJ3QCC.3: the parity HIL 3 (74904fd, 2026-11-10, qa-runs/WEB.parity-hil3-20261110-0216, P4) and the parity HIL 4 (5198343, 2026-11-10, qa-runs/WEB.parity-hil4-20261110-1037, P4) after Disconnect → WebUSB the HID note was gone.
