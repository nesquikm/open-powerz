---
title: Custom PD messages and the raw console
milestone: M_X4VEQ3
status: archived
archived_at: 2026-10-24T06:07:16Z
id: fr_01M5KJYD899REQQGB2GZDGSQ6T
created_at: 2026-10-23T05:15:09.449Z
changelog_category: Added
---

# Custom PD messages and the raw console

## Summary

Advanced users can send their own PD messages and type raw commands to the meter.
A message builder sends PD control and data messages, including cable queries that read a cable's e-marker.
A raw console sends any text command and shows the replies.
Both still run every command through the risk check, and GoodCRC is never sent.
Anything that would raise the voltage above 5 V needs the same confirmation as the trigger panel.

## Requirement

The command line tool and the desktop app send custom PD messages and raw PDM text commands, show the replies, and apply the risk check and the high-voltage confirmation to everything they send.

## Acceptance Criteria

- AC-DGSQ6T.1: A message builder composes PD control messages and data messages with data objects for SOP, SOP' and SOP'', and sends them with the device's pd cmd and pd data commands.
- AC-DGSQ6T.2: A Discover Identity request to the cable (SOP') is offered as a preset, and its reply is decoded into the cable's e-marker details with the PD decoder.
- AC-DGSQ6T.3: GoodCRC (pd cmd=1) is refused in the builder and in the console, with a reason.
- AC-DGSQ6T.4: The raw console sends any text line, shows the transmitted and received lines with times, and parses the binary blocks in replies.
- AC-DGSQ6T.5: Any custom message or console command classified above 5 V, including a Request for a higher voltage and any unparsable command, asks for the same confirmation as the trigger panel before it is sent.
- AC-DGSQ6T.6: The command line tool offers pd send and console commands with the same checks and the allow-high-voltage flag.
- AC-DGSQ6T.7: A Discover Identity to an e-marked cable sends the request on the real meter; the result is recorded in the Notes.

## Technical Design

The builder renders the device's hex message form from typed fields and reuses the PD decoder for replies. The console sends raw lines through the PDM session, so pacing, block parsing and the risk classifier apply unchanged.

## Testing

Unit tests for message rendering and the GoodCRC refusal, risk-classifier tests for raw lines, simulator tests for cable identity replies, and Playwright tests for the console and builder.

## Notes

Hardware 2026-10-23: the meter's SOP' Discover Identity reached the cable, which answered GoodCRC only; the charger's own query got VID 0x050D, PID 0x8254, passive, 50 V 5 A, EPR. The cable seems to answer only the VCONN source. AC.7 reworded by orchestrator decision. Details in docs/protocol.md.

## Implementation notes

- Received `pd data=<hex>` reply format for cable ACK is [inferred] from the doc receive example; parser in message.rs + sim must be adjusted if AC.7 hardware shows otherwise. — recorded during review or hardware runs; not gate-blocking
- Desktop send_message enters PD before a confirmed above-5V message when nothing is entered (so close can return to 5 V). — recorded during review or hardware runs; not gate-blocking
- CLI console prints times in UTC (no tz crate); desktop shows local time. — recorded during review or hardware runs; not gate-blocking
- A confirmed unknown console line does not force a return to 5 V at close (only PD/legacy above-5V commands do); pdm close still ends the session. Review in Stage B. — recorded during review or hardware runs; not gate-blocking
- SOP'/SOP'' risk relaxation (AtMost5V) NOT applied: no test asks for it; the classifier stays fail-closed (VDM to cable needs confirmation / --allow-high-voltage). Candidate follow-up. — recorded during review or hardware runs; not gate-blocking
- AC.5/.6 tests were already GREEN after AC.1-4 implementers (scope overreach in forks); no separate AC.5/.6 forks dispatched. — recorded during review or hardware runs; not gate-blocking
- AC.4 implementer damaged and rebuilt mockBackend.ts and powerz-cli app.rs; app.rs diff reviewed by orchestrator, full suites green. — recorded during review or hardware runs; not gate-blocking
- Console listen window after `entry pd` ends before the `pdo:` block arrives (meter, 2026-10-23); consider a longer window for entry lines. M7 candidate. — recorded during review or hardware runs; not gate-blocking
- hil_cable_identity relaxed on the meter: the 'no answer from the cable' report is a recorded result (AC.7 reword). Other audit advisories: SOP' VDM rated Above5V (friction), no CLI test for pd send with flag + return to 5 V, GoodCRC check duplicated TS/Rust, [doc] tag drift. — recorded during review or hardware runs; not gate-blocking
- Stage B round 1 fixes applied (14). mockBackend.ts not yet mirroring fixes 7-9/12 (entry pd before built messages, cable match by VDM, swaps Above5V, reserved control types) — demo-only divergence, M7 candidate. CLI console Ctrl-C test is a unit test (sim Ctrl-C knob only hits the GET_DATA path).
