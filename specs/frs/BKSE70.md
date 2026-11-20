---
title: Updating a meter on the old bootloader waits two minutes, then times out
milestone: M_E84WQE
status: active
archived_at: null
id: fr_01M7W21FGHK10CK3PGT5BKSE70
created_at: 2026-11-20T08:46:36.074Z
changelog_category: Fixed
kind: bug
---

# Updating a meter on the old bootloader waits two minutes, then times out

## Summary

The app sent the jump, and the meter went into its old bootloader.
The update kept waiting for a bootloader it knows, then gave up after two minutes.
Now the wait stops as soon as the old bootloader appears.
Flash and jump refuse such a meter at once and send it nothing.
The message says this bootloader is not supported yet.
It says unplugging returns the meter to normal, and where to read more.

## Requirement

### Steps to reproduce
1. On a KM003C with bootloader 2.1 in application mode, start a firmware update (desktop or `powerz firmware flash`).

### Expected
As soon as the meter shows up in its old bootloader, the update stops with an actionable message.

### Actual
The meter sits in its bootloader (flashing green bar) while the app waits the full 120 s and fails with a timeout.

## Acceptance Criteria

- AC-BKSE70.1: Regression: in a flash, when the meter re-enumerates as the old bootloader after JUMP_DFU, the switch wait ends at the first poll that sees it, well before the 120 s switch timeout on the simulated clock, with a dedicated old-bootloader error, and no further frame is sent.
- AC-BKSE70.2: A flash started on a meter already in the old bootloader is refused before any frame is sent.
- AC-BKSE70.3: A jump on a meter in the old bootloader is refused before any frame is sent.
- AC-BKSE70.4: The old-bootloader error says this bootloader generation is not supported yet, that unplugging and replugging the meter returns it to the application firmware, and gives the full GitHub URL of the firmware how-to's old-bootloader section; the CLI prints it and exits with the device error code 5.
- AC-BKSE70.5: The desktop firmware update ends with that message when the engine reports it, with no 120 s wait, and an update cannot start on a meter listed in the old bootloader (the message shows instead).
- AC-BKSE70.6: The firmware how-to has an old-bootloader section (the URL's anchor): what the user sees, how to get back to normal, that the official Windows software is the way to update for now, and which read-only commands to send with a report.

## Technical Design

### Root cause
`wait_for` polls for `Bootloader` only and treats any other mode as "not yet"; `flash`/`jump` call the 2.1 shape `Unknown`.

### Fix
`FlashError::LegacyBootloader` (code -110) with the message and URL as one constant. `wait_for` returns early when an open sees `LegacyBootloader` while waiting for the bootloader; `flash` and `jump` refuse it before opening a guarded write. The desktop maps the error to its failure state; `firmwareUpdate.svelte.ts` refuses to start on a listed old bootloader. The mock backend gets an old-bootloader update scenario.

## Testing

Engine tests on the sim opener (frame log empty after the legacy re-enumeration, clock far below the timeout); CLI integration for exit code and text; desktop backend test and a Vitest for the update store.

## Notes

Depends on JT38C8. Flashing the 2.1 bootloader itself is out of scope until we have its endpoint dump.
