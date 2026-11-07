---
title: The notice self-dismiss test does not replay the demo stream
milestone: M_PVFHM6
status: archived
archived_at: 2026-11-07T11:00:24Z
id: fr_01M6TT92CR361TK04MDF16FRNA
created_at: 2026-11-07T10:53:58.200Z
changelog_category: Fixed
kind: bug
---

# The notice self-dismiss test does not replay the demo stream

## Summary

A desktop UI test timed out twice on the macOS CI runner.
It moved the fake clock 37 s while the demo meter streamed.
Each simulated second replayed about 40 data batches and their chart frames.
In WebKit that took about 30 s of the 60 s budget.
The test now jumps the clock, so the notice timer fires without the stream.
Hovered notices still stay, and notices still go 15 s after the pointer leaves.

## Requirement

### Steps to reproduce
1. Run the UI suite's notice self-dismiss test in WebKit on a slow macOS runner.

### Expected
The test passes on every runner, and still fails if a hovered notice leaves or a notice outlives its 15 s.

### Actual
PR 14 run 42362547900, e2e-ui macOS shard 2 of 4, both attempts (jobs 128373830574 and 128379166585): the last check timed out waiting for the notice to go, with nothing received.

## Acceptance Criteria

- AC-16FRNA.1: Regression: the hover and leave steps move the clock on without replaying the demo stream, and the test fails if together they take 3 s or more of real time; they took about 30 s in WebKit before the fix and take a few milliseconds after it.
- AC-16FRNA.2: The notice rule stays as strict: the notice is still shown after 20 s hovered and 14 s after the pointer leaves, and is gone 16 s after it leaves.
- AC-16FRNA.3: A broken hold still fails: with the pointer hold turned off in the app, the test fails.
- AC-16FRNA.4: The app's code does not change.

## Technical Design

### Root cause
The test ran Playwright's clock runFor for 1, 20, 14 and 2 s while the demo streamed. runFor fires every timer due in that span, so the mock's 25 ms batch interval and the chart's animation frames ran about 1,500 and 2,300 times. Timed locally: about 30 s in WebKit and 20 s in Chromium, against a 60 s test timeout.

### Fix
The 20, 14 and 2 s steps use clock fastForward, which fires each due timer at most once, the notice's 15 s timeout included. The 1 s recording step keeps runFor so rows are written.

## Testing

The notice test in Chromium and WebKit, the mutation check with the hold turned off, then polish and reload specs in both browsers.

## Notes

Source: PR 14 CI, macOS WebKit. The reload backfill flake in the same run uses no fake clock, so it is a separate cause. Test-only, no CHANGELOG line. Archived on the shipped M_PVFHM6.
