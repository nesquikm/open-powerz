---
title: Logo upload, remove and read-back in the session
milestone: M_PVFHM6
status: archived
archived_at: 2026-11-07T07:23:18Z
id: fr_01M6P7JAKE21ME37PTA8TZK5K4
created_at: 2026-11-05T16:11:37.285Z
changelog_category: Added
---

# Logo upload, remove and read-back in the session

## Summary

The library gets the three logo actions the app and command line will call.
Upload writes the logo chunk by chunk and turns the custom logo on only after every chunk was accepted.
Then it reads the logo back and compares.
Remove clears the logo and turns it off.
Read reports what the logo area holds today.
Upload is allowed only on a KM003C, after its settings and model were read in the same session.

## Requirement

Session methods drive logo upload, remove and read over the guarded paths, stop on the first failure with the chunk index, and verify every write by reading it back.

## Acceptance Criteria

- AC-TZK5K4.1: Upload sends the padded blob as 256-byte chunks at 0x300000 plus 256·k, each waiting up to 1000 ms for FINISHED; any other reply, a timeout or a transport error stops the write at once with an error naming the chunk index, with no retry and no flag write.
- AC-TZK5K4.2: Only after every chunk was FINISHED does Upload send the flag value 2, then read back the whole padded blob from 0x98300000 in reads of at most 256 bytes, at least 50 ms apart, and compare; a difference is an error naming the first differing offset.
- AC-TZK5K4.3: Remove sends 16 bytes of 0xFF at 0x300000, then the flag value 0, then reads back 16 bytes and expects all 0xFF; any other outcome is an error that says which step failed.
- AC-TZK5K4.4: Read reads 16 bytes at 0x98300000 and reports empty (all 0xFF), a logo with its width, height and mode, or unrecognised; a full read of a recognised logo decodes it to RGBA.
- AC-TZK5K4.5: Upload and Remove are refused before anything is sent unless the session has authenticated, read SETTINGS successfully and read the device model, and the model is KM003C; the refusal names the missing step or the model.
- AC-TZK5K4.6: Every action reports progress as (step, done, total) for chunks and read-back reads, and an upload cannot be cancelled once its first chunk is sent.
- AC-TZK5K4.7: The result of Upload and Remove includes the startup-screen duration from the SETTINGS read and a flag saying the logo will not show because the startup screen is off.
- AC-TZK5K4.8: The startup-screen duration (key 0x06, values off, 1, 3, 5 s) becomes a writable settings field through the existing settings write and read-back; nothing else about that write changes.
- AC-TZK5K4.9: Simulator end-to-end tests cover a full upload and read-back, Remove after a logo, a REJECT at chunk 0, no reply at chunk k, unplug after chunk k, the auth toggle and a non-KM003C model; in each failure no flag write is sent.

## Technical Design

New `session/logo.rs`: `Session::logo_upload(&LogoBlob, &mut dyn FnMut(Progress))`, `logo_remove`, `logo_read`, `logo_read_full`. Chunk frames come from a pure `logo_chunk_plaintext(id, addr, data)`; flag frames from `logo_flag_request(id, value)`. Session state records `settings_read` and `model`. `WritableField::StartupDuration` (key 0x06) is added; the read-value rule for 0x06 stays for every other write.

## Testing

Mock-transport unit tests for frame order, refusals and stop-on-failure; powerz-sim integration tests for the end-to-end and fault cases.

## Notes

Over which pipe the meter takes 0x4A is unknown; the session's main pipe is used, as the vendor app does.
