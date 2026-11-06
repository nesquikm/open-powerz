---
title: The settings-write hint says when the meter's screen really changes
milestone: M_PVFHM6
status: active
archived_at: null
id: fr_01M6RMDMJEZ927P9DCDGJV54SP
created_at: 2026-11-06T14:32:51.022Z
changelog_category: Fixed
kind: bug
---

# The settings-write hint says when the meter's screen really changes

## Summary

After a settings change, the app and the command line said the screen shows it once the setting is opened.
On the meter, a new brightness showed only after its value was changed in the menu.
One step up and back again was enough.
Now the hint says that in plain words, and the docs record it as seen on the device.

## Requirement

### Steps to reproduce
1. Change the brightness from the desktop app's settings editor, or with the command line settings command.
2. Open the brightness item in the meter's menu without changing it.

### Expected
The hint tells the user what makes the screen show the new value.

### Actual
Device run, 2026-11-06: brightness 50 to 80 and back to 50. Opening the item did nothing; the screen changed only after the value was moved one step and back (80 to 81 to 80). The hint said opening the setting was enough.

## Acceptance Criteria

- AC-JV54SP.1: Regression: after a successful settings write, the desktop settings panel says the meter has saved the change and that its screen shows it after that value is changed in the meter's menu, for example one step and back.
- AC-JV54SP.2: The command line says the same after a successful settings write.
- AC-JV54SP.3: The protocol doc records the 2026-11-06 observation, tagged as seen on the device, and the user docs and backend reference no longer say that opening the setting is enough.

## Technical Design

### Root cause
The hint was written from the 2026-10-29 run, where a new theme showed when its menu item was opened; brightness behaves differently.

### Fix
One new sentence in the desktop panel's note and the command line note, the docs updated to match, and the tests that pin the old text moved to the new one.

## Testing

Vitest and Playwright pin the desktop note; the command line settings test pins the stderr note.

## Notes

Found in the M7c carry-over device run, part S.
