---
title: PDM text session over the virtual serial port
milestone: M_X4VEQ3
status: active
archived_at: null
id: fr_01M5KJYD88N3M6FC2TJFPR41FT
created_at: 2026-10-23T05:15:09.449Z
changelog_category: Added
---

# PDM text session over the virtual serial port

## Summary

The meter does protocol triggering through short text commands on its virtual serial port.
This feature opens that text session and sends commands at a safe pace.
It reads the replies, including the binary capability blocks some replies carry.
Every command is classified by risk before it is sent.
Higher layers can then ask for a confirmation before any request above 5 V.
The session always closes cleanly, so the meter never stays in a triggering mode.

## Requirement

The protocol library opens, drives and closes the meter's PDM text session over the virtual serial port, parses every reply form, and classifies each outgoing command by risk.

## Acceptance Criteria

- AC-PR41FT.1: The library finds the meter's virtual serial port, opens the PDM session (pdm open, then pdm mode entry, sending reset first when the meter answers busy) and closes it with pdm close before the port is released, on normal exit, on error and on drop.
- AC-PR41FT.2: Commands are written one per write and at least 60 ms apart. Replies are read as newline-terminated lines, with prefix matching, and a reply timeout produces a typed error.
- AC-PR41FT.3: Reply lines that carry binary blocks (pdo, ufcs, scp, vfcp and tfcp) are parsed into typed capability records, including blocks that arrive without being asked for. Malformed blocks become typed errors and no input makes the parser panic (property test).
- AC-PR41FT.4: A pure function classifies every outgoing command as read-only, 5 V or less, or above 5 V, from the command and the last known capabilities. Unknown or unparsable commands are classified as above 5 V.
- AC-PR41FT.5: The session API refuses to send a command classified above 5 V unless the caller passes an explicit high-voltage confirmation for that single command.
- AC-PR41FT.6: The simulator answers the PDM session with a scripted charger, including pdo blocks for an EPR charger and timeouts for protocols it lacks, and is used by the tests.
- AC-PR41FT.7: Reopening the port re-opens the PDM session before any other command, as the device requires.

## Technical Design

A pdm module in the protocol crate owns a serial transport, a paced writer and a line reader that splits text lines from length-prefixed binary blocks. A command enum renders to text, and a risk classifier maps commands plus capabilities to a risk level. The session guard sends pdm close on drop. The simulator implements the same text protocol.

## Testing

Unit and property tests for the reply parser and the risk classifier, golden tests from the session-2 replies, and simulator integration tests for open, busy, close on error and reopen.

## Notes

Session-2 device facts: newline-terminated replies, unprompted pdo blocks after entry pd, and pdm open needed again after reopening the port. Triggering is tested with no load on the output.
