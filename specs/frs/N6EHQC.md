---
title: "Device session: auth, device info and streaming"
milestone: M_34BAXK
status: active
archived_at: null
id: fr_01M5CYB13K07AGTEP46VN6EHQC
created_at: 2026-10-20T15:25:32.899Z
changelog_category: Added
---

# Device session: auth, device info and streaming

## Summary

A session ties a pipe to the meter's request and reply rules.
It reads single measurements and settings, and reads the device's model, firmware and serial.
It also signs in with the meter's keys to unlock high-rate streaming, at every rate the meter supports.
The keys ship with the project, so these features work out of the box.

## Requirement

In `powerz-protocol`, add a `Session` over any `Transport` that implements CONNECT, one-in-flight request/reply matching, typed errors, AES-128-ECB crypto with the shipped keys, memory read of the four known blocks, `device_info()`, authentication levels 1 and 2, and ADC-queue streaming at 2/10/50/1000 SPS with gap detection.

## Acceptance Criteria

- AC-N6EHQC.1: `Session::open(transport)` works without CONNECT; `connect()` sends CONNECT and returns `Ok` only on ACCEPT.
- AC-N6EHQC.2: Every request uses the next `SequenceCounter` id; a reply is matched when its id equals the request id, except a 0x4C reply, which is matched with id 0. A reply with another id is discarded and the next one read, up to 3 times, then `Error::Protocol`. REJECT returns `Error::Rejected { request_type }`; a DISCONNECT reply to GET_DATA returns an empty section list.
- AC-N6EHQC.3: `read_adc()`, `read_settings()` and `get_data(mask)` return decoded values; the auth level from header bits 17..18 of the latest PUT_DATA reply is available as `auth_level()`.
- AC-N6EHQC.4: Module `crypto` holds the four 16-byte AES keys as constants (indexes 0–3, source documented as `[static]`) and provides AES-128-ECB with 0xFF padding of the last partial block and IEEE CRC-32.
- AC-N6EHQC.5: `memory_read(addr, len)` is allowed only for 0x420 (64), 0x4420 (64), 0x03000C00 (64) and 0x40010450 (12) and returns `Error::Forbidden` without sending for anything else. It sends `44 id 01 01` plus AES(key 0) of `addr, len, 0xFFFFFFFF, crc32(first 12 bytes)` padded with 0xFF to 32 bytes; it checks that the 20-byte `c4` confirmation echoes those 16 plaintext bytes, then reads `roundup16(len)` bytes (possibly over several transfers), decrypts them with key 0 and returns the first `len` bytes.
- AC-N6EHQC.6: `device_info()` works at auth level 0 and returns model, firmware version (block 0x4420), hardware version (block 0x420), serial (OTP block) and UID (hex).
- AC-N6EHQC.7: `authenticate(level)` builds `u64 now_ms ‖ credential ‖ u64 random` (credential = 12-byte UID for level 1, first 16 OTP bytes for level 2), pads it with 0xFF to 32 bytes and sends `4C id 00 02` plus AES(key 3) of it. It decrypts the reply with the key-3 variant whose byte 1 is ASCII `X`, requires the time and random values echoed at offsets 0 and `8 + credential length`, and records the level from the reply header; any other outcome returns `Error::AuthFailed`. Consecutive auth attempts are at least 1 s apart.
- AC-N6EHQC.8: `start_stream(rate)` authenticates at level 1 when the level is 0, sends `0F` first when a stream is already started, then sends `0E` with the rate code and requires ACCEPT; `stop_stream()` sends `0F`. Without auth, `poll_stream()` still returns records at the meter's own rate, taken from the last ADC snapshot or SETTINGS read; while that rate is unknown, CC/D± decode in 1 mV units and no gaps are reported.
- AC-N6EHQC.9: `poll_stream()` sends GET_DATA ADC_QUEUE and returns the decoded records in device-time order, and reports a gap (expected vs. received sample count) whenever two consecutive timestamps differ by more than one sample interval; gaps are never filled. A `Streamer` helper polls at least every 30 ms at 1000 SPS and every 200 ms at lower rates.
- AC-N6EHQC.10: Integration tests over `MockTransport` replay scripted sessions — memory read, level 1 and level 2 auth, and stream start at each rate followed by polls and stop — and check the decoded results. Fixtures that would carry the real serial, UID or OTP are regenerated from synthetic values with the same keys, and a test fails if any committed fixture contains the session-1 serial or UID.

## Technical Design

`Session<T: Transport>` owns the counter, the last observed auth level, the last auth time and the stream state. Crypto via RustCrypto `aes` (ECB done block by block) and `crc32fast`. Keys are a `const [[u8; 16]; 4]` in `crypto/keys.rs` copied from the device agent's key-extraction output and commented with their source, per the user's ship-the-keys decision. Time and randomness are injected through a small `Clock` trait so auth is deterministic in tests.

## Testing

Unit tests for crypto (known-answer vectors), the memory-read frame builder and the auth plaintext builder. Integration tests `tests/session_*.rs` with scripted `MockTransport` sessions covering AC .1–.3 and .5–.10. Real-device runs are a manual step documented in the PR.

## Notes

Session 1: auth level is lost on every reopen, so the CLI re-authenticates per run. Level 2 unlocked nothing beyond level 1.
