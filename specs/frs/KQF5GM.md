---
title: CI green on all 8 jobs
milestone: M_NE3C7K
status: active
archived_at: null
id: fr_01M5HAGR722RHPWBP6Z6KQF5GM
created_at: 2026-10-22T08:09:24.450Z
changelog_category: Fixed
---

# CI green on all 8 jobs

## Summary

CI has been red on three of its eight jobs since the real-meter fixes landed.
So the M2 pull request cannot be marked ready.
The failures come from timing and tooling on shared runners, not from a broken feature.
Real stalls get fixed and races are made deterministic.
A slower budget applies only where a diagnosis shows the runner is the cause.
Every strict budget still holds on a developer machine.

## Requirement

The CI workflow passes all eight jobs on one commit, twice in a row, without weakening what the tests prove locally.

## Acceptance Criteria

- AC-KQF5GM.1: When the slow-sink test at 1000 SPS fails, its message reports the poll-thread evidence: the longest gaps between queue polls, the largest amount by which a sleep overran its request, and the longest time the worker spent between waking and its next sleep.
- AC-KQF5GM.2: The cause of the macOS gate failure is decided from that evidence in a CI run and recorded in the Notes. A stall inside the worker loop is fixed in the worker. A CI-only tolerance is allowed only when the evidence shows the runner oversleeping while the loop's own work stays short, and the tolerance is derived from the measured oversleep.
- AC-KQF5GM.3: Without CI set, the slow-sink test still requires zero gaps, zero holes and every delivered record over 10 s at 1000 SPS.
- AC-KQF5GM.4: The WebKit memory test finds this page's WebKit content process on Linux and on macOS. It reads the full process list, not one cut to a terminal width, and knows each platform's page-process name, including the WPE one headless Linux WebKit uses. When no page process can be found, the test skips with that reason instead of failing. It stays in the suite and in its CI step.
- AC-KQF5GM.5: The pan, overview-drag, wheel-zoom and shuttle render-step tests keep the 100 ms budget without CI set, and use 300 ms (the budget times a CI factor of 3) when CI is set. Each failure message names the budget it used.
- AC-KQF5GM.6: The test that a clear empties the overview strip before the next sample controls the page clock, so no sample can arrive between the clear and its checks. It passes in Chromium and WebKit regardless of runner speed.
- AC-KQF5GM.7: CI is green on all eight jobs for one commit, on its first run and on a re-run.
- AC-KQF5GM.8: The overview-strip drag, click and keyboard-slider tests bound how much the view's span may grow by the real time that passed between reading the view and the pan, not by a fixed 500 ms. They pass in Chromium and WebKit regardless of runner speed.

## Technical Design

Slow-sink test: a recording clock wraps the system clock and logs each sleep request against the elapsed time. Each sleep's overrun and the work time between sleeps go into the failure message. Pushing that to CI tells oversleep apart from a loop stall.
Memory test: read the full process list, match each platform's page-process names, and skip with a reason when none appears.
Render-step tests: one shared helper returns 100 ms, multiplied by 3 when CI is set.
Clear test: install the Playwright clock before the page opens and pause it around the clear, as the polish test already does.

## Testing

Each change is its own regression test: the CI run that fails today must pass. Locally, cargo runs the slow-sink test with CI unset. Playwright runs one project at a time with one worker: chromium, then webkit for the clear test and the memory test (with the memory flag set). The render-step tests run under both CI settings to show which budget each one picks.

## Notes

Failing runs: 39651629627, 39672064832, 39742831114. Evidence run 39747964247 (macOS gate, 10 s): 101 sleeps instead of about 500, max oversleep 265 ms, max loop work 6 ms, and every delivered record reached the sink. The runner oversleeps; the worker does not stall. The 0.2.0 CHANGELOG is released, so this fix ships next.
