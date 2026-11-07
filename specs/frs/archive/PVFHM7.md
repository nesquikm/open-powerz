---
title: Logo image encoder and decoder
milestone: M_PVFHM6
status: archived
archived_at: 2026-11-07T07:23:18Z
id: fr_01M6P7JAK35X1G53WNATPVFHM7
created_at: 2026-11-05T16:11:37.285Z
changelog_category: Added
---

# Logo image encoder and decoder

## Summary

The meter can show a custom picture at startup.
This adds the pure code that turns any picture into the exact bytes the vendor app would store.
It also turns those bytes back into a picture.
It sends nothing to a meter.
Every later piece builds on it, and the preview shows its output pixel for pixel.

## Requirement

The protocol library converts an RGBA image into a vendor-shaped startup logo blob and back, enforcing every encoder-side invariant of the approved design before any frame can be built.

## Acceptance Criteria

- AC-PVFHM7.1: Opaque mode encodes RGB565 little-endian, 2 bytes per pixel, in a 240 px box; alpha mode encodes RGB565 then one alpha byte, 3 bytes per pixel, in a 200 px box.
- AC-PVFHM7.2: The target box is the source size clamped to 48 and the mode's limit on each side; the image is scaled into it with the aspect ratio kept and smooth filtering, and an odd width is rescaled to the next lower even width, also with the aspect ratio kept. A 1000×100 source gives 240×24, a 10×10 source gives 48×48, a 240×135 source is unchanged.
- AC-PVFHM7.3: The blob is the 16-byte header (info word, pixel byte count, zero, info word) followed by the pixel rows, top row first, with no row padding. The info word holds the format code (4 opaque, 5 alpha) in bits 0–4, zero in bits 5–9, width in bits 10–20 and height in bits 21–31. Opaque 240×135 gives info 0x10E3C004 and 64 816 bytes; alpha 200×200 gives 0x19032005 and 120 016 bytes; opaque 48×48 gives 0x0600C004 and 4 624 bytes.
- AC-PVFHM7.4: The written blob is padded with 0xFF to a multiple of 256 bytes while the header's byte count stays exact; the 48×48 opaque logo pads to 4 864 bytes (19 chunks).
- AC-PVFHM7.5: Encoding refuses, with a typed error and no blob, any result that breaks an invariant: format other than 4 or 5, a side of 0 or above the mode's limit, an odd width, a byte count other than width × height × bytes per pixel, bits 5–9 set, or an unpadded blob above 120 016 bytes.
- AC-PVFHM7.6: The decoder turns a blob, or a vendor-style raw file (4-byte info word then pixels), back into RGBA. Encode then decode of an RGB565-exact image returns the same pixels; alpha 0 and 255 survive unchanged. A header that breaks the invariants, or a buffer shorter than its byte count, is reported as unrecognised, never a panic.
- AC-PVFHM7.7: The preview data names the final width and height, mode, blob size, padded size, chunk count, and a warning flag when either side is below 48.
- AC-PVFHM7.8: Loading PNG, JPEG, BMP and the raw file format into RGBA works behind an optional cargo feature of the protocol crate, so the core module still builds for wasm32 without it. Palette and grayscale PNGs are converted to RGBA8888 first.
- AC-PVFHM7.9: docs/protocol.md gains a "Startup logo" section in our own words describing the blob, the info word, the pixel formats and the scaling rule, each fact tagged [static] or [inferred].

## Technical Design

New module `powerz_protocol::logo`: `LogoMode::{Opaque, Alpha}`, `Rgba { w, h, px }`, `encode(&Rgba, mode) -> Result<LogoBlob, LogoError>`, `decode(&[u8]) -> Result<Rgba, LogoError>`, `LogoBlob { info, bytes, padded() }`, `Preview`. Scaling is our own area-average/bilinear resize (no image crate in the core). `validate_header` is shared with the guard. File loading sits behind feature `image-files` using the `image` crate with only png, jpeg and bmp enabled.

## Testing

Byte-vector unit tests per AC in `logo.rs`, including the three worked info words, the clamp cases, odd-width rescale, pad length, refusal table, and round trips. A wasm32 `cargo check` without the feature.

## Notes

Source: the user-approved design proposal §1–§2 and logo.md §3.
