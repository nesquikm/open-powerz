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

## Technical Design

`CableView` gains `speed` and `vendor`. A `cable_read` command wraps the existing `send_message` path with the preset and `await_cable_identity`. The classifier's narrow exception reuses `is_cable_discover_identity` plus an exact-byte match. The passive path reads the latest matching record from the capture store.

## Testing

Rust unit tests with the simulator cable, including no-answer; Vitest and Playwright for the card. The tester reads a real e-marked cable from a runbook.

## Notes

The classifier exception is deliberately narrow: it covers one message to the cable, which cannot raise VBUS. The first real send runs through the tester's runbook, with nothing on the meter's output and the user's OK.
