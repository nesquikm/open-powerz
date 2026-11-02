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
- AC-AXHK2N.8: A request at or below 5 V, a confirmed request that does not qualify, a request that fails after anything was written, Detect, any built message, any console line that is not read-only, and any change in the charger's offers each disarm too, with a test for each. A request above 5 V whose offer differs from the one last listed is refused, not sent.
- AC-AXHK2N.9: Once a PD request or the way back to 5 V was accepted, VBUS that stays below 2 V for 300 ms ends the backend's armed state with no press, within 1 s of first reading below 2 V, and the status goes. One low reading between normal ones does not, as 1.77 V and 0.67 V did on the meter, and neither does a dip to 4.5 V during a voltage change. VBUS of 0 V before any contract means nothing, so Detect does not trip it.
- AC-AXHK2N.10: The same collapse forgets the charger's offers in the backend and on the page. A request is refused until the next Detect, no stale offer can be pressed, and the page says the charger went away and to press Detect. A same-charger replug behaves the same, with nothing sent.
- AC-AXHK2N.11: When the trigger session ends, leaving the tab included, the offers go and the view says to press Detect. No control stays on screen that does nothing when pressed.
- AC-AXHK2N.12: `app.log` records the offers each time they are listed, and the reason armed ended, one line each, with no serial.
- AC-AXHK2N.13: A request in flight shows that it is in progress, and the armed status goes as soon as Detect starts.
- AC-AXHK2N.14: While Detect runs, every control that sends to the meter is disabled: offers, Back to 5 V, Read cable and the console. A press that reached the page during Detect is dropped, never answered with a dialog after Detect ends. Test first.
- AC-AXHK2N.15: Leaving the Trigger tab logs its own armed-end reason, `left the trigger view`, never the reason of a press nobody made.
- AC-AXHK2N.16: `app.log` records every message the trigger session writes to the meter, with its text (`pdm open`, `entry pd`, `pd req=2`, `pd hrst`, `pd data=…`, `reset`, `pdm close` and console lines), and the PD capture's start and stop with the meter's answer. No serial and no port name.
- AC-AXHK2N.17: A collapse logs the VBUS readings that ended it, and one low reading that did not count logs as an ignored dip with its value, so a hardware run can tell a real dip from a bad reading.

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
- The hardware run answered the swap question only for a charger with different offers. ACs .9 and .10 stop relying on what the meter reports.
- The hardware run on ec17ae3 found no silent send, but the armed status stayed through a charger swap until the next press, so a same-charger replug was assumed to send. ACs .9 to .13 close that.
- 2 V: PD holds VBUS at 4.75 V or above across a voltage change, and 0.8 V is its off level. The a5442bc run showed a false end costs more than a dialog: one low reading during Read cable ended the PD session at a steady 5.18 V. So a collapse needs 300 ms of low readings. The check runs every 100 ms on the session thread, from the stream's latest sample.
- This charger reads 0 V until a PD session starts, hence "once a contract was accepted".
- The check arms after a reading of 3 V or more since the contract, so a stale sample from before it cannot trip. PPS starts at 3.3 V.
- After Detect's `reset`, closing still sends `pd req=1` because the session keeps its PD flags. That is older than this FR and fails safe, with a Hard Reset fallback on the real meter. The simulator does not answer it.
- A contract that never reads 3 V counts as a collapse after 1 s, so a charger pulled during the first request is caught. A demo meter reading 0 V trips this too, which is how a test proves the stream wiring.
- A PD listing that gets no answer also clears the collapse, so a replacement charger with no PD is no dead end. With no offers, no PD request can go out.
- ACs .14 to .17 come from the a5442bc hardware run. A 9 V press during Detect opened its dialog by itself 55 s later. Leaving the tab logged `back to 5 V`. `app.log` held no send, so E3, E7 and the cable read could not be counted.
- Whether the low readings were real dips or bad readings is not known. The log lines of .17 are there to settle it.
- A VBUS reading is the stream's latest record with its device time, and a record read twice counts once. A collapse needs two or more low records whose device times span 300 ms, with none at 2 V or above between. A missing reading, no new record for the record period plus 300 ms (so 2 SPS still works), an accepted contract, or the end of the rise wait each start the spell afresh, so a never-rising contract collapses after about 1.3 s.
- `app.log` gets one line per command, at the flush that ends it (commands carry no terminator), with control characters as `\xNN` and at most 256 characters plus ` [cut]`. A failed write logs `trigger send failed: <command>`.
