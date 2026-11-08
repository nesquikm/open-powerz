---
title: Logo write tests stall after resuming the fake clock
milestone: M_QFTCWW
status: archived
archived_at: 2026-11-08T21:34:13Z
id: fr_01M6XP1TRDY8RSZMTCW0FFQVBQ
created_at: 2026-11-08T13:37:33.453Z
changelog_category: Fixed
kind: bug
---

# Logo write tests stall after resuming the fake clock

## Summary

Two logo write tests timed out on the macOS runner three runs in a row.
They hold the fake clock mid-write, then let it run in real time.
The resumed clock replays two demo meters' data batches and chart frames.
On a slow runner each catch-up outgrows the last, so the write never ends.
The tests now step the held clock in short slices instead.
Every check stays the same.

## Requirement

### Steps to reproduce
1. Run the UI suite on the macos-latest runner (PR #15 runs 42477535385, 42480023059 and 42482627155, e2e-ui shards 1 and 2).

### Expected
The logo write tests pass on every runner, and still fail if the write does not end verified or a control is not held.

### Actual
`logo.spec.ts:405` and `logo-hold.spec.ts:192` time out after 30 s on both attempts. The trace shows the view frozen at "Read-back 1 of 6", with rendered frames after `clock.resume()` spaced 0.09, 0.2, 0.42, 0.51, 0.93, 2.0, 4.5 and 8.6 s apart.

## Acceptance Criteria

- AC-FFQVBQ.1: Every logo write test in `apps/desktop/e2e/ui/logo.spec.ts` and `apps/desktop/e2e/ui/logo-hold.spec.ts` that holds the clock mid-write advances the held clock in bounded steps until the write's end shows, instead of resuming it in real time before that wait; each test keeps its existing assertions.
- AC-FFQVBQ.2: A shared helper in `apps/desktop/e2e/ui/helpers.ts` does the stepping, stops as soon as the given condition holds, and fails with a clear message when it never does within its page-time limit.
- AC-FFQVBQ.3: Regression: `apps/desktop/e2e/ui/clockHold.test.ts › logo write tests` fails when either spec waits for a write's end right after `page.clock.resume()`; it fails before the fix and passes after it.

## Technical Design

### Root cause
After `holdClock`, `page.clock.resume()` makes the fake clock catch up with real time on each tick, firing every due timer: the mock's 25 ms stream batches for two meters and their chart frames. When one page second costs more than one real second, each tick covers a longer span than the last and the logo's 50 ms read-back timers starve.

### Fix
A helper steps the held clock with `clock.runFor` in 100 ms slices, checking the condition between slices, then resumes. The 30 s waits keep their assertions.

## Testing

The two logo specs in Chromium and WebKit, then a Vitest source check of both specs.

## Notes

Same class as 16FRNA (fake clock plus streaming replay). Test-only, so no CHANGELOG line.
