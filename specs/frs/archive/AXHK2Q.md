---
title: Hide serial numbers
milestone: M_7PKVA6
status: archived
archived_at: 2026-11-05T07:00:44Z
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
- AC-AXHK2Q.6: `powerz list` and `powerz info` accept `--hide-serial`, in text and JSON, printing `hidden` for serial and UID. With several meters and no `--device`, or a `--device` that matches none, `info --hide-serial` repeats no serial in its error. Other CLI output does not change, and `--trace` stays a raw-frame channel, as the docs say.
- AC-AXHK2Q.7: A README line and the docs describe the option.
- AC-AXHK2Q.8: No error, log line or CLI message carries a serial port's name or path, with the option on or off, since on macOS that name contains the meter's serial. Messages say "the meter's serial port". The `in use by another program` error and every error mapped from the serial library are covered, and a test feeds a port name holding a serial through each. With `--hide-serial`, a port name that `powerz list` shows is hidden too.

## Technical Design

One `displaySerial(serial, hidden)` helper in the UI, used at every site that reads a serial, with a lint-style test that fails on direct reads in components. The switch lives in the `settings.ts` pattern. The CLI masks in its render step.

## Testing

Vitest for the helper and titles, Playwright for the DOM scan, CLI tests for both formats.

## Notes

The source is the M7 list. The inventory has no vendor equivalent, so this is our own design.

## Implementation notes

- AC.8 comes from the AXHK2J hardware run on ec17ae3: the held-port error printed a port name holding the meter's serial.
- `list --hide-serial` prints `hidden` for a meter with no serial too, so the flag does not show which meter has none.
- With the switch on, two meters with no serial read the same in the firmware session prompt. AC.4 asks for distinct tiles only, and the prompt lists each hold.
- The library's several-meters flash text counts meters, since it reaches the desktop page and `app.log`. `powerz firmware flash` still names the serials from the error's field, as AC-STQEA1.6 requires and AC.6 keeps.
- The chart-image half of AC.5 has nothing to test until AXHK2V adds image export; AC-AXHK2V.10 carries it.
