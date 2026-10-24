---
title: Charger protocol detection
milestone: M_X4VEQ3
status: archived
archived_at: 2026-10-24T06:07:16Z
id: fr_01M5KJYD899REQQGB2GZDGSQ6R
created_at: 2026-10-23T05:15:09.449Z
changelog_category: Added
---

# Charger protocol detection

## Summary

Before triggering, it helps to know what a charger supports.
This feature runs the meter's own detection for BC1.2 and Apple charging and its automatic protocol list.
It also adds a hint table built from the D+ and D- voltages.
Detection that stalls on the device is stopped cleanly after a time limit.
Detection never asks for a voltage above 5 V.

## Requirement

The library, the command line tool and the desktop app report which charging protocols a connected charger supports, using the meter's detection commands and a D+ and D- hint table.

## Acceptance Criteria

- AC-DGSQ6R.1: BC1.2 and Apple detection report supported, not supported or unknown from the device replies, including the reply forms seen on the device in session 2.
- AC-DGSQ6R.2: The automatic list (entry list, and entry list+ on request) reports each protocol line as it arrives with its result. A stall longer than a configurable limit, 45 s by default, ends with reset and a partial result marked incomplete.
- AC-DGSQ6R.3: A hint table maps the measured D+ and D- voltages to likely charging modes (Apple 2.4 A, Apple 2.1 A, Samsung 2 A, DCP and others) and labels the result as a hint.
- AC-DGSQ6R.4: The command line tool has a detect command with text and JSON output.
- AC-DGSQ6R.5: Detection commands are classified as 5 V or less and never trigger a confirmation.
- AC-DGSQ6R.6: On the real meter, detection runs on each of the user's chargers with nothing on the output, and the results are recorded in the Notes.
- AC-DGSQ6R.7: When the automatic list ends incomplete, the session checks that the meter still answers in step, with a read-only command that must get its own reply, before anything else is sent. If it does not, detection says the meter needs a power cycle and the session refuses further commands.
- AC-DGSQ6R.8: When the BC1.2 or Apple check and the automatic list disagree, the result names which check said what, never an unqualified "not supported" next to "supported".

## Technical Design

A detect module on the PDM session handles the per-command reply forms and the stall timer. The hint table is a pure lookup on the measured D+ and D- voltages, with ranges taken from the vendor manual. A list is also cut off with `reset` after 64 entries, and the CLI warns that the meter's own probing may raise VBUS. After a cut list a read-only `pdm parm` must get its own reply, and any line belonging to another command latches the session out of sync, refusing later commands.

## Testing

Simulator tests for every reply form and the stall timeout, unit tests for the hint table boundaries, and CLI tests. The hardware runs are manual.

## Notes

Hardware 2026-10-23: entry bc and entry apple answered fail on all three chargers. Auto-list: Apple 140 W bc1.2 only; Anker QC 2.0, QC 3.0 and PD 45 W; EcoFlow Apple 2.4A, Samsung 2A, UFCS 63 W, QC 2.0, QC3+, AFC, FCP and PD. A full list ends with done.

## Implementation notes

- ListResult.complete is a bool: a 64-entry cut-off, a stall and a stop all read "incomplete" — reason enum later
- any `word:rest` line counts as a list entry (by design, so unknown protocols still show); a stray `error: x` line would too — recorded during review or hardware runs; not gate-blocking
- an I/O error mid-list drops the collected entries (text mode already printed them; --json prints nothing) — recorded during review or hardware runs; not gate-blocking
- Ctrl-C during entry bc / entry apple waits out the 8 s reply window before pdm close — recorded during review or hardware runs; not gate-blocking
