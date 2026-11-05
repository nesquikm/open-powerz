---
title: A firmware update never ends a recording unasked
milestone: M_7PKVA6
status: archived
archived_at: 2026-11-05T07:00:44Z
id: fr_01M69QVAS7BY12FMV0Y1AXHK2F
created_at: 2026-10-31T19:48:09.149Z
changelog_category: Fixed
kind: bug
---

# A firmware update never ends a recording unasked

## Summary

A firmware update can end a running recording without asking.
This happens when the app cannot tell a connected meter from the update target.
The prompt also matches meters by id only, and the backend does not enforce it.
After this fix the backend refuses to release any session the user did not agree to release.
The prompt lists every session the update would end.

## Requirement

### Steps to reproduce
1. Connect a real meter with no readable serial and start a recording on it.
2. In the Firmware view, start an update for a meter listed at another address.
3. Type the version and confirm.

### Expected
The app names the session it may release and waits for an answer. The recording keeps running until the user agrees.

### Actual
`begin_flash` releases the session and the recording ends with no prompt.

## Acceptance Criteria

- AC-AXHK2F.1: Regression: `apps/desktop/src-tauri/src/devices.rs` › `flash_refuses_an_unsure_recording_session_it_was_not_told_to_end` reproduces the failure; it fails before the fix and passes after it.
- AC-AXHK2F.2: The backend works out which sessions an update would release. If any of them is not named in the start request, it refuses with a typed answer listing them. No session is released and nothing is sent to any meter.
- AC-AXHK2F.3: A session that cannot be told from the target (no serial on one side) needs the same consent as the target's own session, recording or not.
- AC-AXHK2F.4: The prompt lists each session by name and says which are recording. Declining leaves every session and recording running, and focus returns to the confirmation field.
- AC-AXHK2F.5: A recording that starts after consent is not covered by it: the backend refuses again and the prompt reopens.
- AC-AXHK2F.6: A disconnect or detach racing the start cannot leave a session half released; the worker gone or the flash lease dropped both end in a known state, with a test for each order.
- AC-AXHK2F.7: `Recording.stop` answers false while a start or stop is in flight, and a failed stop does not mark the recording ended, so the update stays unstarted.

## Technical Design

### Root cause
`begin_flash` in `devices.rs` releases `same` and `unsure` sessions unconditionally; the UI check `isRecording` asks about the target id only.

Split it: `flash_holds(id)` returns the sessions to release; `begin_flash(id, agreed)` refuses with `UiError` kind `flash_needs_consent` carrying them unless `agreed` covers all. The UI catches that kind and opens the prompt. Recording state is read inside the manager lock.

## Testing

Unit tests in `devices.rs` with demo and fake real sessions, Vitest for the prompt state, Playwright for the prompt path on the demo device.

## Notes

Source: AC-4PJRVJ.2 partial. No meter is flashed for this fix; the simulator covers it.
