---
title: Frame codec
milestone: M_34BAXK
status: active
archived_at: null
id: fr_01M5CYB13697EM1B99TXBXYTJX
created_at: 2026-10-20T15:25:32.899Z
changelog_category: Added
---

# Frame codec

## Summary

The meter speaks in small binary frames with packed headers.
This feature turns those bytes into typed values and back, with no USB involved.
Replies can hold several sections chained together, and the parser walks them safely.
Bad or truncated input must produce a clear error, never a crash.

## Requirement

In `powerz-protocol`, add pure encode/decode for the main header (control and data views), the extended section header, request builders, reply classification and the PUT_DATA section walker, as specified in `docs/protocol.md`. No I/O.

## Acceptance Criteria

- AC-BXYTJX.1: `MainHeader` decodes and encodes the control view (type bits 0..6, extend bit 7, id bits 8..15, enc bit 16, att bits 17..31) and the data view (type, extend, id, encode bit 16, permi bits 17..18, obj bits 22..31); decode then encode round-trips every u32; `0C 00 02 00` decodes as type 0x0C id 0 att 1, `41 01 80 02` as type 0x41 id 1 permi 0 obj 10, and `41 61 82 02` as permi 1.
- AC-BXYTJX.2: `SectionHeader` decodes att (bits 0..14), next (bit 15), chunk (bits 16..21) and size (bits 22..31): `01 00 00 0b` is att 1, next 0, size 44 and `01 80 00 0b` has next 1.
- AC-BXYTJX.3: Request builders produce exact bytes: `get_data(id, mask)` gives `0C id` followed by `mask << 1` as u16 LE (mask 0x001 gives `0C id 02 00`, mask 0x009 gives `0C id 12 00`); `connect(id)` gives `02 id 00 00`; `start_queue(id, code)` gives `0E id (code << 1) 00`; `stop_queue(id)` gives `0F id 00 00`.
- AC-BXYTJX.4: `classify_reply` maps type 0x05 to Accept, 0x06 to Reject, 0x07 to Finished, 0x03 to Disconnect, 0x41 to PutData, 0x44 with the extend bit set (`c4`) to MemReadConfirm and 0x4C to AuthReply, each carrying the decoded header; any other type is `Unknown(type)`.
- AC-BXYTJX.5: `parse_put_data` walks the `next` chain and returns each section's attribute and payload; the length is `chunk × size` for ADC_QUEUE (att 0x002) and `size` otherwise; unknown attributes are skipped by length; PD_STATUS (att 0x020) takes the rest of the frame. It parses the captured 52-byte ADC reply, the 188-byte SETTINGS reply, the ADC+SETTINGS chained reply (ADC first with next 1) and a 1268-byte queue reply (63 records).
- AC-BXYTJX.6: The walker never uses the main-header `obj` field for framing: a reply whose `obj` disagrees with its length (the captured 242-byte reply with obj 56) parses correctly.
- AC-BXYTJX.7: Truncated headers, a section length past the end of the buffer, and `next = 1` with no following header each return a typed `FrameError` variant; a property test feeding 10 000 random byte strings (0–2048 bytes) to `parse_put_data` and `MainHeader::decode` completes without a panic.
- AC-BXYTJX.8: `SequenceCounter::next()` yields 1, 2, …, 255, 0, 1, … (pre-incremented u8, wraps at 256).

## Technical Design

Module `frame` with `MainHeader`, `SectionHeader`, `Reply`, `Section { att, payload: &[u8] }` borrowed from the input, and `FrameError` (thiserror). Bit packing via explicit shifts and masks on u32 LE, no bitfield crate. Attributes as a `bitflags`-style newtype over u16. Fixtures: hex strings in `tests/vectors/` taken from our session-1 logs (no identifying fields in these frames). `proptest` as a dev-dependency for AC .7.

## Testing

Unit tests in `frame.rs` for AC .1–.4 and .8 using the literal bytes above. Integration test `tests/frame_vectors.rs` for AC .5–.6 over the captured replies. `proptest` property test for AC .7.

## Notes

Device session 1: sections arrive in ascending attribute order, and gated or empty attributes are simply absent.
