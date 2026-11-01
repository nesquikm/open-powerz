---
title: "Trigger panel \"don't ask again\""
milestone: M_7PKVA6
status: active
archived_at: null
id: fr_01M69QVAS7BY12FMV0Y1AXHK2N
created_at: 2026-10-31T19:48:09.149Z
changelog_category: Added
---

# Trigger panel "don't ask again"

## Summary

Every request above 5 V opens a confirmation, which slows repeated work on one charger.
A new option in that dialog skips the question for later requests up to 20 V.
It ends when the user goes back to 5 V, hard resets, disconnects or quits.
EPR requests above 20 V and the raw console always ask.
The backend holds this state, and the command line tool does not change.

## Requirement

The trigger panel offers a session-scoped "don't ask again" option for requests up to 20 V. The backend enforces its limits, so no UI state can widen them.

## Acceptance Criteria

- AC-AXHK2N.1: The above-5 V dialog shows a "Don't ask again until I go back to 5 V or disconnect" checkbox only for a USB PD fixed, PPS or SPR AVS offer of known voltage at or below 20 V. It starts unchecked every time.
- AC-AXHK2N.2: After a confirmed request with the box checked, the backend is armed. Later fixed, PPS and AVS requests at or below 20 V send with no dialog.
- AC-AXHK2N.3: A request above 20 V, of unknown voltage, at an EPR position at any voltage, to a battery or variable offer, from a non-PD protocol, from the raw console or from built messages always shows its confirmation, armed or not, and the backend refuses it unconfirmed.
- AC-AXHK2N.4: Back to 5 V, hard reset, disconnect, device loss, trigger session close and quit each disarm the backend, with a test for each. A page reload does not arm it, and an armed backend reports its state to the reloaded page.
- AC-AXHK2N.5: While armed, the panel shows a visible "Not asking up to 20 V" status with a Clear button that disarms at once.
- AC-AXHK2N.6: The arming request is refused if the request does not qualify; the backend never trusts a UI flag for the 20 V limit.
- AC-AXHK2N.7: `powerz` trigger commands and `--allow-high-voltage` behave exactly as before. The mock backend follows the same rules. A README line and the trigger how-to describe the option.
- AC-AXHK2N.8: A request at or below 5 V, a confirmed request that does not qualify, a request that fails after anything was written, Detect, any built message, any console line that is not read-only, and any change in the charger's offers each disarm too, with a test for each. A request whose offer differs from the one last listed is refused, not sent.

## Technical Design

`Session` gains `armed: bool`; closing the session ends it, and `disarm()` covers the rest. `TriggerControl::request` takes `Confirm::{No, Yes, YesRemember}`, with `From<bool>` so callers stay as they are. A pure `pd::rememberable(offers, request)` decides qualification from the offers, never from the UI. `risk` adds `above_5v_rememberable` and `armed`. New commands report and clear the state.

## Testing

Rust unit tests per disarm path and limit, Vitest for the panel, Playwright for dialog and Clear. The tester runs one 9 V then 20 V sequence on the wall charger from a runbook.

## Notes

User decision 2026-10-26. Triggering at or below 20 V is approved in the wall-charger setup only.

## Implementation notes

- Scope calls, all toward asking more: PD offers only, since "fixed, PPS or AVS" names PD offer kinds. QC, FCP and the rest always ask. EPR always asks, even at 18 V on an EPR AVS, per the 2026-10-26 decision.
- A PPS or AVS request is judged at the larger of the request and the offer's minimum, as the risk classifier does.
- A reload never finds an armed session: attaching closes the leftover trigger session, which goes back to 5 V. The panel still reads the state, for the meter on screen.
- While armed, the session reads the port for 20 ms before judging anything, since nothing else reads it between commands. That is how a new charger's offers can arrive in time.
- A "yes" stands on the offer the page last listed, so a dialog naming 9 V never sends 12 V. Detect refreshes it.
- A confirmed request that does not qualify disarms, EPR or legacy, so nothing stays armed across a contract the option never covered.
- Open: whether the meter reports anything when a charger is swapped while the session stays open. The simulator cannot settle it; part E of the runbook does. The how-to tells the user to end the option first.
