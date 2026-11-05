---
title: Guarded logo write, flag and read paths
milestone: M_PVFHM6
status: active
archived_at: null
id: fr_01M6P7JAK70W1K1PJTKMR1ST8Z
created_at: 2026-11-05T16:11:37.285Z
changelog_category: Added
---

# Guarded logo write, flag and read paths

## Summary

Writing the logo needs three device commands the app has refused until now.
Each gets its own narrow, checked route, in the same style as the record delete.
The flash write is accepted only inside the logo area and only for chunks of a valid logo.
The startup flag accepts only its two known values.
Reads open a small window over the logo area.
Every other use of these commands stays refused.

## Requirement

The transport layer adds exactly three guarded device paths for the startup logo, each with accept and refuse table tests, while the shared check keeps refusing every other 0x4A and 0x48 frame.

## Acceptance Criteria

- AC-R1ST8Z.1: A new logo-chunk path accepts a plaintext frame only when: type 0x4A with the extend bit clear, bit 16 set, bits 17–21 clear, obj equal to the chunk length plus 16, chunk length 16 or 256, block-header address equal to 0x300000 plus a multiple of 256 with the address at least 0x300000 and address plus length at most 0x31D500, the second length equal to the chunk length, word three equal to 0xFFFFFFFF, and a correct CRC-32 over the first 12 header bytes.
- AC-R1ST8Z.2: A 16-byte chunk is accepted only at 0x300000 and only as 16 bytes of 0xFF (Remove). A 256-byte chunk at 0x300000 is accepted only when its first 16 bytes are a valid logo header (encoder invariants) whose padded size keeps every chunk inside the region. The check runs on the plaintext, before encryption; the frame that leaves the guard is encrypted with key slot 0 and padded to whole AES blocks.
- AC-R1ST8Z.3: A new logo-flag path accepts only SET_DATA 0x48 laid out as main header obj 3, one extended header with attribute 0x1C and size 4, and a u32 value of 0 or 2; for example 48 01 C0 00 1C 00 00 01 02 00 00 00.
- AC-R1ST8Z.4: The shared check still refuses every 0x4A and 0x48 frame, and the record-table and settings paths still refuse the logo frames. A table test shows each path refuses the other paths' frames.
- AC-R1ST8Z.5: Logo memory reads are allowed only in 0x98300000–0x9831FFFF, at most 256 bytes per read, with the whole read inside the window; any other address or length outside the existing allow list is refused before anything is sent.
- AC-R1ST8Z.6: Every wrapper transport forwards the two new paths so nested guards each check the frame, and the real transports send the encrypted frame through their normal write.
- AC-R1ST8Z.7: Accept and refuse table tests cover each boundary: first and last allowed chunk, one chunk past the end, one byte low, wrong length, wrong obj, bad CRC, wrong word three, value 1 and 3 on the flag, a read straddling the window edge, and a 257-byte read.
- AC-R1ST8Z.8: docs/protocol.md lists the three paths in the safe-command section with their limits, each fact tagged.

## Technical Design

`guard.rs` gains `check_logo_chunk(plain)` and `check_logo_flag_write(frame)`, plus `Transport::transact_logo_chunk` and `transact_logo_flag` mirroring `transact_record_table` (default: check, then for chunks encrypt bytes 4.. with `crypto::KEYS[0]`, then `transact`). `GuardedTransport` overrides both to check then forward. `session/memory.rs` adds `LOGO_READ_WINDOW` and a checked `logo_memory_read`.

## Testing

Table tests in `guard.rs` and `memory.rs`; frame-byte tests against the logo.md worked examples (chunk 0, chunk 1, Remove CRCs).

## Notes

Exceptions approved by the user on 2026-11-05. The read window intentionally reaches 0x9831FFFF.
