---
title: No test keeps the docs review's fixes true
milestone: M_SPYPSC
status: active
archived_at: null
id: fr_01M731VQ6REMG9HCGYFTSPYPSD
created_at: 2026-11-10T15:40:16.932Z
changelog_category: Fixed
kind: bug
---

# No test keeps the docs review's fixes true

## Summary

The docs review fixed the README and the pages under docs, but four of its promises have no test.
A later edit could bring back a broken link, an old screenshot, a wrong transport rule or a stale desktop-only claim, and nothing would fail.
Now tests pin each of the four, and the link check runs in every gate.
Any doc these tests catch is fixed.

## Requirement

### Steps to reproduce
1. Look for the tests that cover the docs review's first four acceptance criteria.

### Expected
Each one has a test that fails when the docs drift.

### Actual
None of the four has a committed test.

## Acceptance Criteria

- AC-SPYPSD.1: Regression: a test checks that the README's desktop screenshot file exists, is a real PNG recording capture and not the old demo image, and that its alt text names the Apple 140 W charger and the 5, 9, 15 and 20 V steps.
- AC-SPYPSD.2: A test checks every relative link and every anchor in the README and in every page under docs: the target file exists and the anchor matches a heading under GitHub's slug rules. It runs in the gate on every OS.
- AC-SPYPSD.3: A test checks the CLI reference against the CLI's own rules: HID refuses the serial-port commands, the CDC source runs with any pipe but HID, and the reference says both.
- AC-SPYPSD.4: A test checks that no page calls the web edition's recording, export, settings, records, logo, triggering, PD capture or PDF export desktop-only, and that only firmware update, the recorder's folder and showing a saved file stay desktop-only.
- AC-SPYPSD.5: Every doc these tests catch is fixed, and the whole gate passes.

## Technical Design

### Root cause
The review was checked by hand; its acceptance criteria did not get tests of their own.

### Fix
New Vitest files beside the other README tests for the screenshot, the links and the desktop-only claims. The link checker slugs headings the way GitHub does and honours explicit anchors. A cargo test in the CLI crate runs the argument validation over each pipe and reads the reference's transport row. Doc fixes as found.

## Testing

Vitest runs the new docs tests; cargo test runs the CLI transport-rule test.

## Notes

Follows the docs review shipped in v0.12.1.
