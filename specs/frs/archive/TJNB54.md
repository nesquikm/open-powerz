---
title: The workspace docs build fails on a private link in the desktop crate
milestone: M_PVFHM6
status: archived
archived_at: 2026-11-07T07:23:18Z
id: fr_01M6SD1PDBCG107MV4ZHTJNB54
created_at: 2026-11-06T21:43:14.091Z
changelog_category: Fixed
kind: bug
---

# The workspace docs build fails on a private link in the desktop crate

## Summary

The API docs for the whole workspace no longer build with warnings treated as errors.
The docs of the desktop backend's settings write link to a private helper.
Public docs may not link to private items.
CI documents only the protocol library, so CI stayed green while the workspace docs build failed.
The settings write docs now name the helper as plain text, and the docs build for every crate.

## Requirement

### Steps to reproduce
1. On branch feat/m7c-logo at 8ea9bee, run `bun run build:web`, then `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked`.

### Expected
The docs build for every workspace crate with no warnings.

### Actual
rustdoc reports that the public documentation for `write_settings` links to the private item `Self::restart_after_settings_write` (`apps/desktop/src-tauri/src/worker.rs` line 1391, added in d52c275), and documenting `open-powerz-desktop` fails.

## Acceptance Criteria

- AC-TJNB54.1: After `bun run build:web`, `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked` succeeds.
- AC-TJNB54.2: Regression: `RUSTDOCFLAGS="-D warnings" cargo doc -p powerz-protocol --no-deps --locked` (CI's exact command) succeeds.
- AC-TJNB54.3: The full gating chain in CLAUDE.md passes.

## Technical Design

### Root cause
The doc comment of the public `write_settings` uses an intra-doc link to the private method `restart_after_settings_write`; rustdoc's `private_intra_doc_links` lint fires, and `-D warnings` turns it into an error.

### Fix
Replace the link with a plain code span. Any other workspace doc warnings found by the same command are fixed in this FR.

## Testing

The doc build is the regression test: both doc commands run by hand alongside the gate. No behaviour change, so no new unit tests.

## Notes

Found by the M7c orchestrator, 2026-11-07. Docs only.
