---
title: A settings change in the browser misses another program on the serial port
milestone: M_ZFZQE7
status: active
archived_at: null
id: fr_01M70N4CYAK860D6Y2A1CJ3QC9
created_at: 2026-11-09T17:19:12.000Z
changelog_category: Fixed
kind: bug
---

# A settings change in the browser misses another program on the serial port

## Summary

On macOS the browser can open the meter's serial port while another program already uses it.
The two programs then read each other's replies.
A settings change wrote the meter, showed a confusing reply error and made the other program fail.
Now the browser notices replies it never asked for and stops before writing.
It shows the same "in use by another program" message as when the port cannot be opened.

## Requirement

### Steps to reproduce
1. On macOS, run the CLI's live readout over the serial port.
2. In the web edition, change a setting and press Apply.

### Expected
"The meter's serial port is in use by another program." and nothing is written.

### Actual
The write lands, the panel shows "No reply with the request's id after 3 re-reads." and the CLI exits with a protocol error.

## Acceptance Criteria

- AC-CJ3QC9.1: Regression: when a reply on the settings port is not one the settings change could have asked for, the change stops with the in-use message and sends no write.
- AC-CJ3QC9.2: Another program's replies already waiting on the port when the change starts count the same way: the in-use message and no write.
- AC-CJ3QC9.3: The in-use refusal releases the port and leaves the panel ready for another try.
- AC-CJ3QC9.4: A settings change with nobody else on the port works as before.

## Technical Design

### Root cause
The in-use message fired only when opening the port failed; macOS lets a second program open the port, so the conflict surfaced later as id mismatches.

### Fix
The settings pipe classifies each frame it reads, also those waiting before its first request: a data reply for another attribute than SETTINGS can only answer someone else's request, so the pipe refuses with the in-use error; the two reads before the write see it first.

## Testing

Vitest over a fake serial port that also carries another program's traffic.

## Notes

Found by the web parity HIL (2026-11-09, step P6).

## Hardware evidence (parity HIL 3 and 4)

- [x] AC-CJ3QC9.1 and .2: the parity HIL 3 (74904fd, 2026-11-10, qa-runs/WEB.parity-hil3-20261110-0216, P6) and the parity HIL 4 (5198343, 2026-11-10, qa-runs/WEB.parity-hil4-20261110-1037, P6) with `powerz --transport cdc watch --source cdc` on the port, Apply showed the in-use message in 27–250 ms and nothing was written (Refresh unchanged).
- [x] AC-CJ3QC9.3: the parity HIL 4 (5198343, 2026-11-10, qa-runs/WEB.parity-hil4-20261110-1037, P6) the form stayed open after the refusal; once the CLI was gone the next Apply saved in ≈0.3 s.
- [x] AC-CJ3QC9.4: the parity HIL 4 (5198343, 2026-11-10, qa-runs/WEB.parity-hil4-20261110-1037, P5) 9 of 9 Applies with nobody else on the port succeeded.
