---
title: The UI suite runs in shards and fits CI's clock
milestone: M_7PKVA6
status: archived
archived_at: 2026-11-05T10:30:00Z
id: fr_01M6NE1EV3M0PGPKJF8H9QTPNR
created_at: 2026-11-05T10:07:00.000Z
changelog_category: Fixed
kind: bug
---

# The UI suite runs in shards and fits CI's clock

## Summary

The UI suite grew from about 650 to about 1080 tests and no longer fits its 30 minute CI job.
Every OS was cut off before the end, so failures past that point went unseen.
The suite now runs in two shards per OS, each well under the limit.
The frame-rate and memory tests run once per OS in their own one-worker step.
When a job is cancelled the test report is still uploaded.

## Requirement

### Steps to reproduce
1. Push a commit to PR 12 and watch the e2e-ui jobs.

### Expected
Each e2e-ui job finishes, with headroom, and its report is kept when it fails or is cancelled.

### Actual
All three e2e-ui jobs were cancelled at 30 minutes on 017c826, and the report upload was skipped because a cancelled job is not a failed one.

## Acceptance Criteria

- AC-9QTPNR.1: The e2e-ui job runs the suite as two Playwright shards per OS (six jobs), each with a 30 minute limit.
- AC-9QTPNR.2: The frame-rate tests run once per OS, outside the shards, in a step with one worker; the WebKit memory test stays once per OS (not on Windows), in the first shard's job.
- AC-9QTPNR.3: The Playwright report and test results are uploaded when the job fails or is cancelled, named by OS and shard.
- AC-9QTPNR.4: A workflow test checks the shard matrix, the single run of the frame-rate and memory steps, and the upload condition.
- AC-9QTPNR.5: On PR 12 each e2e-ui job finishes in about 20 minutes or less.

## Technical Design

A `shard: [1, 2]` matrix axis and `--shard=${{ matrix.shard }}/2`. Frame-rate specs carry a `@perf` tag; the shards run `--grep-invert @perf`, and shard 1 also runs `--grep @perf --workers=1` and the memory step. Upload condition: `failure() || cancelled()`.

## Testing

The workflow Vitest; the CI run on the pushed head.

## Notes

Source: PR 12 CI on 017c826. CI-only, so no CHANGELOG line. Archived on the shipped M_7PKVA6 by orchestrator decision.
