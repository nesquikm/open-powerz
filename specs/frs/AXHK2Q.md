---
title: Hide serial numbers
milestone: M_7PKVA6
status: active
archived_at: null
id: fr_01M69QVAS7BY12FMV0Y1AXHK2Q
created_at: 2026-10-31T19:48:09.149Z
changelog_category: Added
---

# Hide serial numbers

## Summary

Screenshots and screen shares can reveal a meter's serial number.
A new option hides serial numbers everywhere in the desktop app.
Two meters stay easy to tell apart, because tiles then use a position label.
The command line tool gains a matching flag.
The option stays off by default and survives a restart.

## Requirement

A persistent option hides every serial number and UID in the desktop UI, and a CLI flag does the same for its output.

## Acceptance Criteria

- AC-AXHK2Q.1: A "Hide serial numbers" switch persists across restarts and is off by default.
- AC-AXHK2Q.2: With it on, no serial or UID value appears anywhere in the page text, attributes or labels: tiles, picker, top bar, meter details, About, firmware view and its session prompt, and settings. A Playwright test scans the DOM for the demo serial.
- AC-AXHK2Q.3: Hidden fields read "Hidden", and turning the switch off restores them with no reconnect.
- AC-AXHK2Q.4: With it on, meter tiles are titled by model and position ("KM003C 2"), so two meters stay distinct, including two with no serial.
- AC-AXHK2Q.5: Chart images, recording file names and default export names carry no serial, with the option on or off.
- AC-AXHK2Q.6: `powerz list` and `powerz info` accept `--hide-serial`, in text and JSON, printing `hidden` for serial and UID. Other CLI output does not change.
- AC-AXHK2Q.7: A README line and the docs describe the option.

## Technical Design

One `displaySerial(serial, hidden)` helper in the UI, used at every site that reads a serial, with a lint-style test that fails on direct reads in components. The switch lives in the `settings.ts` pattern. The CLI masks in its render step.

## Testing

Vitest for the helper and titles, Playwright for the DOM scan, CLI tests for both formats.

## Notes

The source is the M7 list. The inventory has no vendor equivalent, so this is our own design.
