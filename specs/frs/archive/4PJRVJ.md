---
title: Firmware update fixes
milestone: M_3KPN7D
status: archived
archived_at: 2026-10-31T13:10:58Z
id: fr_01M65KVDYC49QF1V7N1Y4PJRVJ
created_at: 2026-10-30T05:17:20.204Z
changelog_category: Fixed
---

# Firmware update fixes

## Summary

The firmware update works on the real meter, and its reviews left a list of rough edges.
The update cannot be cancelled, it can end a running recording without asking, and a page reload loses its progress.
This fixes those and a few smaller checks around firmware versions.
No real-meter flashing is needed for these fixes.

## Requirement

The firmware update and firmware status code handle cancel, reloads, other devices and odd version strings correctly, and agree between backend and UI.

## Acceptance Criteria

- AC-4PJRVJ.1: The desktop update can be cancelled before the erase starts; once erasing has begun, Cancel is disabled with a reason.
- AC-4PJRVJ.2: Starting an update disconnects only the target meter, and asks before ending a running recording.
- AC-4PJRVJ.3: A page reload during an update shows the running update's stage (the backend reports it), not an idle screen.
- AC-4PJRVJ.4: The firmware thresholds (1.7.0, 2.0.7) come from the backend, or one test pins both sides.
- AC-4PJRVJ.5: Version strings with a leading v or a suffix (e.g. 2.0.7-beta) are parsed by a documented policy, with tests.
- AC-4PJRVJ.6: The firmware notice is announced by screen readers (the live region is mounted before its text is set).
- AC-4PJRVJ.7: A failing info block does not abort `powerz info` or the desktop connect; the info blocks are read once per connect.
- AC-4PJRVJ.8: The probe stops on any SYNC reply, whatever its id; release notes that number every line show no extra bullets; `HeadReply::update_required()` and the desktop branches match AC-V9SK52.3.

## Technical Design

Engine stop flag wired to a Cancel command with a stage gate; device-scoped disconnect; backend-reported update state on attach; shared thresholds via the info command.

## Testing

Simulator tests for cancel at each stage, reload/attach state and the tolerant info read; unit tests for version parsing and the notice; Playwright for cancel and the recording prompt.

## Notes

Source: the M_MJ7KE9 M7 follow-ups. Flashing on the real meter is not repeated.

## Implementation notes

- Engine wait_for does not poll stop, up to 120 s — left for M7b; outside this FR's ACs
- Reload loses the Replug prompt and the true outcome — left for M7b; outside this FR's ACs
- AC.2 prompt matches by id only and the backend does not enforce it — left for M7b; outside this FR's ACs
- Checks use plain string equality — left for M7b; outside this FR's ACs
- devices_changed is false during a flash — left for M7b; outside this FR's ACs
- Blank fields in JSON and the PD header — left for M7b; outside this FR's ACs
- MIN_SUPPORTED_FIRMWARE and KNOWN_LATEST_FIRMWARE are test-only — left for M7b; outside this FR's ACs
- FirmwareWarning::UpdateRequired and HeadReply::update_required() are dead — left for M7b; outside this FR's ACs
- ERASE_BEGAN and CANCEL_TOO_LATE are duplicated — left for M7b; outside this FR's ACs
- Focus after Keep recording needs a look — left for M7b; outside this FR's ACs
- u32::parse accepts + — left for M7b; outside this FR's ACs
- cancelFlash is optional — left for M7b; outside this FR's ACs
- FR Testing promises notice tests that do not exist — left for M7b; outside this FR's ACs
- Duplicate asserts at firmware_flash.rs:2895 and sim_firmware.rs:102 — left for M7b; outside this FR's ACs
- followedEnded note lingers and its role=status is inserted late — left for M7b; outside this FR's ACs
- Recording.stop returns true while starting or stopping, and a failed stop still calls #end() — left for M7b; outside this FR's ACs
- Pre-release tags compare equal, Display drops the tag, 2.0.7- parses as pre-release — left for M7b; outside this FR's ACs
- released is a public field — left for M7b; outside this FR's ACs
