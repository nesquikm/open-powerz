---
title: The MSRV job covers the desktop crate
milestone: M_7PKVA6
status: active
archived_at: null
id: fr_01M69QVAS7BY12FMV0Y1AXHK2H
created_at: 2026-10-31T19:48:09.149Z
changelog_category: Fixed
kind: bug
---

# The MSRV job covers the desktop crate

## Summary

The workspace declares Rust 1.96 as its minimum version.
The CI job for that version skips the desktop crate, which inherits the same declaration.
A test comment says the crate needs a newer Rust, and that comment is wrong.
We compiled the crate on Rust 1.96 and it passed.
This fix makes the job check the desktop crate too and removes the wrong comment.

## Requirement

### Steps to reproduce
1. Read the `msrv` job in the CI workflow and the comment in the release workflow test.
2. Compare them with the desktop crate's inherited `rust-version`.

### Expected
Every crate that declares `rust-version` is built with it in CI, and no comment contradicts that.

### Actual
The job builds default members only, and the comment claims the Tauri crate needs a newer Rust.

## Acceptance Criteria

- AC-AXHK2H.1: Regression: `apps/desktop/scripts/release/release-workflow.test.ts` › `the MSRV job also checks the desktop crate` reproduces the failure; it fails before the fix and passes after it.
- AC-AXHK2H.2: The `msrv` job builds the web UI, installs the Linux libraries the Tauri crate links, and runs `cargo check -p open-powerz-desktop --all-targets --locked` on the declared MSRV.
- AC-AXHK2H.3: The job still builds and tests the default members.
- AC-AXHK2H.4: The comment saying the Tauri crate needs a newer Rust is gone, and no comment or doc repeats it.
- AC-AXHK2H.5: The job passes on the PR for this milestone; a failure there moves `rust-version` up instead of narrowing the job.

## Technical Design

### Root cause
AC-4PJRVN.6 scoped the job to default members on an unchecked belief. `cargo +1.96 check -p open-powerz-desktop --all-targets --locked` passed on macOS on 2026-10-31, and the highest dependency `rust-version` is 1.90, so the belief was wrong.

The job gains the steps above, reusing the steps from the gate job.

## Testing

The workflow test pins the job's steps. The real proof is the job's run on the PR.

## Notes

Source: AC-4PJRVN.6 partial. A Linux pass is not proven until the PR's job runs.
