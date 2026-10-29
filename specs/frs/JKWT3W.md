---
title: Change meter settings
milestone: M_CD7M3M
status: active
archived_at: null
id: fr_01M61PKCQNB3VEMC0G4YJKWT3W
created_at: 2026-10-28T16:48:24.821Z
changelog_category: Added
---

# Change meter settings

## Summary

The meter's display settings can now be read; this feature also changes them.
Brightness, screen dimming, rotation, auto-rotate, language, theme and sample rate are covered.
Calibration is never written.
Every change is read back to confirm it, and the first real write needs the user's approval.

## Requirement

The library, CLI and desktop app change the meter's display and rate settings with the settings write message, write only the supported keys, never calibration, and confirm each change by reading the settings block back.

## Acceptance Criteria

- AC-JKWT3W.1: The key numbers for the settings write (type 0x0A) are recovered from the vendor PDM settings dialog with REA and written to the protocol docs with evidence; any key not recovered is left out of the feature.
- AC-JKWT3W.2: The write sends one type-0x0A message in the documented layout: one 8-byte entry (key word, value) for every writable key, changed ones at their new values and the rest at the values just read, then the 0x7FFF save marker. A function with byte-vector tests builds it.
- AC-JKWT3W.3: Only the recovered display and rate keys are accepted, each with its valid range; calibration, OTP and any unknown key are refused by the guard before anything is sent, with tests.
- AC-JKWT3W.4: After the write the settings block is read again; the changed fields must hold the new values and every other field must be unchanged, else an error names the difference. Nothing is retried on its own.
- AC-JKWT3W.5: `powerz settings set <field>=<value> … --i-understand` writes, and the desktop Meter settings panel gets Edit controls with an Apply button and a summary of the change before it is sent.
- AC-JKWT3W.6: The simulator accepts the write and updates its settings block, so CLI, desktop and e2e tests run without a meter.

## Technical Design

Key map and message builder in the protocol crate beside the settings decoder; the guard gains a narrow allow rule for type 0x0A with the allowed keys only. The desktop panel reuses the read-only panel with edit mode.

## Testing

Unit tests for the builder and the guard, simulator and CLI tests, a Playwright test for the edit flow. The real-meter run is in the hardware FR.

## Notes

User decision 2026-10-28: build settings writes in M6b, never calibration; first real write only with the user's OK.

Real meter, 2026-10-29: the first write went over the vendor bulk interface and the KM003C ignored it (no reply, settings and screen unchanged). The vendor tool sends this message only on the CDC serial port, with no CONNECT or sign-in before it [static]. So the write (and the SETTINGS reads around it) now goes over the meter's CDC port in the CLI and the desktop app, the library refuses it over bulk before sending, and the simulator's bulk persona ignores it as the meter did. Replies on CDC are matched by request id through the ADCNEW/text demux.
