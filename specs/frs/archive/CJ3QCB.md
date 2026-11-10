---
title: Refreshing the record list in the browser misses a new record
milestone: M_ZFZQE7
status: archived
archived_at: 2026-11-10T07:04:51Z
id: fr_01M70N4CYAK860D6Y2A1CJ3QCB
created_at: 2026-11-09T17:19:12.000Z
changelog_category: Fixed
kind: bug
---

# Refreshing the record list in the browser misses a new record

## Summary

A record made on the meter after the browser connected did not show on Refresh.
Reconnecting showed it, and so did the CLI.
The list is read in a sign-in that started at connect, and the meter seems to answer from that time.
Now each Refresh signs in again before it lists, as a reconnect does.

## Requirement

### Steps to reproduce
1. Connect a meter in the web edition and open Records.
2. Make a short record on the meter, then press Refresh.

### Expected
The new record is listed.

### Actual
The list still shows only the older records until the meter is connected again.

## Acceptance Criteria

- AC-CJ3QCB.1: Regression: every record list the browser reads starts with a fresh level-1 sign-in, also when the session already holds one.
- AC-CJ3QCB.2: A refused sign-in fails the list with the existing sign-in message and shows no stale rows.
- AC-CJ3QCB.3: Download and delete keep working on the list they read.

## Technical Design

### Root cause
Inferred: the driver signs in once per session and the meter answers the record list from the state at that sign-in; a reconnect or the CLI signs in afresh.

### Fix
The list step always signs in before reading the list; the HIL re-run confirms on the meter.

## Testing

Vitest over the fake meter: the sign-in request goes out before each list.

## Notes

Found by the web parity HIL (2026-11-09, step P7). The meter's behaviour is inferred until the HIL re-run.

## Hardware evidence (parity HIL 3)

- [x] AC-CJ3QCB.1: the parity HIL 3 (74904fd, 2026-11-10, qa-runs/WEB.parity-hil3-20261110-0216, P7) a record made on the meter after connect (A02.d, 12 pts, 0:00:11) showed on Refresh in the same session.
