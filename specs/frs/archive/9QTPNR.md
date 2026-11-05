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
The suite now runs in four shards per OS, each well under the limit.
The frame-rate and memory tests run once per OS in their own job, with one worker.
Windows runs only Chromium, the engine its app ships with.
When a job is cancelled the test report is still uploaded.

## Requirement

### Steps to reproduce
1. Push a commit to PR 12 and watch the e2e-ui jobs.

### Expected
Each e2e-ui job finishes, with headroom, and its report is kept when it fails or is cancelled.

### Actual
All three e2e-ui jobs were cancelled at 30 minutes on 017c826, and the report upload was skipped because a cancelled job is not a failed one. On 2d3c4ca run 2 an Ubuntu shard spent 24 of its 30 minutes downloading browsers and was cancelled with its tests passing.

## Acceptance Criteria

- AC-9QTPNR.1: The e2e-ui job runs the suite as four Playwright shards per OS (twelve jobs), each with a 30 minute limit.
- AC-9QTPNR.2: The frame-rate tests run once per OS, outside the shards, in a job of their own with one worker; the WebKit memory test stays once per OS (not on Windows), in that job.
- AC-9QTPNR.3: The Playwright report and test results are uploaded when the job fails or is cancelled, named by OS and shard.
- AC-9QTPNR.4: A workflow test checks the shard matrix, the single run of the frame-rate and memory steps, and the upload condition.
- AC-9QTPNR.5: On PR 12 each e2e-ui job finishes in about 20 minutes or less.
- AC-9QTPNR.6: Each OS runs the Playwright projects that stand in for the engine the app ships there: macOS and Linux run Chromium and WebKit, and Windows, whose app renders with WebView2, runs Chromium only, in the shards and in the frame-rate job. The workflow and the testing spec say why.
- AC-9QTPNR.7: Both UI jobs cache Playwright's browsers keyed on the OS and the exact Playwright version in the lock file; a miss downloads them in a step with its own 8 minute limit and one retry, and a Linux hit still installs the system libraries, with the same limit and retry. Steps that run after a failure, such as the memory test, run only once the browsers are ready. A workflow test checks it.

## Technical Design

A `shard: [1, 2, 3, 4]` matrix axis and `--shard=${{ matrix.shard }}/4`; a macOS runner has one worker, so two shards would not fit. Frame-rate specs carry a `@perf` tag; the shards run `--grep-invert @perf`, and an `e2e-ui-perf` job per OS runs `--grep @perf --workers=1` and the memory step. On Windows both pass `--project=chromium`. Upload condition: `failure() || cancelled()`. Browsers: `actions/cache` on the three ms-playwright folders, a bounded install plus one retry on a miss, `install-deps` on a Linux hit.

## Testing

The workflow Vitest; the CI run on the pushed head.

## Notes

Source: PR 12 CI on 017c826. CI-only, so no CHANGELOG line. Archived on the shipped M_7PKVA6 by orchestrator decision.
