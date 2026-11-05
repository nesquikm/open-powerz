---
title: A charger unplugged with no contract is noticed
milestone: M_7PKVA6
status: active
archived_at: null
id: fr_01M6K0WPZHSM78REDKCWTFA5N7
created_at: 2026-11-04T10:15:21.330Z
changelog_category: Fixed
kind: bug
---

# A charger unplugged with no contract is noticed

## Summary

The trigger session notices a lost charger only when VBUS collapses under a contract.
Right after Detect there is no contract, and this charger keeps VBUS at 0 V.
So an unplug at that point changes nothing the app watches.
In the r4 hardware run that replug went unnoticed.
The session now also watches the CC lines.
A charger pulls one of them up; with none attached both read near zero.

## Requirement

### Steps to reproduce
1. Open the Trigger tab with a USB PD charger on the meter's input and press Detect.
2. Before pressing any offer, unplug the charger, wait a second, and plug it back in.

### Expected
The view says the charger went away and asks for Detect again; Read cable is disabled with its reason.

### Actual
The view kept the old detection and said nothing.

## Acceptance Criteria

- AC-TFA5N7.1: While a trigger session is open and Detect is not running, both CC lines reading below 0.3 V on records spanning at least 300 ms of device time mark the charger gone, with or without a contract. It is the same state a VBUS collapse sets: the offers are forgotten, requests are refused, "don't ask again" ends, the view says "The charger went away. Press Detect." and Read cable is disabled with "No charger to ask: press Detect to read the cable."
- AC-TFA5N7.2: One CC line at or above 0.3 V ends the spell, a single low record never counts, and records read while Detect runs are ignored. A stream that stops ends the spell as the VBUS watch does.
- AC-TFA5N7.3: app.log gets one line per such event, "trigger cc gone:" with the low CC1 and CC2 readings, and no serial.
- AC-TFA5N7.4: Regression: Rust tests feed readings for an unplug with no contract, an unplug under a contract, a single low record, a low spell during Detect, and a charger with one CC line pulled up, and check the gone state and the log line. A simulator test covers the page's message.

## Technical Design

### Root cause
The session judges a lost charger by VBUS alone, and only once a contract has raised it.

### Fix
The VBUS reading grows CC1 and CC2, and the session runs a second low-spell check on them beside the VBUS one, sharing the record de-duplication and the stopped-stream rule. Its threshold is a named constant tagged [inferred]: a charger's pull-up reads about 3.2 V alone and about 1.5 V against the meter's sink, and nothing attached reads near zero. The gone path is the existing one.

## Testing

Unit tests in the trigger module with a fed reader, and a simulator scenario that drops both CC lines.

## Notes

Source: M7b r4 hardware run, part P4. The 0 V reading with no charger is inferred; the r5 runbook checks it. Related: AXHK2N AC.9 (VBUS collapse) and AXHK2P AC.10.

## Hardware evidence (r4 to r6)

- [x] AC-TFA5N7.1: r6 (20e4bb1, 2026-11-05) part T2, as in WFSNZD AC.1: the gone state on the page with no contract, Read cable disabled with its reason. r5 (cf64002, 2026-11-04) T2 had the backend side (the log line) but not the view.
- [x] AC-TFA5N7.3: one `trigger cc gone:` line per unplug with four CC1/CC2 readings and no serial, in r5 T2 and r6 T2.
- AC-TFA5N7.2 stays unticked: its edge cases cannot be provoked on demand. What the runs show: no false gone during any Detect with the charger attached (r5 and r6 T1, r6 D2), and CC1 reads 3.17 V once the charger is back (r5 T3). Its evidence is automated, in `apps/desktop/src-tauri/src/trigger.rs`: `a_single_low_cc_record_never_counts`, `one_cc_line_at_0_3_v_ends_the_spell`, `a_charger_with_one_cc_line_pulled_up_is_never_gone`, `a_low_cc_spell_spanning_detect_is_ignored` and `a_stream_that_stops_ends_the_cc_spell`.
