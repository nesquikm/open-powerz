---
title: Flash a meter on the old bootloader, experimentally
milestone: M_XQPRRR
status: active
archived_at: null
id: fr_01M84XK64X8RMNMF4VVHDJYNRP
created_at: 2026-11-23T09:00:00.000Z
changelog_category: Added
---

# Flash a meter on the old bootloader, experimentally

## Summary

A firmware update can now run on a meter already in bootloader 2.1.
It runs only when the user asks with an explicit flag.
This path is tested only on the simulator, and the output says so first.
The bootloader is never written, so a failed run can be retried.
Without the flag, the refusal stays and now names the flag.

## Requirement

Meters on bootloader 2.1 answer the same download sequence as bootloader 2.5 except for their reply ids (see XW4QQN). Users with such a meter want to update without Windows.

## Acceptance Criteria

- AC-DJYNRP.1: The CLI flash command takes a flag named experimental-old-bootloader; with it and the existing risk acknowledgement, a meter already in the old bootloader is flashed end to end in the simulator mode that numbers its own replies: CONNECT, HEAD, descriptor, erase, every payload write with SYNC, FINISHED, JUMP_APROM, and the version check after return.
- AC-DJYNRP.2: Without the flag, a flash on the old bootloader is refused before any frame is sent, as today, and the refusal message now names the flag and says it is experimental; jump keeps refusing the old bootloader, and the switch wait after JUMP_DFU still stops at once when the old bootloader appears.
- AC-DJYNRP.3: With the flag, before any frame is sent, the CLI prints that this path has been tested only in simulation, that the bootloader below 0x4000 is never written, and that a failed run can be retried by holding Back while plugging in.
- AC-DJYNRP.4: On the old bootloader the download takes a HEAD reply by type whatever its id (field must still be 0), takes an erase ACCEPT whatever its id, waits up to 2 s for the first CONNECT and retries it once without a pipe recovery, and reads past stray ACCEPT replies before the descriptor so a late CONNECT answer cannot be taken for the erase. On bootloader 2.5 the HEAD id check stays fail-closed and the sequence is byte-identical.
- AC-DJYNRP.5: The flag changes nothing on application or bootloader 2.5 meters: the frames sent are identical with and without it.
- AC-DJYNRP.6: Every other refusal and guard is unchanged: the strict flash guard (descriptor start 0x4000, size budget), model check, container checks and the risk acknowledgement all still apply on the old bootloader.
- AC-DJYNRP.7: The desktop update keeps refusing a meter on the old bootloader, showing the updated message; the firmware how-to and the CLI reference describe the flag, its simulator-only testing and how to retry.

## Technical Design

`FlashAck::with_experimental_old_bootloader()` sets a flag inside the ack, so `flash` keeps its signature. `flash` takes `LegacyBootloader` as a bootloader only with that flag. `download` gets a reply-id policy: by type on BL 2.1, echoed on BL 2.5. On BL 2.1 it also waits 2 s for the first CONNECT with no recover, and drains stray ACCEPTs (short reads) before the descriptor; any other reply there, or a drain that never expires, fails closed as a write error. The switch wait and `jump` are untouched. The sim own-ids mode runs the bootloader sequence with its own counter. `LEGACY_BOOTLOADER_HELP` and its TS copy name the flag.

## Testing

Engine tests on the sim opener (full flash with own ids; frame log identical with and without the flag on 2.5; HEAD id still fail-closed on 2.5); CLI integration with the sim mode; the Vitest that pins the TS message copy.

## Notes

Depends on XW4QQN. No real-meter flash in this milestone; the field user tries it.
