---
title: The fallback-clear worker test holds the worker at a fixed poll
milestone: M_7PKVA6
status: archived
archived_at: 2026-11-05T07:00:44Z
id: fr_01M6KZXX3VZT75RFAJ4GDS89PS
created_at: 2026-11-04T19:18:11.944Z
changelog_category: Fixed
kind: bug
---

# The fallback-clear worker test holds the worker at a fixed poll

## Summary

A worker test about clearing the history sometimes fails on a busy computer.
The test holds the worker in the meter link a moment after it sees a batch.
Events reach the test on a separate thread, so a busy machine moves that moment.
Sometimes the held poll's samples were already sent, and the test's setup check fails.
After this fix the test holds the worker at a fixed poll and waits on signals, not sleeps.

## Requirement

### Steps to reproduce
1. On the 18-core dev Mac, start 24 `yes > /dev/null` processes.
2. Run `cargo test -p open-powerz-desktop --test worker a_fallback_history_clear` 20 times.

### Expected
The test passes every run, whatever the load.

### Actual
3 of 20 runs fail at `tests/worker.rs:835`: "no recorded sample was waiting in the batcher". The gate run of 2026-11-04 hit the same failure once, while other jobs loaded the CPU.

## Acceptance Criteria

- AC-DS89PS.1: Regression: `apps/desktop/src-tauri/tests/worker.rs` › `a_fallback_history_clear_never_sends_pre_clear_samples_after_it` passes 20 of 20 runs while 24 `yes` processes load the 18-core dev Mac; the same run failed 3 of 20 before the fix.
- AC-DS89PS.2: The test transport counts its own ADC-queue polls and holds the worker at a fixed one. It signals when it holds, and the test waits on that signal instead of a fixed sleep. The event sink no longer arms the hold.
- AC-DS89PS.3: The test tells batches sent before the clear from those sent after it by the session statistics each batch carries, not by when they reach the sink. It still checks that samples were waiting in the batcher, and still fails if a sample recorded before the clear reaches the page after it.
- AC-DS89PS.4: Only test code changes, and the full workspace test suite passes.

## Technical Design

### Root cause
The sink armed the hold, but events reach it on the forwarder thread. On simulated time the worker runs ahead meanwhile, so the poll let through lands anywhere in the 50 ms batch cycle. One poll in five meets a flush and leaves nothing pending.

The transport now holds the Nth queue poll, so the simulated timeline before the hold is fixed. Every flush after the release includes the held poll's own samples, so its batch statistics exceed the count read at the hold. Earlier batches never do.

## Testing

The test itself, run 20 times under the load in the steps above, then the full workspace suite once.

## Notes

Found by the M7b gate run. Test-only, so the WebKit suite needs no rerun.
