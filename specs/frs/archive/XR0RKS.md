---
title: Scripted unplugs keep streaming, so a late look cannot lose the charger
milestone: M_7PKVA6
status: archived
archived_at: 2026-11-05T11:30:00Z
id: fr_01M6NHHABNEFQEA1R61PXR0RKS
created_at: 2026-11-05T11:00:00.000Z
changelog_category: Fixed
kind: bug
---

# Scripted unplugs keep streaming, so a late look cannot lose the charger

## Summary

One trigger test failed on a loaded macOS runner while the app behaved as designed.
The watcher drops a low spell when more than 400 ms pass between two of its looks.
The test's scripted stream gave one record per look and then stopped repeating its last one.
So one late look before the spell was complete left too few records to finish it.
A real meter keeps streaming after an unplug, so the scripts now do too.

## Requirement

### Steps to reproduce
1. Run the desktop crate's library tests with the e2e feature on a loaded runner.

### Expected
Every trigger test passes on every runner.

### Actual
PR 12 CI on ccd7fd1, macOS gate: the CC unplug-under-a-contract test found no CC gone line within 5 s; the log stopped after the arming request.

## Acceptance Criteria

- AC-XR0RKS.1: Regression: the CC unplug-under-a-contract test passes when one look before its spell completes comes 450 ms late, as it does without the delay.
- AC-XR0RKS.2: Every test that expects a CC gone line or a VBUS collapse uses a scripted stream that goes on with its last values once its records run out; tests of a stream that stops keep the repeated record.
- AC-XR0RKS.3: The collapse-readings test checks that the logged readings are a run of at least four of the looks below 2 V. With no late look it still checks the unplug's first three lows and no dip line; with a late look, any dip line holds only those lows.
- AC-XR0RKS.4: The trigger tests pass twenty times in a row under 24 busy loops, and with a 450 ms delay put into both scripts.

## Technical Design

### Root cause
`check_vbus` ends both spells when the time since the last new record passes the stream period plus 300 ms. The scripts hand out one record per look, so a delayed look resets the spell, and a finite script then stops before the 300 ms of device time a decision needs.

### Fix
`CcScript::stream` and `Script::stream` keep producing records of the last values, 100 ms apart in device time. No app code changes.

## Testing

The trigger tests with the e2e feature in a loop of twenty under 24 busy loops; once with a 450 ms delay injected into each script; the full gate.

## Notes

Source: PR 12 CI on ccd7fd1, macOS gate. Same class as PASCKV, which this sweep extends. Test-only, so no CHANGELOG line. Archived on the shipped M_7PKVA6 by orchestrator decision.
