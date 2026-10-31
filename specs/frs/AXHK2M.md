---
title: Firmware, trigger and release review leftovers
milestone: M_7PKVA6
status: active
archived_at: null
id: fr_01M69QVAS7BY12FMV0Y1AXHK2M
created_at: 2026-10-31T19:48:09.149Z
changelog_category: Fixed
kind: bug
---

# Firmware, trigger and release review leftovers

## Summary

The M7a reviews left small faults in the firmware, trigger and release code.
A cancel can wait up to two minutes, and a reload loses the replug prompt.
Trigger poll failures stay silent, and some hints lose text or show raw marks.
A release with the wrong tag stays public.
This fix closes those faults and removes dead code around them.

## Requirement

### Steps to reproduce
1. Cancel an update while the engine waits for the meter to return.
2. Reload the page during a replug prompt.
3. Let a poll fail with a trigger session open.

### Expected
The cancel ends promptly, the prompt returns, and the failure is shown.

### Actual
The cancel waits up to 120 s, the prompt is lost, and the failure is silent.

## Acceptance Criteria

- AC-AXHK2M.1: Regression: `crates/powerz-protocol/src/firmware/` tests › `a_stop_during_wait_for_ends_within_a_second` reproduces the failure; it fails before the fix and passes after it. The stop still applies only before the erase.
- AC-AXHK2M.2: A page reload during an update shows a pending replug prompt and, once finished, the true outcome.
- AC-AXHK2M.3: Attach and `devices_changed` report correctly during an update for meters other than the target.
- AC-AXHK2M.4: `FirmwareWarning::UpdateRequired`, the dead `HeadReply::update_required()` branches and test-only firmware constants are removed. `ERASE_BEGAN` and `CANCEL_TOO_LATE` have one source. `cancelFlash` is a required API member.
- AC-AXHK2M.5: Pre-release versions compare below their release, `Display` keeps the tag, `2.0.7-` is refused, and numeric fields refuse a leading `+`. The policy doc is updated.
- AC-AXHK2M.6: A poll failure while a PDM session is open shows on the trigger view, and every view shows an error's hint. Trigger hints show no raw backticks. `console_refusal` treats Unicode blanks as blank.
- AC-AXHK2M.7: Firmware notices have tests, the `followedEnded` note clears on the next action, and its status region is mounted before its text.
- AC-AXHK2M.8: A tag mismatch after publishing turns the release back to draft, and the release version compare handles `-rc` suffixes.

## Technical Design

### Root cause
Each fault is a separate gap the M7a reviews noted; the common cause is scope.

`wait_for` polls the stop flag between short sleeps. The flash status gains the last prompt and outcome. Hints render through one helper. The publish step gains a draft rollback.

## Testing

Unit tests per item in the owning crate or module, and a Vitest test per UI state.

## Notes

Source: M7a Implementation notes marked for M7b. The plan lists items deferred with reasons.
