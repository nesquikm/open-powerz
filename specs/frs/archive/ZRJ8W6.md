---
title: The trigger session close is lost when the charger is attached
milestone: M_PVFHM6
status: archived
archived_at: 2026-11-07T07:23:18Z
id: fr_01M6S7XXDRWR64YNHE4DZRJ8W6
created_at: 2026-11-06T20:13:47.321Z
changelog_category: Fixed
kind: bug
---

# The trigger session close is lost when the charger is attached

## Summary

With the charger attached, leaving the Trigger tab or quitting lost the trigger session close.
The way back to 5 V waited three seconds for offers, ran twice, and quitting did not wait for it.
Now closing sends the Hard Reset without waiting, never repeats the way back, and quitting waits for the close.

## Requirement

### Steps to reproduce
1. In the Trigger view, detect a charger and accept a 9 V request.
2. Press Detect again, so the listing ends in a protocol other than PD.
3. Leave the Trigger tab, then quit the app within a few seconds.

### Expected
Leaving sends the way back to 5 V and then the session close. Quitting ends the session with the close. The app log records the close every time.

### Actual
Device run, 2026-11-06, build d52c275: leaving sent a 5 V request and a Hard Reset, then a reset three seconds later, then the 5 V request and the Hard Reset again. The app disconnected one second later. No session close appeared in the app log.

## Acceptance Criteria

- AC-ZRJ8W6.1: With the charger attached, a PD contract from before and another protocol entered since, leaving the Trigger tab sends the 5 V request, the Hard Reset and the reset once each, then the session close, all within the quit wait, while the Hard Reset's offers take three seconds.
- AC-ZRJ8W6.2: In that state, quitting the app from the Trigger tab sends the way back to 5 V and then the session close before the app exits, and the app log records the close.
- AC-ZRJ8W6.3: Quitting while the close of a left Trigger tab is still under way waits for it, within the quit wait, so the session close is sent and in the app log before the app exits.
- AC-ZRJ8W6.4: The close never repeats a way back to 5 V that leaving the tab already sent, whatever that way back's outcome.
- AC-ZRJ8W6.5: Regression: with the charger gone, leaving and quitting still send no 5 V request and no Hard Reset, and the session close is in the app log.
- AC-ZRJ8W6.6: Regression: pressing Back to 5 V still waits for the charger's new offers after a Hard Reset, and the session stays open.

## Technical Design

### Root cause
After a Detect the last entry is a legacy protocol, so the PD way back's 5 V request fails and the Hard Reset waits three seconds for offers. The failed way back is not marked done, so the close repeats it. Quit's shutdown skips a session that the tab's close already took.

### Fix
On leave and close the Hard Reset fallback is written on the safety path, with no offers wait; the 5 V request and the legacy reset stay. A way back that ran to its end marks the session at 5 V. Shutdown waits for the meter's PDM holds within the quit wait.

## Testing

Rust trigger tests on a port whose Hard Reset offers take three seconds, for leave, close and quit with the charger attached and gone; a device-manager test for quit during a close.

## Hardware evidence

- [x] AC-ZRJ8W6.1: the M7C pdm-close run (8ea9bee, 2026-11-07, qa-runs/M7C.verify-pdm-20261107-0109, B2) left the Trigger tab after a 9 V contract and a second Detect; `pd req=1`, `pd hrst`, `reset` and `pdm close` were logged once each, in the same second.
- [x] AC-ZRJ8W6.2: the same run (B3) quit from the tab with a 9 V contract; `pd req=1` and `pdm close` were logged before `disconnected`, and the process was gone 0.43 s after the click.
- [x] AC-ZRJ8W6.3, as far as that run covers it: every quit (B3, B4) logged `pdm close` before the disconnect. A quit timed to land inside a close still under way was not provoked on hardware; the race itself is pinned by the device-manager tests.

## Notes

Reopens JV54SQ AC.4 and AC.5. The PD contract flag stays set after a Detect; it is kept, since the way back must not skip a contract that may stand.
