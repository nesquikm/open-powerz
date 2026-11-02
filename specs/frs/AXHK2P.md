---
title: One-click cable e-marker panel
milestone: M_7PKVA6
status: active
archived_at: null
id: fr_01M69QVAS7BY12FMV0Y1AXHK2P
created_at: 2026-10-31T19:48:09.149Z
changelog_category: Added
---

# One-click cable e-marker panel

## Summary

Reading a cable's e-marker needs the PD console today.
A new Cable card in the trigger view reads it with one click.
It shows the vendor, current, voltage, speed and whether the cable is active.
If no answer comes, the card explains why.
The card can also show an identity already seen in a PD capture, which sends nothing.

## Requirement

The trigger view has a Cable card that reads the e-marker with one click during an open PD session, and falls back to the identity seen in a PD capture.

## Acceptance Criteria

- AC-AXHK2P.1: With a PD session open, a "Read cable" button sends one SOP' Discover Identity and shows vendor, VID, PID, maximum current, maximum voltage, USB speed, active or passive, and EPR capability.
- AC-AXHK2P.2: The button works only inside an open PD session. It never opens, enters or closes a session, and it never sends any other message. With no session open the backend refuses it and sends nothing, with a test. One press sends one message, with no retry.
- AC-AXHK2P.3: The backend classes exactly that message as 5 V or lower, by comparing the built bytes with one constant in one backend function, so it needs no dialog. No UI flag can select the exemption. Every other SOP' or SOP'' message keeps today's confirmation.
- AC-AXHK2P.9: A test changes every byte of the built message in turn, covers SOP and SOP'' targets, header bits, the VDM command and an added or removed byte, and shows each variant still needs the confirmation.
- AC-AXHK2P.4: When the wait ends with no answer, the card says the cable may have no e-marker or the meter may not be the VCONN source. Nothing is retried on its own.
- AC-AXHK2P.5: A vendor with no entry in our own table shows as its hex VID. The table is written from the public USB-IF list, not from vendor software.
- AC-AXHK2P.6: With no session open and a PD capture holding a SOP' Discover Identity ACK, the card shows that identity, labelled as seen in the capture, and sends nothing.
- AC-AXHK2P.7: `powerz-pd` decodes the speed and the active or passive fields from the cable VDOs, with byte-vector tests.
- AC-AXHK2P.8: The demo device answers with a sample cable. A README line and the trigger how-to describe the card.
- AC-AXHK2P.10: Read cable is disabled, with the reason shown, while the charger is gone and while Detect runs.
- AC-AXHK2P.11: A PD capture start the meter refuses says so, naming the capture, with no advice to update the firmware. `app.log` records the start and the meter's answer.

## Technical Design

`CableView` gains `speed` and `vendor`. A `cable_read` command builds the message from one constant and waits once with `await_cable_identity`; it does not go through `send_message`, which would enter PD. The exemption is an exact-byte match in one function, so a cable read needs no dialog. The passive path reads the latest matching record from the capture store.

## Testing

Rust unit tests with the simulator cable, including no-answer; Vitest and Playwright for the card. The tester reads a real e-marked cable from a runbook.

## Notes

The classifier exception is deliberately narrow: it covers one message to the cable, which cannot raise VBUS. The first real send runs through the tester's runbook, with nothing on the meter's output and the user's OK.

## Implementation notes

- An open PD session means the session has entered PD. With another protocol entered, or none, the read is refused and sends nothing, since the button never enters anything.
- The exemption lives in one backend function, used by the read and by the built-message risk and send, so the same bytes from the console builder need no dialog either. The raw console keeps asking. The library classifier does not change, so `powerz` keeps asking too.
- A read ends "don't ask again", like any built message.
- The vendor table is ours and small. An unlisted vendor shows as hex, which is the normal case.
- The builder's send enters PD first, as before, so the same bytes from the builder can write `entry pd`. The button never does.
- AC.9's variants found a hole in the library classifier, older than this FR: a body that did not match the header's object count could class as a control message, so a Request's RDO behind a count of zero went out with no dialog. Any such body is now above 5 V, in the CLI too.
- `pdm.entered()` sees parsed `entry` lines only. An unparsed confirmed console line that changed protocol would leave it saying PD. The read still cannot raise VBUS, so this stays.
- A variant that drops the object and clears the count (`01 8F 00`) is a different, complete control message, so it keeps its own class.
- ACs .10 and .11 come from the a5442bc hardware run. B2 never got a fair try: one low VBUS reading ended the PD session at a steady 5.18 V, so the card never came (AC-AXHK2N.9 and .17 now need 300 ms of low readings). AC.1 stays unproven on the meter.
- In that run the meter refused the capture start (0x10) after a trigger session, with the cable under test in its female port and nothing beyond it. M3's captures were read by polling without 0x10. Whether the trigger session's state or this wiring causes the refusal is not known. The next runbook starts a capture on a fresh connect before any trigger, then again after one, and the log of .11 shows the answers.
- AC.11's hint says to replug the meter and try again: the refusal's cause is not known, and a replug clears the trigger session. Only the capture start (0x10) gets the new text; every other refused request keeps its hint. The app.log line `pd capture start refused` holds the start and the answer in one line.
