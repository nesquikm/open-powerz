---
title: Simulated logo flash
milestone: M_PVFHM6
status: active
archived_at: null
id: fr_01M6P7JAKBFGMXAZ8P61A2XTES
created_at: 2026-11-05T16:11:37.285Z
changelog_category: Added
---

# Simulated logo flash

## Summary

The logo commands must be tested end to end before any real meter sees them.
The simulated meter learns to take logo chunks and store them in a model of its flash.
It also keeps the startup flag and serves reads of the logo area.
Tests can make it refuse, go silent or unplug at any chunk, and pick how its flash erases.
A helper says what the screen would show at boot.

## Requirement

The powerz-sim meter models the logo flash, the 0x1C flag, faults and erase policies, and offers a boot-render helper, as listed in the approved design's simulator section.

## Acceptance Criteria

- AC-A2XTES.1: The simulator decrypts each 0x4A frame with key slot 0, checks the block header (CRC, 0xFFFFFFFF, second length) and answers FINISHED with the request's id, or REJECT for a bad header; a rejected chunk changes no flash byte.
- AC-A2XTES.2: Flash is a sparse model whose unwritten bytes read 0xFF, with three erase policies a test selects: erase each 4 KiB sector on its first write, erase the whole logo region on a write at 0x300000, or no erase (new bytes AND into old ones). A test shows what each policy leaves after a large logo is followed by Remove.
- AC-A2XTES.3: An auth toggle makes the simulator REJECT 0x4A until the session has authenticated; off by default it accepts without auth.
- AC-A2XTES.4: Fault injection per test: no reply at chunk k, REJECT at chunk k, and unplug after chunk k (every later call fails as a disconnected device).
- AC-A2XTES.5: SET_DATA 0x1C stores the value for 0 and 2 and answers REJECT for any other value, leaving the flag unchanged; an option mirrors the flag into a chosen settings-block byte so tests can model the open settings question.
- AC-A2XTES.6: 0x44 reads in the 0x98300000 window are served from the flash model with the 20-byte confirmation first, at most 256 bytes, as the existing memory reads are.
- AC-A2XTES.7: The boot-render helper takes flash, flag and startup duration and returns nothing (duration off), the built-in logo (flag 0, or flag 2 with a header that breaks the invariants) or the custom image decoded from flash, following only our inferred rules, each marked [inferred] in its doc comment.

## Technical Design

New `powerz-sim/src/logo.rs`: `LogoFlash { bytes: BTreeMap<u32, [u8; 4096]>, policy: ErasePolicy }`, `LogoFault::{NoReplyAt(k), RejectAt(k), UnplugAfter(k)}`, `flag: u32`, `auth_required: bool`, `boot_render(&LogoFlash, flag, duration) -> BootScreen`. Wired into `SimDevice` dispatch beside `records.rs` and `memory.rs`, using the protocol's `logo::decode`.

## Testing

Unit tests in `logo.rs` per AC, and dispatch tests in the sim crate's integration tests driving raw frames.

## Notes

Design proposal §5. Erase behaviour on the real meter is unknown until stage 2.
