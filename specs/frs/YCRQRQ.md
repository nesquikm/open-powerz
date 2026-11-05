---
title: Command line logo commands
milestone: M_PVFHM6
status: active
archived_at: null
id: fr_01M6P7JAKHTQS4VT76ZXYCRQRQ
created_at: 2026-11-05T16:11:37.285Z
changelog_category: Added
---

# Command line logo commands

## Summary

The command line tool gets four logo commands: preview, read, write and remove.
Preview works without a meter and shows exactly what would be written.
Write and remove ask for typed confirmation, show progress, and print the result with the startup-screen duration.
Read reports what the meter holds and can save it as an image.

## Requirement

powerz gains a logo subcommand group that wraps the session logo actions with confirmation, progress and plain output.

## Acceptance Criteria

- AC-YCRQRQ.1: powerz logo preview IMG [--alpha] [-o out.png] prints final size, mode, blob size, padded size and chunk count, warns when a side is below 48, and with -o writes the decoded preview PNG; it opens no device.
- AC-YCRQRQ.2: powerz logo read prints empty, the logo's size and mode, or unrecognised; with -o it reads the whole logo and writes it as PNG; it sends no write.
- AC-YCRQRQ.3: powerz logo write IMG [--alpha] needs --i-understand and a typed confirmation at the prompt (--yes skips only the prompt), prints per-chunk and read-back progress on stderr, and ends with the verified result and the startup-screen duration, adding a hint when the startup screen is off.
- AC-YCRQRQ.4: powerz logo remove follows the same confirmation rules and prints the verified result.
- AC-YCRQRQ.5: Ctrl-C during an upload's chunk phase does not stop the write; the tool says it will finish first, then exits.
- AC-YCRQRQ.6: Every error names the failed step and chunk index, and the exit code is non-zero. Output never contains the meter serial or UID.
- AC-YCRQRQ.7: CLI tests run each command against the simulator, including a failure at chunk k and a refused non-KM003C model; the README and the CLI docs describe the commands.

## Technical Design

New `commands/logo.rs` with clap subcommands; image loading through the protocol crate's `image-files` feature; confirmation reuses the records-delete prompt helper; progress through `output.rs`.

## Testing

Integration tests with the simulator backend used by the existing CLI tests; snapshot of preview output for a fixed image.

## Notes

The CLI is never run against the real meter in stage 0.
