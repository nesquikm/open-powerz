---
title: The old bootloader answers, but with its own reply ids, so we call it silent
milestone: M_XQPRRR
status: active
archived_at: null
id: fr_01M84XK64WP9GDPJCEDVXW4QQN
created_at: 2026-11-23T09:00:00.000Z
changelog_category: Fixed
kind: bug
---

# The old bootloader answers, but with its own reply ids, so we call it silent

## Summary

A field trace shows that bootloader 2.1 does answer CONNECT and HEAD.
Its replies carry its own running number, not the number we sent.
We dropped them as stale, so the probe said the meter timed out.
Its first reply is also slow, and our pipe reset fails on it.
On the old bootloader only, replies now match by type, with a longer wait.
The probe says when the meter's numbers differ from ours.

## Requirement

### Steps to reproduce
1. KM003C with bootloader 2.1, held Back while plugging in; run `powerz --trace firmware probe` on v0.13.2.

### Expected
CONNECT accepted and HEAD field 0, with a note that the meter numbers its replies itself.

### Actual
`CONNECT: timed out (IN read)`, `HEAD: timed out (IN read)`; the three variants then report a bare "accepted" although every reply id differs from the request id (reply ids 0, 0, 1, 2, 3, 4, 5 for request ids 1 to 8).

## Acceptance Criteria

- AC-XW4QQN.1: Regression: on the simulated old bootloader that numbers its own replies and answers the first CONNECT late (the field trace), the probe reports CONNECT accepted and HEAD field 0 from the standard path alone, and runs no variant.
- AC-XW4QQN.2: On a meter in the old-bootloader mode, a reply answers CONNECT when its type is ACCEPT or REJECT and answers HEAD when its type is HEAD or REJECT, whatever its id; a reply of another type is skipped and read past, up to the existing stale-reply limit. In application and bootloader 2.5 mode the exact id match is unchanged.
- AC-XW4QQN.3: On the old bootloader the first CONNECT waits the full CONNECT timeout (2 s), sends no pipe recovery, and is retried once when unanswered; application and bootloader 2.5 mode keep the 500 ms wait, the recovery and the retry as today.
- AC-XW4QQN.4: When an accepted reply's id differs from the request id, the probe text says, for that CONNECT or HEAD (standard path or variant), "answered with mismatched ids (meter id 0x.., ours 0x..)", and the JSON gives the two ids for it; when the ids match, nothing is added and the output is unchanged.
- AC-XW4QQN.5: A simulator mode, selected with the test-only environment value legacy-bootloader-own-ids, reproduces the field trace frame for frame when driven the way v0.13.2 drove it (strict ids, 500 ms first wait): the same requests, the same replies, and the same timeouts.
- AC-XW4QQN.6: The probe prints string descriptors cut at the first NUL, so the field meter's string 2 reads "BOOT", not "BOOT\0".
- AC-XW4QQN.7: The desktop's Linux busy-device hint names the full Linux guide URL (https://github.com/nesquikm/open-powerz/blob/main/docs/linux.md), not a bare repository path.
- AC-XW4QQN.8: The protocol doc says, tagged by confidence, that bootloader 2.1 answers with its own reply counter, answers the first CONNECT late, and fails the pipe recovery, and that the vendor app matches no reply ids.

## Technical Design

### Root cause
`reread_until_id` drops every reply whose id is not the request id, and BL 2.1 never echoes it. `connect_checked` waits 500 ms, then `recover()` fails (no second CONNECT is in the trace), so the late `05 00` lands on the HEAD read and is dropped too.

### Fix
A session reply-id policy: echoed (default) or by type. `probe_with_variants(LegacyBootloader)` switches to by type: CONNECT takes ACCEPT/REJECT, HEAD takes HEAD/REJECT, the first CONNECT waits `CONNECT_TIMEOUT`, there is no recover, and there is one retry. The session records an id mismatch, and `ProbeReport` and `VariantOutcome` carry it. The sim adds an own-ids flag, a late first reply, and a failing recover.

## Testing

Protocol tests over a scripted transport replaying the field bytes; sim tests for both drive styles (strict replays the trace, legacy gets a clean probe); CLI text and JSON golden checks; a ui_error unit test.

## Notes

Builds on JT38C8 and QF92WX. The id rule of the old bootloader is fitted to one trace.
