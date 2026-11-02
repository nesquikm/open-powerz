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
- AC-AXHK2M.9: The outage gap a session sends over an unplug equals the history gap over the same unplug, also when live batches were dropped for a slow sink. A test forces the drop, and `the_reconnect_continues_the_timeline_with_an_outage_gap_in_history_and_export` passes 60 of 60 full-target runs with its timings unchanged.
- AC-AXHK2M.10: A live batch dropped for a slow sink costs no sample in the session history, the export or a recording, and a test pins it. The docs state whether the dropped samples are lost to the live view or only delayed.
- AC-AXHK2M.11: At 375 px wide, with one meter and with several, the page has no horizontal scroll, and the Add meter control is fully visible and usable. A test pins both.
- AC-AXHK2M.12: At 375 px wide with one meter, the device select does not cover Disconnect or any other top bar control. A test pins it.

## Technical Design

### Root cause
Each fault is a separate gap the M7a reviews noted; the common cause is scope.

`wait_for` polls the stop flag between short sleeps. The flash status gains the last prompt and outcome. Hints render through one helper. The publish step gains a draft rollback.

## Testing

Unit tests per item in the owning crate or module, and a Vitest test per UI state.

## Notes

Source: M7a Implementation notes marked for M7b. The plan lists items deferred with reasons.

## Implementation notes

- The `pd cmd` / `pd data` guard and the page's check now treat zero-width blanks as blanks too. The AC named only the console refusal; a hidden blank could carry GoodCRC past the guard.
- A dropped live batch leaves an unmarked straight line in the live chart. Nothing fetches it on its own. History-drawn views show it. AXHK2W measures whether this happens in the real app and fixes it if so.
- The session's last samples are always delivered, except after a worker panic.
- AC-AXHK2M.2 is simulator-only: a release build has no way to reload the page, and none is added for this. `firmware_flash_control` and the page's own reload tests cover it.
- AC-AXHK2M.3 relies on a target keeping its serial across the bootloader jump. A listing with no serial, or any real listing not in application mode, stays refused.
- The tag rollback in the release script has only run against the fake client. It now reads GitHub's answer, on the rollback and on the publish.
- Any raw console line with a C0 or C1 control character inside is refused, in the library and the page. GoodCRC is judged first, so `pd\tcmd=1` keeps its own reason. Whether the firmware splits on such characters is unknown, so this fails closed.
- A lease dropped without `finish` keeps a generic failure, never a success. A kept outcome nobody read still shows at the next page load.
- A trigger gets the same target check as a connect. A running meter that cannot be told from the target needs consent first, so the check is a second line; only a unit test reaches it.
- A dropped batch held back goes out before the next non-batch event, so no event overtakes samples it follows.
- A stop during the wait after JUMP_DFU no longer claims the bootloader: the meter may not have switched.
- `firmwareFileErrorText` had no production caller and is gone; the store keeps `fileErrorHint` beside `fileError`.
- Waiters held during an update with a serial-less target are not retried when the lease drops, since no hotplug fires. It predates this FR, and the runbook covers it.
- `FlashEnd` stays a struct with two options, the wire shape.
- The phone-width check found the firmware warning card sitting over the bottom of the settings panel on a desktop window. It predates this FR and is not fixed here. AXHK2V's layout pass picks it up.
