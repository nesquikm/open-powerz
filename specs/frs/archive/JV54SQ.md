---
title: The trigger view after the charger went away
milestone: M_PVFHM6
status: archived
archived_at: 2026-11-07T07:23:18Z
id: fr_01M6RMDMJEZ927P9DCDGJV54SQ
created_at: 2026-11-06T14:32:51.022Z
changelog_category: Fixed
kind: bug
---

# The trigger view after the charger went away

## Summary

Once the Trigger view said the charger went away, its Back to 5 V button stayed on screen.
There was nothing left to return from.
Leaving and quitting then tried the way back to 5 V, which waits for a charger that is not there.
So quitting gave up before closing the session.
Now the button hides while the charger is gone.
Closing skips the pointless way back, so the session close always goes out and is logged.

## Requirement

### Steps to reproduce
1. In the Trigger view, detect a PD charger and accept a 9 V request.
2. Unplug the charger until the view says the charger went away and to press Detect.
3. Leave the Trigger tab, then quit the app.

### Expected
No Back to 5 V button while the charger is gone. Closing the session sends the close and the app log records it.

### Actual
Device run, 2026-11-06, build 46f79cb: Back to 5 V stayed on screen. Leaving the tab sent a 5 V request and a Hard Reset; quitting sent a reset, a 5 V request and a Hard Reset, and no session close appeared in the app log.

## Acceptance Criteria

- AC-JV54SQ.1: While the view says the charger went away, the Back to 5 V button is not shown; it comes back with the next session that has offers.
- AC-JV54SQ.2: Regression: with the charger gone after a PD contract, leaving the view and then closing the session sends no 5 V request and no Hard Reset, sends the session close, and finishes well within the quit wait.
- AC-JV54SQ.3: With the charger gone after a legacy protocol was entered, closing still sends a reset before the session close.
- AC-JV54SQ.4: The session close is in the app log on quit after the charger went away, as every message the session writes must be.
- AC-JV54SQ.5: With the charger still attached, leaving the view and closing behave as before: the way back to 5 V, then the session close.

## Technical Design

### Root cause
With the charger gone, the way back sends a 5 V request, then a Hard Reset whose negotiation waits three seconds for source offers that never come. That outlasts the quit's three-second wait, the session thread is left behind and the app exits before the close is written. The view showed the button whenever a session was open.

### Fix
The backend's way back skips the PD steps when the charger is marked gone, keeping the legacy reset. The page hides the button while the charger is gone.

## Testing

Rust trigger tests on the simulated charger for the gone, legacy and attached cases with written lines and log lines; Vitest for the button condition; Playwright on the mock.

## Hardware evidence

- [x] AC-JV54SQ.1: the M7C verify run (d52c275, 2026-11-07, qa-runs/M7C.verify-20261106-2044, part A) showed no Back to 5 V after an armed 9 V contract's charger was unplugged; it came back with the next Detect's offers.
- [x] AC-JV54SQ.4, the close-logging half: the pdm-close run (8ea9bee, 2026-11-07, qa-runs/M7C.verify-pdm-20261107-0109) logged `pdm close` on leaving (B2) and on both quits (B3, B4). Those quits had the charger attached; a quit with the charger gone was not run on hardware.
- [x] AC-JV54SQ.5: the same run (B2) left the view with the charger attached and logged the way back to 5 V, then `pdm close`; VBUS ended at 5.18 V. The d52c275 run (part A, finding A-1) had no close line; 8ea9bee fixed that.

## Notes

Found in the M7c carry-over device run, part U. A charger plugged in later starts at 5 V on its own.
