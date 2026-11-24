---
title: Workspace API docs do not build because of links to private items
milestone: M_8M9B7H
status: archived
archived_at: 2026-11-24T08:06:20Z
id: fr_01M86889K9NK29RC9CQH8VDNQ8
created_at: 2026-11-24T07:45:44.000Z
changelog_category: Fixed
kind: bug
---

# Workspace API docs do not build because of links to private items

## Summary

Building the API docs of the whole workspace with warnings as errors fails.
Two public doc comments link to private items, one in the CLI and one in the browser binding.
CI documents only the protocol library, so it never noticed.
The links become plain code names, and CI documents the whole workspace.

## Requirement

### Steps to reproduce
1. `bun run build:web`, then `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked`.

### Expected
The docs build.

### Actual
`watch_cdc` links to the private `MAX_START_ECHOES`, and `push` links to the private `db_select_rows_sql`; rustdoc stops with an error.

## Acceptance Criteria

- AC-8VDNQ8.1: Regression: after `bun run build:web`, `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked` succeeds, and no lint is allowed to get there.
- AC-8VDNQ8.2: The CI "Rust docs" step documents the whole workspace with warnings as errors.

## Technical Design

### Root cause
TJNB54 fixed the same fault once, but CI kept documenting only `powerz-protocol`, so new private links passed.

### Fix
Use code spans instead of links. Change the CI step to `--workspace`; its dependencies are already built there by the earlier steps.

## Testing

Run the command. A test reads `ci.yml` and checks the docs step covers the workspace.

## Notes

Same fix as TJNB54: a code span, no lint allow.

## Implementation notes

- The CLAUDE.md API docs command still documents only powerz-protocol; left to the user.
