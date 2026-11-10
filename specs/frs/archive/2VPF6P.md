---
title: The recorder's chart memory line reads like the session history
milestone: M_NA20AD
status: archived
archived_at: 2026-11-10T14:02:43Z
id: fr_01M72ENP0VEFKVGWKFVT2VPF6P
created_at: 2026-11-10T10:04:47.515Z
changelog_category: Fixed
kind: bug
---

# The recorder's chart memory line reads like the session history

## Summary

The recorder options say how long the chart memory lasts at the current rate.
That number comes from the live chart buffer, about 10 minutes at 1000 samples per second.
It reads like the session history, which keeps far more.
Now the line names the live chart buffer and says the session history keeps far more.
The how-to pages use the same words.

## Requirement

### Steps to reproduce
1. Connect the demo device at 1000 SPS and open the recorder options.
2. Read the line under the row limit.

### Expected
The line says it is the live chart buffer, and that scrolling back reads the longer session history.

### Actual
It says "Chart memory holds about 10 minutes at this rate", which reads like all the history the app keeps.

## Acceptance Criteria

- AC-2VPF6P.1: Regression: the recorder options line names the live chart buffer and its span at the current rate, and says the session history keeps far more; it no longer says "Chart memory".
- AC-2VPF6P.2: While the rate is unknown the line names the live chart buffer and says it is measuring the rate.
- AC-2VPF6P.3: The record-a-csv and explore-a-session pages call it the live chart buffer, in the same words as the app, and the doc pins check that.

## Technical Design

### Root cause
The memory text named only "chart memory", with no word on the history behind it.

### Fix
Reword the memory depth text and the two how-to pages. The live buffer size stays as it is.

## Testing

Vitest covers the new wording and the unknown-rate case; the recorder options doc pin follows the new words.

## Notes

Only wording changes.
