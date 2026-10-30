---
title: Trigger panel and console fixes
milestone: M_3KPN7D
status: active
archived_at: null
id: fr_01M65KVDYC49QF1V7N1Y4PJRVH
created_at: 2026-10-30T05:17:20.204Z
changelog_category: Fixed
---

# Trigger panel and console fixes

## Summary

Several small problems were found while triggering real chargers.
The request result card could mix an old request with a new one, and some messages ran together.
The console kept the last command and stopped listening too early.
This fixes them and keeps the demo mode in step with the real app.

## Requirement

The trigger panel, the raw console, the CLI stream stop and the demo backend behave correctly in the cases found during M4 testing.

## Acceptance Criteria

- AC-4PJRVH.1: After Back to 5 V, the request result card shows only the latest request's command and delivered values, never a mix of an old and a new request.
- AC-4PJRVH.2: A failure's message and hint are separated by a space or line break.
- AC-4PJRVH.3: The console clears the command field after a line is sent, and its listen window after `entry` lines is long enough to catch the `pdo:` block that follows.
- AC-4PJRVH.4: The desktop Detection card shows the D+/D- charging-mode hint the CLI already shows.
- AC-4PJRVH.5: A stream error that is not a device loss (decode or protocol) keeps an open PDM session open and does not trigger an automatic bulk reconnect while it is open.
- AC-4PJRVH.6: `powerz stream` tolerates a rejected stop (0x0F) after a queue backlog overflow and exits cleanly with a note.
- AC-4PJRVH.7: GoodCRC detection has one source of truth, or a parity test pins the TypeScript and Rust versions together.
- AC-4PJRVH.8: The mock backend mirrors the shipped trigger rules (PD entry before built messages, cable match by VDM, swaps above 5 V, reserved control types).

## Technical Design

Fixes in the trigger panel store and components, the console component, the CLI stream command and the mock backend; a shared fixture for the GoodCRC parity test.

## Testing

Unit and component tests for each AC; Playwright for the card and console fields; a CLI test with a simulated rejected stop.

## Notes

Source: the M_X4VEQ3 M7 follow-ups. Charger coverage gaps that need hardware are out of scope.
