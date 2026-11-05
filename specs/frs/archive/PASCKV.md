---
title: Two tests stop racing the clock and the Windows file system
milestone: M_7PKVA6
status: archived
archived_at: 2026-11-05T07:40:00Z
id: fr_01M6NA0S7JRP9CFD2R7NPASCKV
created_at: 2026-11-05T07:33:20.755Z
changelog_category: Fixed
kind: bug
---

# Two tests stop racing the clock and the Windows file system

## Summary

The release PR's CI failed on two tests, and the app was not at fault in either.
One trigger test slept a fixed time and assumed the watcher had seen a collapse.
On a loaded runner the watcher was late, so the request went through.
One recording test named its file with a question mark, which Windows forbids.
Both tests now hold on every runner and still check the same things.

## Requirement

### Steps to reproduce
1. Run the desktop crate's tests on a loaded macOS CI runner, and on Windows.

### Expected
Every test passes on every runner.

### Actual
PR 12 CI: on macOS, the collapse-just-before-an-armed-request test saw its request accepted; on Windows, the URI-path database test could not create its file.

## Acceptance Criteria

- AC-PASCKV.1: Regression: the collapse-just-before-an-armed-request test waits until the watcher reports the charger gone, within the 1 s bound of AC-AXHK2N.9, instead of sleeping a fixed 450 ms. The request is still refused and nothing is written.
- AC-PASCKV.2: The CC-gone log line test waits for the line before its fixed wait for a second one, so a late watcher cannot fail it. It still checks for exactly one line holding both low readings.
- AC-PASCKV.3: Regression: the database test with URI characters in its path keeps the question mark on Unix. On Windows it uses a name with percent, hash, space, ampersand and equals, and it still opens the database and checks the file stays alone.
- AC-PASCKV.4: A sweep of the trigger module and the desktop integration tests finds no other fixed sleep followed by a check that something already happened, and no other file name with a character Windows forbids.

## Technical Design

### Root cause
`a_collapse_just_before_an_armed_request_refuses_it` slept 450 ms against the 300 ms low spell, and `the_cc_gone_line_has_both_low_readings` slept 300 ms before reading the line; both race the watcher thread. `a_path_with_uri_characters_opens_as_a_database` used `?`, which NTFS rejects.

### Fix
The first test calls `gone_within_a_second`, the second calls `wait_for_line` before its sleep, and the third picks its file name by `cfg(windows)`. No app code changes.

## Testing

The desktop crate's library and e2e-save tests with the e2e feature, as CI runs them, in a loop of ten; the full workspace tests and clippy.

## Notes

Source: PR 12 CI on 4703dc1, macOS and Windows gates. Same class as DS89PS. Written straight into the archive because M_7PKVA6 had already shipped in v0.9.0 (orchestrator decision); test-only, so no CHANGELOG line.
