---
title: The page hears a charger gone with no contract
milestone: M_7PKVA6
status: archived
archived_at: 2026-11-05T07:00:44Z
id: fr_01M6KQ0MKCCDJ6ZYTBJ9WFSNZD
created_at: 2026-11-04T16:41:58.636Z
changelog_category: Fixed
kind: bug
---

# The page hears a charger gone with no contract

## Summary

The backend now notices a charger unplugged right after Detect, before any offer is pressed.
The Trigger view still did not show it in the r5 hardware run.
The page asks about a lost charger only on a VBUS drop under a contract or a refused request.
With no contract it never asks, so the old offers stay on screen.
The page now also asks when both CC lines read low, contract or not.

## Requirement

### Steps to reproduce
1. Open the Trigger tab with a USB PD charger on the meter's input and press Detect.
2. Before pressing any offer, unplug the charger.

### Expected
The view says "The charger went away. Press Detect.", the old offers go, and Read cable is disabled with "No charger to ask: press Detect to read the cable."

### Actual
The app log has the "trigger cc gone:" line, but the view keeps the old offers and says nothing; Read cable keeps "Reading the cable needs PD entered: press a USB PD offer first."

## Acceptance Criteria

- AC-WFSNZD.1: While a trigger session is open and Detect is not running, a live reading with both CC lines below 0.3 V makes the page ask the backend whether the charger is gone, with or without a contract. When the backend says yes, the view shows the gone state of AC-TFA5N7.1: the offers go, "The charger went away. Press Detect." shows, and Read cable is disabled with "No charger to ask: press Detect to read the cable."
- AC-WFSNZD.2: The page asks no more than before otherwise: a VBUS reading below 2 V still asks only under a contract, so a charger that keeps VBUS at 0 V with CC pulled up causes no questions; one question is out at a time; none is asked while Detect runs, with no session, or once the charger is known gone. An answer that comes back after Detect started or the session ended changes nothing.
- AC-WFSNZD.3: The case with a contract (VBUS collapse after an accepted request) works as before.
- AC-WFSNZD.4: Regression: Vitest covers the low-CC question with no contract, the guards of AC-WFSNZD.2, and the late answer. A mock mode drops both CC lines a moment after Detect with no request, and a WebKit test checks the message and the Read cable reason after Detect with no offer pressed; the existing contract test stays green.

## Technical Design

### Root cause
The page's charger-gone question is gated on its own contract flag and is only cued by low VBUS or a refusal, so the backend's no-contract gone state never reaches the view.

### Fix
The trigger store's batch handler cues the question on low VBUS under a contract or on both CC lines below a named 0.3 V constant mirroring the backend's. The question's gate becomes session open, Detect not running and not already gone; a Detect counter taken before the call drops a late answer. The backend still decides. The mock gains a mode whose stream drops CC and sets the gone flag after Detect.

## Testing

Vitest on the trigger store with a fed API, a mock-backend test for the new mode, and a WebKit trigger spec on that mode.

## Notes

Source: M7b r5 hardware run, part T2. Related: TFA5N7 (backend CC watch) and AXHK2N AC.9.

## Hardware evidence (r4 to r6)

- [x] AC-WFSNZD.1: r6 (20e4bb1, 2026-11-05) part T2. After Detect, with no offer pressed, the charger was unplugged. Within 3 s the view said "The charger went away. Press Detect.", every offer card read "Not listed yet.", and Read cable was disabled with "No charger to ask: press Detect to read the cable." app.log had one `trigger cc gone:` line (CC1 and CC2 0.00 V). The same step failed in r5 T2, which is what this FR fixed.
- AC-WFSNZD.2, guard "none once the charger is known gone": in r6 T3 the message stayed after the replug until Detect, and Detect brought the same offers back and cleared it. The other guards are covered by Vitest only.
