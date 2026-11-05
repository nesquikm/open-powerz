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
- AC-AXHK2M.13: A stop the user asked for shows as stopped, not as a failure: no error colour, no "Retry the update" step, and plain words on the state the meter may be in.
- AC-AXHK2M.14: After a stop, the next step says to replug the meter first, and to hold its button while replugging only if that does not bring it back. A meter the app sees in bootloader mode is shown as such in the device list.
- AC-AXHK2M.15: After an update ends, stopped or done, the app connects again to the same meter by its serial once it is back in application mode.
- AC-AXHK2M.16: After an update, the Firmware tab shows the meter's firmware and bootloader versions read anew, never a dash until a reconnect.
- AC-AXHK2M.17: When a listed meter's vendor interface string turns from application to bootloader or back, at the same USB address or a new one, the device list follows on the next hotplug poll, within 1 s. The open list and the closed device select both show "(bootloader mode)" while the meter is in its bootloader, and drop it once it is back. A backend hotplug test feeds an APP entry and then a BOOT one, and a UI test changes a listed meter's mode under the same entry.
- AC-AXHK2M.18: app.log has one line for each listed meter whose mode changes, naming the model, the USB address and the old and new mode, with no serial.

## Technical Design

### Root cause
Each fault is a separate gap the M7a reviews noted; the common cause is scope.

`wait_for` polls the stop flag between short sleeps. The flash status gains the last prompt and outcome. Hints render through one helper. The publish step gains a draft rollback.

## Testing

Unit tests per item in the owning crate or module, and a Vitest test per UI state.

## Notes

Source: M7a Implementation notes marked for M7b. The plan lists items deferred with reasons.

## Hardware evidence (r4 to r6)

- [x] AC-AXHK2M.15, the done ending: the app connected again to the same meter with no replug once it was back in application mode, in r4 (334d005, 2026-11-04) M4 (`connected` 0.17 s after `bootloader to application`, streaming resumed), r5 F1 and r6 F1. No update was stopped on the meter, so the stopped ending is simulator-only.
- [x] AC-AXHK2M.17: r6 (20e4bb1, 2026-11-05) parts F3 and F5. Over an 8 s update the meter's listing went application → bootloader → application (`mode` lines 06:16:25Z and 06:16:31Z), and the Device list line, built from the closed select's own label for each listing, read plain → "(bootloader mode)" → plain. Caveat: the device select was not read while the meter was in bootloader mode, and the 1 s bound was not timed. The meter stays in its bootloader only about 6 s (r4 M2 was not run); the backend hotplug test and the UI test cover both.
- AC-AXHK2M.14 stays unticked: it cannot be provoked on demand, since we do not press Cancel on the real meter. Its bootloader-label half is the AC.17 evidence above. Its evidence is automated: `apps/desktop/src/lib/firmwareUpdate.stop.svelte.test.ts` › "the next step after a stop (AC-AXHK2M.14)" (a plain replug first; hold the button only if the replug does not bring the meter back; a meter in bootloader mode listed as such).

## Implementation notes

AC.17 and .18 come from the f4abf09 hardware run, step B4. The Firmware tab's Mode row said Bootloader, read from the same listing, but the device select still showed the plain name. The listing likely changed under the same entry and the closed select kept its old text. The run could not tell when the listing flipped, so .18 logs each change.

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
- AC-AXHK2M.3 is simulator-only: the a5442bc run had no second meter. The serial stayed the same across the jump and across the full update, as .3 assumes.
- ACs .13 to .16 come from the a5442bc run. The user read the red stop as a failed update. The meter stayed in its bootloader over 2 minutes after the stop, came back with a plain replug, and was not listed meanwhile. Both runs left the selector on the demo device, and the versions read a dash until a reconnect.
- After a page reload during an update, the followed meter carries no serial, so .15 and .16 find it again only under the same id. A real meter changes address across the jump, so a reloaded page may need a manual connect. Not covered by a test; the runbook watches it.
- Every stop point lies before the descriptor, so the stop message's "before anything was erased" holds. A stop before the first stage still shows the replug steps, which is harmless.
- AC.16 reads the versions anew at the .15 reconnect; until then the page shows the read-back version, or the kept ones after a stop. The V9SK55 test that expected dashes after a done update now follows .16.
- Fixing the firmware specs found the recording timer's text split in two by the AXHK2R commit, so `REC 0:00:04` read as two lines to tests. TopBar renders it as one text again.
- AC.18 remembers each meter's last mode by serial, or by its USB id when it has none. A serial-less meter that changes mode at a new address gets no line; whether the bootloader reports a serial is not confirmed on the device. The memory is not cleared on unplug, so a meter that left in application mode and comes back in bootloader mode gets a line, which is what the hardware run needs.
- The device select keys each option by id and mode, so a mode change puts a new option in place. WebKit does not repaint a closed select whose picked option's text changes in place [inferred from the f4abf09 run].
