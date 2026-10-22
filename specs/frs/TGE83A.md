---
title: PD capture core and the powerz pd command
milestone: M_NE3C7K
status: active
archived_at: null
id: fr_01M5HVWJ41WV7P8G50RPTGE83A
created_at: 2026-10-22T13:12:57.217Z
changelog_category: Added
---

# PD capture core and the powerz pd command

## Summary

The meter can sniff USB Power Delivery traffic on the cable that passes through it.
This feature starts and stops that sniffer, polls the meter for captured packets and splits them into records.
Each record is a PD message, a long PD message or an event such as attach, detach or a reset.
The command line tool gets a pd command that streams the records live.
Everything here is passive: the meter keeps passing power and nothing is triggered.

## Requirement

The protocol library and the command line tool capture PD traffic from the meter, parse the packet stream into typed records, and stop the sniffer cleanly on every exit path.

## Acceptance Criteria

- AC-TGE83A.1: The protocol library starts the PD sniffer (control type 0x10, mode 1) and stops it (control type 0x11). It authenticates to level 1 first when needed, and a REJECT from the meter becomes a typed error.
- AC-TGE83A.2: While the sniffer runs, the library polls the PD packet attribute (0x10) about every 5 ms, combined with the ADC attribute when the caller asks for it, and parses each chunk's 12-byte status block: timestamp in ms, VBUS in mV, IBUS as signed mA, CC1 and CC2 in mV.
- AC-TGE83A.3: The record parser splits the rest of each chunk into PD message records (b0 bit 7), big message records (b0 bit 5, extra length at offset 6) and event records, using the lengths in the protocol notes. A record that runs past the chunk is skipped and counted, and no input makes the parser panic (property test).
- AC-TGE83A.4: Event records decode to attach and detach with the CC line, bad message ID, hard reset and cable reset with the sent-by-the-meter flag, receive errors with their cause, monitor type and big message. Unknown codes are kept with their raw byte.
- AC-TGE83A.5: Record timestamps are milliseconds since the capture start and never go backwards when the meter clock restarts.
- AC-TGE83A.6: The command line tool's pd command streams records as text lines, or as JSON Lines with a json flag, until Ctrl-C, and sends the stop command on Ctrl-C, on error and on normal exit.
- AC-TGE83A.7: The session-2 vectors in the protocol notes decode to their annotated values in tests.
- AC-TGE83A.8: The simulator offers a PD capture scenario with a full negotiation, an EPR entry and a reset, used by the tests and by the demo device.
- AC-TGE83A.9: On the real meter, a pass-through capture with a charger on the input and a phone or laptop on the output records Source_Capabilities, Request, Accept and PS_RDY. The check is passive and recorded in the Notes.

## Technical Design

A sniffer session type in the protocol crate owns start, poll and stop, and stops on drop. A pure parser turns a chunk into a status sample plus records; message bodies stay raw bytes here and are decoded by the PD decoder FR. The CLI subcommand drives the session and prints records. Like the authenticated queue stream, it runs over the bulk pipe only: HID replies would span several reports, and sniffing with 0x10 started over CDC is unverified. The simulator answers 0x10, 0x11 and attribute 0x10 polls from a scripted capture.

## Testing

Unit and property tests for the chunk and record parser, golden tests from the session-2 vectors, simulator integration tests for start, poll and stop including stop on error, and a CLI test against the simulator. The real-meter check is manual.

## Notes

Passive sniffing only, per the device safety rules. The UFCS analyzer (attribute 0x40) is left for a later milestone.
