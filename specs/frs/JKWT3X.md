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
- AC-JKWT3X.2: Settings: brightness and one other visible setting are changed, read back, seen on the meter's screen, and then restored to their original values, which are read back too.
- AC-JKWT3X.3: The user approves each write in the test agent's tab before it is sent, and the approval is recorded in the results.
- AC-JKWT3X.4: Each stage runs from a runbook written by the implementer and is carried out by a fresh test agent that only runs the runbook's commands.
- AC-JKWT3X.5: No command outside the runbook is sent; no calibration, firmware, logo or memory write.

## Technical Design

One runbook per stage: preconditions, exact commands, expected output, abort criteria, and how to recover (the meter's own Settings menu can restore display settings; a recording lost by mistake is restored from the backup CSV only on the PC side).

## Testing

Hardware only; results are recorded rather than automated.

## Notes

The recordings made for the test are throwaway; the user's existing recordings are backed up first. Amended 2026-10-28 before the spec commit (spec author's OK): the meter holds the user's real recordings, so hardware delete-all is optional and needs the user's explicit agreement at the time.
