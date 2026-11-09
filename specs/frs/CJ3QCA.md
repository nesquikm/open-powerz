---
title: A settings change in the browser reports a lost reply although it landed
milestone: M_ZFZQE7
status: active
archived_at: null
id: fr_01M70N4CYAK860D6Y2A1CJ3QCA
created_at: 2026-11-09T17:19:12.000Z
changelog_category: Fixed
kind: bug
---

# A settings change in the browser reports a lost reply although it landed

## Summary

Once in a while a settings change in the browser failed with a lost-reply error.
The meter had taken the change, as a fresh read showed.
Old replies left on the reused serial port were read as the answer to the new request.
Now old replies are skipped, and the read-back after the write decides whether the change landed.

## Requirement

### Steps to reproduce
1. In the web edition, apply a settings change, then another one on the same granted port.

### Expected
Each change ends with its read-back result.

### Actual
About one in four changes showed "No reply with the request's id after 3 re-reads." while the write had landed.

## Acceptance Criteria

- AC-CJ3QCA.1: Regression: settings replies with an earlier request's id, left on the port, are skipped and the change completes from the replies with the right ids.
- AC-CJ3QCA.2: When the write itself gets no reply with its id, the read-back decides: a read-back that shows the change reports success.
- AC-CJ3QCA.3: A read-back that does not show the change still fails the change, as before.

## Technical Design

### Root cause
A frame left from an earlier exchange can still arrive after the pipe drains, and only three mismatched replies were allowed, then the change failed even on the write.

### Fix
The settings pipe skips earlier settings replies until the deadline instead of counting them; a write without its own reply is left to the read-back, as a timed-out one already was.

## Testing

Vitest over a fake serial port that replays stale replies.

## Notes

Found by the web parity HIL (2026-11-09, step P5).
