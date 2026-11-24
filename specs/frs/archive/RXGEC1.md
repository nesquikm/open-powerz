---
title: Texts about the old bootloader still say it is not supported yet
milestone: M_8M9B7H
status: archived
archived_at: 2026-11-24T08:06:20Z
id: fr_01M86889K24QQTPZ4Y1TRXGEC1
created_at: 2026-11-24T07:45:44.000Z
changelog_category: Fixed
kind: bug
---

# Texts about the old bootloader still say it is not supported yet

## Summary

Since v0.14.0 the CLI can flash a meter on bootloader 2.1 behind an experimental flag.
Several texts from before that still say the old bootloader is not supported yet.
That contradicts the CLI, and a field user is about to read those texts.
Now each text says the flash is experimental and works only from the CLI.
The desktop app still cannot flash such a meter, and its texts say so.
No behaviour changes.

## Requirement

### Steps to reproduce
1. Run `powerz firmware probe` on a meter in bootloader 2.1, or list it in the desktop app.

### Expected
The text says this bootloader is old, and that its flash is experimental, only in the CLI, behind `--experimental-old-bootloader`.

### Actual
`Mode: bootloader (old, not supported yet)`, and similar texts in the flash refusals, the desktop app and the docs.

## Acceptance Criteria

- AC-RXGEC1.1: Regression: the CLI text mode of the old bootloader (probe and info `Mode:` lines) is exactly `bootloader (old, 2.1-style; experimental CLI-only flash with --experimental-old-bootloader)`; its JSON value stays `legacy_bootloader`.
- AC-RXGEC1.2: The two old-bootloader refusal texts of the flash engine say "supported only experimentally, from the CLI", still say "nothing was sent to it", name the whole command `powerz firmware flash FILE --i-understand --experimental-old-bootloader`, the replug and the how-to link, and the desktop copy of the first one matches the engine text.
- AC-RXGEC1.3: The desktop device list (backend, web UI label and mock backend) labels such a meter `(old bootloader, experimental CLI update)`.
- AC-RXGEC1.4: The desktop connect refusal (backend and mock backend, same text) says the desktop app cannot connect to or update a meter on the old bootloader, names the CLI's whole command `powerz firmware flash FILE --i-understand --experimental-old-bootloader`, calls it experimental and tested only in simulation, and says unplugging and replugging returns to the application firmware.
- AC-RXGEC1.5: The how-to's old-bootloader section, the CLI reference, the desktop-backend reference and the protocol-library API reference quote the new texts, give the whole command and no longer say the old bootloader is "not supported yet"; the hedges stay (tested only in simulation, never sends a write below 0x4000, retry expected but not verified).
- AC-RXGEC1.6: No file under `crates/*/src`, `apps/desktop/src`, `apps/desktop/src-tauri/src` or `docs` says "not supported yet" about the old bootloader; tests that assert the phrase is absent may still contain it.

## Technical Design

### Root cause
JT38C8 and BKSE70 wrote the texts before the experimental flash existed, and DJYNRP only added the flag to them.

### Fix
Change `DeviceMode::label`, the two constants in `flash.rs`, the suffix and refusal in `devices.rs`, and their TypeScript copies in `deviceDetails.ts`, `format.ts` and `mockBackend.ts`. Correct the comment on `FlashError::LegacyBootloader`. Update the docs. Update the tests that pin the old strings to pin the new ones with the same strictness.

## Testing

Update the JT38C8, BKSE70, XW4QQN and firmware mock tests to the new wording. Add a docs test and a source scan for the stale phrase.

## Notes

Supersedes the wording of JT38C8 (AC .4, .5) and BKSE70 (AC .4, .6). The CHANGELOG and archived FRs keep their history. No protocol change and no real meter.

## Implementation notes

- The jump command lines "Was in: … mode" reuse the mode label, which reads long there; jump refuses the old bootloader, so it should not show.
- A review before merge asked for the whole, copyable command and for text that does not overclaim; AC .2 to .5 were amended.
