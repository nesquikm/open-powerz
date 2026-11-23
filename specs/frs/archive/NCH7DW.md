---
title: The experimental old-bootloader flash refuses a meter it just switched, and overstates what we know
milestone: M_XQPRRR
status: archived
archived_at: 2026-11-23T22:36:40Z
id: fr_01M855A4NA5YM6MFXQV0NCH7DW
created_at: 2026-11-24T08:00:00.000Z
changelog_category: Fixed
kind: bug
---

# The experimental old-bootloader flash refuses a meter it just switched, and overstates what we know

## Summary

A review of the experimental old-bootloader flash found a real bug and some overstated text.
A meter that starts in its application and switches to bootloader 2.1 was refused.
The refusal told the user to pass the flag they had already passed.
The warning and docs also stated as fact what is only expected.
Now the opted-in flash goes on after the switch, and the text says exactly what is known.

## Requirement

### Steps to reproduce
1. A KM003C with bootloader 2.1 in application mode; run `powerz firmware flash FILE --i-understand --experimental-old-bootloader`.

### Expected
JUMP_DFU, the meter comes back in its old bootloader, and the experimental download runs on it.

### Actual
The flash stops after the switch with the old-bootloader refusal, which asks for the flag that was passed.

## Acceptance Criteria

- AC-NCH7DW.1: Regression: with the opt-in, a meter that starts in application mode and comes back from JUMP_DFU in the old bootloader is flashed end to end on the old-bootloader path in the simulator, with no refusal.
- AC-NCH7DW.2: Without the opt-in, that switch still ends with nothing sent to the old bootloader, and the message says the meter is now in its old bootloader, that it can be retried with the experimental flag while it stays there, and that a replug returns it to the application.
- AC-NCH7DW.3: The CLI warning, the firmware how-to and the CLI reference say that open-powerz never sends a write below 0x4000, and that bootloader 2.1 keeping that layout and taking a retry after a failed run is expected but not verified; none of them states either as fact.
- AC-NCH7DW.4: The protocol doc tags the late first CONNECT and the failed pipe recovery as inferred from the trace, with the reasoning, and its bootloader 2.1 rows no longer say that CONNECT and HEAD time out or that flashing is unsupported.
- AC-NCH7DW.5: The CLI's experimental warning prints only when the meter is in the old bootloader, at open or after the switch, and before any frame is sent to it; on application or bootloader 2.5 meters that stay there it does not print.
- AC-NCH7DW.6: On the old bootloader, a HEAD exchange that yields only stray ACCEPTs past the cap is a read error (vendor code -108), not a product error.
- AC-NCH7DW.7: The probe JSON gives string descriptors cut at the first NUL, as the text does, and the simulator-mode usage error lists legacy-bootloader-own-ids.

## Technical Design

### Root cause
`switch_to_bootloader` never sees the ack, so `wait_for` ends the switch on `LegacyBootloader` with the plain refusal. The warning prints in `flash_on` before the meter is opened.

### Fix
`wait_for` takes an `old_ok` flag; with it, an old-bootloader meter counts as reached and `flash` continues on the old path. Without it, a new `FlashError::SwitchedToLegacyBootloader` (-110) carries the after-switch text. A CLI opener wrapper prints the warning on the first old-bootloader open. HEAD stray overflow maps to `Read`. JSON strings reuse the NUL cut.

## Testing

Engine and sim tests for both switch arms; CLI tests for the warning gate, JSON strings and usage text; doc tests for the hedged wording and retags; desktop mapping test for the new error.

## Notes

Found by an independent review of PR #22. Supersedes the wording of DJYNRP's third AC.

## Implementation notes

- Two DJYNRP tests were superseded and updated: the opted-in switch now runs on (the old test now covers the no-flag arm), and the warning test checks the new wording.
- The desktop BKSE70 test fixture wrote one shared temp file from parallel tests; it now writes one file per call (a flake the first gate run caught).
- The usage-error line for the sim mode has no defect check (no plant); it is a static string.
- No test checks that the warning prints only once per run.
