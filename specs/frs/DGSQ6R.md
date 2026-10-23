---
title: Charger protocol detection
milestone: M_X4VEQ3
status: active
archived_at: null
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

## Technical Design

A detect module on the PDM session handles the per-command reply forms and the stall timer. The hint table is a pure lookup on the measured D+ and D- voltages, with ranges taken from the vendor manual. A list is also cut off with `reset` after 64 entries, and the CLI warns that the meter's own probing may raise VBUS.

## Testing

Simulator tests for every reply form and the stall timeout, unit tests for the hint table boundaries, and CLI tests. The hardware runs are manual.

## Notes

Session 2: entry bc replied only ok, entry apple replied fail on a non-Apple charger, and entry list stalled after vfcp:null until reset. How a full list ends is unknown, so a list is always reported incomplete until hardware shows its end.
