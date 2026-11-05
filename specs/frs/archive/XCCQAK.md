---
title: The firmware update keeps the device list's labels
milestone: M_7PKVA6
status: archived
archived_at: 2026-11-05T07:00:44Z
id: fr_01M6K0WPYG7T8QWY9VAJXCCQAK
created_at: 2026-11-04T10:15:21.296Z
changelog_category: Added
---

# The firmware update keeps the device list's labels

## Summary

During an update the meter jumps to its bootloader and back on its own.
In the r4 hardware run it stayed there only about six seconds.
That was too short for a tester to read the device select.
The progress section now keeps a short history of the meter's list labels.
A tester reads it after the update ends.
The flash itself does not change.

## Requirement

The label the device select gives the updated meter during an update can be checked after the update ends.

## Acceptance Criteria

- AC-XCCQAK.1: While an update runs and after it ends, the Update progress section shows a "Device list" line naming, in order, each label the updated meter's entry took in the device list since the update started, joined by arrows; a label repeats only after a different one.
- AC-XCCQAK.2: Each label is the text the closed device select shows for that entry, so a meter in its bootloader reads with "(bootloader mode)", and with Hide serial numbers on no serial appears.
- AC-XCCQAK.3: The line stays after the update is done, failed or stopped, until the outcome is closed, and a new update starts it afresh.
- AC-XCCQAK.4: A UI test runs a mocked update in which the listed meter turns to bootloader mode and back, and reads the whole line after the update ends, with the switch on and off.

## Technical Design

The firmware store notes the updated meter's listing whenever the device list changes while an update holds it, keeping the mode and listing in order and dropping repeats. The view renders each with the select's own label function and the switch, so a toggle restyles the line. The update's matching rule for the updated meter is reused. No backend change.

## Testing

Vitest on the store's history with fed listings, and a WebKit test on the mock's update path with a mode change.

## Notes

Source: M7b r4 hardware run, part M. It makes AXHK2M's bootloader label checkable on hardware; the r5 runbook reads the line after the update.

## Hardware evidence (r4 to r6)

- [x] AC-XCCQAK.1 and .2: r6 (20e4bb1, 2026-11-05) parts F3 and F4, the three labels in order, the middle one "(bootloader mode)", and "Hidden" for every serial with the switch on (see PGY704).
- [x] AC-XCCQAK.3, the done ending and Close: the line stayed after the update was done until Close removed it (r6 F3, F4). Failed and stopped endings and a fresh start are covered by Vitest only.
