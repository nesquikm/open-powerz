---
title: WebUSB connect gives up when recovery cancels the pending read
milestone: M_QFTCWW
status: archived
archived_at: 2026-11-08T21:34:13Z
id: fr_01M6YBWDNTDVP70T4D6F7T1YGJ
created_at: 2026-11-08T19:59:34.000Z
changelog_category: Fixed
kind: bug
---

# WebUSB connect gives up when recovery cancels the pending read

## Summary

On a first try, the browser edition showed "The meter did not answer in time" without trying again.
Stale data from an earlier session answered the first connect, so the driver started its recovery.
Clearing the stuck endpoint cancelled the read still waiting on it.
The transport took that cancelled read as an unplugged meter.
Recovery now skips reads it cancelled itself and goes on to the second connect.
The debug log also shows more of each frame.

## Requirement

### Steps to reproduce
1. Leave stale data on the meter's IN pipe (an earlier session ended mid-transfer).
2. Open the web edition and connect to the KM003C.

### Expected
The driver recovers the pipe and the second CONNECT signs in.

### Actual
"The meter did not answer in time." after the recovery line, with no drain and no second CONNECT in the debug log (web HIL run WEB.hil2-20261108-2223, W2 try 1).

## Acceptance Criteria

- AC-7T1YGJ.1: During recovery, an IN transfer that was pending before it and rejects is skipped as cancelled; the transport is not ended as disconnected.
- AC-7T1YGJ.2: During recovery, a halt clear that fails is logged and recovery goes on to the drain.
- AC-7T1YGJ.3: Regression: the driver recovery test (`driver.recovery.test.ts`, fake device that rejects the pending read when its halt is cleared) shows that after such a recovery, the driver sends the second CONNECT and signs in when the meter answers it.
- AC-7T1YGJ.4: A real unplug (the browser's disconnect event) during or after recovery still ends the transport as disconnected.
- AC-7T1YGJ.5: The debug log adds the last 4 bytes of any IN frame longer than 8 bytes, and the error name of a failed transfer or halt clear.

## Technical Design

### Root cause
The timed-out re-read stays queued; recover clears the IN halt under it. A rejected transfer and a failed halt clear both called the lost path, which ends the transport, so the drain read failed and recover rejected before the second CONNECT.

### Fix
Mark the transport as recovering; within it, rejections and halt-clear failures are logged and skipped, not treated as an unplug.

## Testing

Vitest with the fake USB device rejecting the pending transfer when its halt is cleared.

## Notes

Whether the reply was glued behind 64 stale bytes is left to the HIL with the extended log.

## Hardware evidence (web HIL rounds 2 and 3)

- [x] AC-7T1YGJ.5, the frame half: round 3 (qa-runs/WEB.hil3-20261109-0101/console-W2-W5.log) logged the last 4 bytes of IN frames longer than 8 bytes.
- AC-7T1YGJ.1 to .4 stay unticked on hardware: recovery never fired in round 3 (no stale data at open). Their evidence is automated (webusb.cancelled.test.ts, driver.recovery.test.ts).
