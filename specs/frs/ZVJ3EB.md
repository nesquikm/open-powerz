---
title: Protocol reference document
milestone: M_34BAXK
status: active
archived_at: null
id: fr_01M5CYB1320MSY6S4RRPZVJ3EB
created_at: 2026-10-20T15:25:32.899Z
changelog_category: Added
---

# Protocol reference document

## Summary

We write our own reference for the meter's USB protocol, in our own words.
It draws on the research notes and the first session on a real meter.
Every fact carries a tag saying how sure we are: seen on the device, read from vendor files, or guessed.
The library code is written from this document, and it lists the commands we must never send.

## Requirement

Create `docs/protocol.md`: the in-repo protocol spec for the KM003C / KM002C. Rewrite it from `../notes/README.md`, `../notes/protocol/*.md` and `../notes/device/` (session 1, firmware 2.0.6), without copying vendor text. Keep the `[device]` / `[static]` / `[doc]` / `[inferred]` confidence tags. Where the device contradicted earlier notes, the device result wins and the doc says so.

## Acceptance Criteria

- AC-ZVJ3EB.1: `docs/protocol.md` exists and has one `##` section each for: USB identity and interfaces, header bit layouts (main control view, main data view, extended section header), message catalogue, GET_DATA attributes, ADC snapshot, ADC queue streaming, SETTINGS, memory read and device info, authentication, session flow and errors, HID limits, CDC and the ADCNEW stream, PD capture, and never-send commands.
- AC-ZVJ3EB.2: The doc opens with a legend defining `[device]`, `[static]`, `[doc]` and `[inferred]`, and every data row of every table in the doc contains at least one of those tags.
- AC-ZVJ3EB.3: Facts confirmed in device session 1 are tagged `[device]` with the firmware version they were confirmed on (2.0.6), including: ADC payload is 44 bytes; rate codes 0/1/2/3 = 2/10/50/1000 SPS and code 4 behaves like code 0; queue polling works without auth while 0x0E/0x0F/0x10/0x11 are REJECTed below auth level 1; memory read works without auth; reply ids are echoed except the 0x4C reply (id 0); auth level appears in reply header bits 17..18 and is lost on reopen; SETTINGS is 180 bytes with two CRC-32 blocks; PD_STATUS declares size 0 with trailing bytes.
- AC-ZVJ3EB.4: The HID section states, tagged `[device]`, that the ADC snapshot (GET_DATA mask 0x001) is the only request that is safe over HID, that control commands get no reply, and that any request whose reply exceeds 64 bytes hangs and reboots the meter; it states, tagged `[inferred]` (not yet tested with a charger), that PD_PACKET and QC_PACKET replies fit in 64 bytes only while nothing is attached and grow past 64 bytes with a charger.
- AC-ZVJ3EB.5: The never-send section lists types 0x04, 0x08, 0x09, 0x42, 0x48, 0x4A, memory reads outside 0x420 / 0x4420 / 0x03000C00 / 0x40010450, and every HID request other than the ADC snapshot, and states that the library guard refuses them.
- AC-ZVJ3EB.6: The doc contains no AES key bytes, no vendor code, and no text copied verbatim from vendor documents; it names the crate module that holds the keys instead.
- AC-ZVJ3EB.7: `README.md` links to `docs/protocol.md`.

## Technical Design

One Markdown file, written section by section from the notes, cross-checked against the device log. Byte layouts as tables with offset, type, field, scale and tag columns. Worked examples use bytes from our own captures, with serial numbers and UIDs replaced by placeholders. A Rust integration test in the protocol crate reads the doc and checks the required headings, the legend and per-row tags, so the doc cannot silently lose structure.

## Testing

Integration test `crates/powerz-protocol/tests/protocol_doc.rs`: asserts each required `##` heading (AC .1), the legend and that every table data row carries a tag (AC .2), and a `never send` section naming each forbidden type (AC .5). A second assertion scans the doc for the device serial and UID from session 1 and fails if found. ACs .3, .4 and .6 are reviewed by hand at `/implement`.

## Notes

Session 1 was on firmware 2.0.6, not 2.0.7 as the notes assumed. KM002C facts stay `[inferred]` until a KM002C is tested.
