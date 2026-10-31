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

- AC-AXHK2N.1: The above-5 V dialog shows a "Don't ask again until I go back to 5 V or disconnect" checkbox only for a fixed, PPS or AVS request of known voltage at or below 20 V. It starts unchecked every time.
- AC-AXHK2N.2: After a confirmed request with the box checked, the backend is armed. Later fixed, PPS and AVS requests at or below 20 V send with no dialog.
- AC-AXHK2N.3: A request above 20 V, of unknown voltage, or from the raw console or built messages always shows its confirmation, armed or not, and the backend refuses it unconfirmed.
- AC-AXHK2N.4: Back to 5 V, hard reset, disconnect, device loss, trigger session close and quit each disarm the backend, with a test for each. A page reload does not arm it, and an armed backend reports its state to the reloaded page.
- AC-AXHK2N.5: While armed, the panel shows a visible "Not asking up to 20 V" status with a Clear button that disarms at once.
- AC-AXHK2N.6: The arming request is refused if the request does not qualify; the backend never trusts a UI flag for the 20 V limit.
- AC-AXHK2N.7: `powerz` trigger commands and `--allow-high-voltage` behave exactly as before. The mock backend follows the same rules. A README line and the trigger how-to describe the option.

## Technical Design

`TriggerEngine` gains `armed: bool`. `request(req, confirm)` takes `Confirm::{No, Yes, YesRemember}`. The classifier gains `Armed` for a qualifying request while armed, which `risk` returns so the UI skips the dialog. Every disarm path clears it in one `disarm()`. Voltage comes from the classifier's own lookup against the capabilities, never from the UI.

## Testing

Rust unit tests per disarm path and limit, Vitest for the panel, Playwright for dialog and Clear. The tester runs one 9 V then 20 V sequence on the wall charger from a runbook.

## Notes

User decision 2026-10-26. Triggering at or below 20 V is approved in the wall-charger setup only.
