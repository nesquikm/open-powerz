---
title: The README says browser edition where the rest says web edition
milestone: M_NA20AD
status: archived
archived_at: 2026-11-10T14:02:43Z
id: fr_01M72ENP12Z9RFFF589GZ9PC5A
created_at: 2026-11-10T10:04:47.522Z
changelog_category: Fixed
kind: bug
---

# The README says browser edition where the rest says web edition

## Summary

The README's Coming next list says browser edition.
The rest of the README, the docs and the app say web edition.
A README test pins the old words, so the clash cannot be fixed without it.
Now every user-facing page says web edition, and the tests pin that.

## Requirement

### Steps to reproduce
1. Read the Coming next section of the README.
2. Compare it with the Web edition heading and the docs.

### Expected
One name for the same thing everywhere.

### Actual
Coming next says browser edition, and a test requires it.

## Acceptance Criteria

- AC-Z9PC5A.1: Regression: the README's Coming next says web edition, and the README test pins web edition there.
- AC-Z9PC5A.2: No README line and no page under docs says browser edition, and a test checks it.

## Technical Design

### Root cause
Coming next kept its wording from before the web edition got its name.

### Fix
Reword the README line and the code comments that still say browser edition; update the README test and add a check over the README and docs.

## Testing

Vitest runs the README feature tests and the web edition docs tests.

## Notes

Only wording changes.
