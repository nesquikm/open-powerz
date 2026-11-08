---
title: Fold export test races the live demo stream
milestone: M_QFTCWW
status: active
archived_at: null
id: fr_01M6XP1TS7XPMGJGNRQCB67EYP
created_at: 2026-11-08T13:37:33.479Z
changelog_category: Fixed
kind: bug
---

# Fold export test races the live demo stream

## Summary

A unit test of folded exports failed once on the Windows runner.
It reads the fold boundary, then exports, while the demo meter keeps streaming.
Samples that arrive in between move the boundary, so the two numbers differ.
The test now keeps the stream from advancing between the two reads.
Its checks stay exact.

## Requirement

### Steps to reproduce
1. Run `bun run test` on windows-latest (PR #15, CI run 42482627155, gate job).

### Expected
`exportFold.test.ts` passes on every runner, and still fails if the export reports a different fold than the history holds.

### Actual
`demo backend export of a folded session (AC-AXHK2W.14) › reports the folded samples, their span and the file's start past its limit` failed with `expected 15025 to be 15000`.

## Acceptance Criteria

- AC-B67EYP.1: The fold export test reads the boundary and runs both exports with no demo samples arriving in between, and keeps its exact equality checks on samples and span.
- AC-B67EYP.2: Regression: `apps/desktop/src/lib/exportFold.test.ts › reports the folded samples, their span and the file's start past its limit` fails when demo stream timers fire between the boundary read and an export; it fails before the fix and passes after it.

## Technical Design

### Root cause
The test connects the demo meter at 1000 samples per second and awaits between `historyFoldBoundary()` and each export. On a slow runner the stream's real timers fire in between, so the history grows and the export's fold is computed over more samples.

### Fix
Use Vitest fake timers around the seeded part of the test so no stream timer fires between the reads.

## Testing

The fold export test, plus a check that a stream tick between the reads breaks the old shape.

## Notes

The same race exists on main; the Windows runner exposed it. Test-only, so no CHANGELOG line.
