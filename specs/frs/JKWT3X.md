---
title: Real-meter checks for meter writes
milestone: M_CD7M3M
status: active
archived_at: null
id: fr_01M61PKCQNB3VEMC0G4YJKWT3X
created_at: 2026-10-28T16:48:24.821Z
changelog_category: Added
---

# Real-meter checks for meter writes

## Summary

Deleting recordings and changing settings are checked on the real meter before the milestone ships.
Each check runs from a written runbook by a fresh test agent.
The user approves each write in that agent's tab before it is sent.
Results are compared with the meter's own screen.

## Requirement

Record delete and settings writes are checked on the real KM003C from runbooks, each write approved by the user at the time, with results compared against the meter's screen, before the milestone ships.

## Acceptance Criteria

- AC-JKWT3X.1: Delete: with two short recordings made on the meter for the test, deleting one leaves the other intact in the list, its download unchanged, and the meter's own record screen agrees; the throwaway recordings are deleted one at a time. Delete-all is checked on the meter only if the user explicitly agrees at the time, after a verified full backup; otherwise it is covered by simulator tests and the stage still passes.
- AC-JKWT3X.2: Settings: brightness and one other visible setting are changed, read back, confirmed in the meter's own Settings menu, and then restored to their original values, which are read back too. The meter's live screen updates only after the user opens or changes that setting in the meter's menu [device], so that is not a failure.
- AC-JKWT3X.3: The user approves each write in the test agent's tab before it is sent, and the approval is recorded in the results.
- AC-JKWT3X.4: Each stage runs from a runbook written by the implementer and is carried out by a fresh test agent that only runs the runbook's commands.
- AC-JKWT3X.5: No command outside the runbook is sent; no calibration, firmware, logo or memory write.

## Technical Design

One runbook per stage: preconditions, exact commands, expected output, abort criteria, and how to recover (the meter's own Settings menu can restore display settings; a recording lost by mistake is restored from the backup CSV only on the PC side).

## Testing

Hardware only; results are recorded rather than automated.

## Notes

The recordings made for the test are throwaway; the user's existing recordings are backed up first. Amended 2026-10-28 before the spec commit (spec author's OK): the meter holds the user's real recordings, so hardware delete-all is optional and needs the user's explicit agreement at the time.

Delete stage, 2026-10-29 (`qa-runs/JKWT3X.delete-*`, outside the repo):
- Round 1 (`ffda724`): deleted the last row correctly, but the CLI took the meter's FINISHED reply as an error and exited 5. Fixed in `cd63907`.
- Round 2 (`cd63907`): stopped before any write, because the runbook compared the list's charge and energy with the meter's screen within 1 %, and the screen does not always show the stored total (docs/protocol.md, Offline records, open question). Later rounds check the screen for names, points and durations only.
- Round 3 (`cd63907`): AC-JKWT3X.1, .3, .4 and .5 PASS. A middle row, then the last throwaway, deleted one at a time, each answered FINISHED and exit 0; each write and each list after it held exactly the expected entries byte for byte; every surviving CSV was byte-equal to the backup; the user's recording is intact. Each delete was approved by the user before it was sent. Delete-all was not run on the meter.

Settings stage, 2026-10-29 (`qa-runs/JKWT3X.settings-*`, outside the repo):
- Round 2 (`cd63907`): the write went over bulk and the meter ignored it (no reply, nothing changed); stopped at the read-back. Fixed in `4e726fb` (the write goes over the CDC port).
- Round 3 (`4e726fb`): the meter answered FINISHED and stored brightness and theme, but also moved the sample rate and the bits read as rotation, because keys 0x14 and 0x15 were swapped. The user set them back by hand. Fixed in `a13ac4d`.
- Round 4 (`a13ac4d`): AC-JKWT3X.2, .3, .4 and .5 PASS. Both writes were answered FINISHED; after write 1 only brightness (50 → 20) and theme (blue → green) changed, and after the restore the settings read back identical to the start, in text and JSON. Calibration was untouched. Each write was approved by the user before it was sent. The meter's Settings menu showed the new values; its live screen showed them only after the menu item was opened, or for brightness once, changed. AC-JKWT3X.2 was amended for this (spec author's decision, 2026-10-29). The user's words: "1. brighness was 20, but I saw chenges only after I changed it in menu (to 21). 2. theme was green, and I saw that it changed when I entered "theme" submenu. So, it's changes but not affecting until I enter corresponding menu or even change value (brignnes)"; after the restore: "yes, it back to 50 and blue, but effect was only I enter the corresponging menu"; and about round 3: "Previous time was exactly the same".
