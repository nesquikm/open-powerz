---
title: The keep-up drop-log test and the real-time sim test hold on a loaded runner
milestone: M_7PKVA6
status: archived
archived_at: 2026-11-05T07:55:00Z
id: fr_01M6NAWKA0TFRFY22QE5SZGY5S
created_at: 2026-11-05T07:48:32.192Z
changelog_category: Fixed
kind: bug
---

# The keep-up drop-log test and the real-time sim test hold on a loaded runner

## Summary

The release PR's Windows CI failed the test that says a page keeping up logs no drop line.
The app was right: its forwarder really did fall the queue limit behind.
The simulator made samples as fast as it could, and the runner starved the forwarder.
A sweep found one more test that bounded a real-time count by a fixed window.
Both tests now hold on a loaded runner and still check the same things.

## Requirement

### Steps to reproduce
1. Run the desktop crate's live drop-log tests on a slow Windows CI runner.

### Expected
A page that keeps up writes no live-batch drop line.

### Actual
PR 12 CI on Windows: the keep-up run logged "live batches dropped … page fell 512 batches behind", although its page never blocked.

## Acceptance Criteria

- AC-SZGY5S.1: Regression: the keep-up drop-log test paces the simulated meter's pipe calls in real time, so the forwarder must be starved for well over half a second to fall the queue limit behind. It still requires no live-batch line in app.log.
- AC-SZGY5S.2: The keep-up run delivers more sample batches than the 512-batch queue limit, and the test asserts it, so it cannot pass without ever nearing the limit.
- AC-SZGY5S.3: The pace is opt-in on the shared two-meter rig and is off by default, so every other test using the rig runs unpaced as before.
- AC-SZGY5S.4: The simulator's real-time production test bounds its record count by the time that really passed, not by a fixed 400 ms window.
- AC-SZGY5S.5: A sweep of the desktop and crate integration tests finds no other undrained blocking sink, and no other fixed sleep before a check that something already happened.

## Technical Design

### Root cause
The forwarder's backlog counts batches queued for its thread, not unread page events, so an undrained page channel does not cause it. The simulator clock advances as fast as the worker polls, so on a starved runner the worker outran the forwarder. Locally the run delivered only 401 batches and never neared the limit.

### Fix
A shared atomic pace on the rig's transport sleeps per pipe call; the keep-up test sets 1 ms and waits for 5,000 samples. The real-time test measures elapsed time and allows 50 SPS over it.

## Testing

The desktop crate's test binaries in a loop under 24 CPU loads, the sim queue tests, then the full gate.

## Notes

Source: PR 12 CI on 11725b6, Windows Rust test job. Same class as DS89PS and PASCKV. Archived on the shipped M_7PKVA6 by orchestrator decision; test-only, so no CHANGELOG line.
