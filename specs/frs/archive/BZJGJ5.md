---
title: The built-in startup logos are cut by their frame
milestone: M_4RN3SF
status: archived
archived_at: 2026-11-10T20:51:44Z
id: fr_01M73H7QNM9QEK33VK1MBZJGJ5
created_at: 2026-11-10T20:09:42.000Z
changelog_category: Fixed
kind: bug
---

# The built-in startup logos are cut by their frame

## Summary

The meter shows a 200 by 200 logo in the middle of its larger screen, on its own background.
The plain mascot's sea ran into the image's left, right and bottom edges.
On the meter it looked like a clipped box.
The lettered logo's wave ran into the left and right edges the same way.
Now both seas end inside the picture with rounded, outlined ends.
Each logo floats on the meter's background without a border.

## Requirement

### Steps to reproduce
1. Upload the plain Mascot built-in logo and restart the meter.

### Expected
The mascot and its wave sit on the meter's background with no straight cut edge.

### Actual
The sea is cut flat at three sides of the 200 by 200 frame and reads as a box.

## Acceptance Criteria

- AC-BZJGJ5.1: Regression: in both built-in logos every pixel within 3 pixels of the image's border is fully transparent, so the meter's own background shows there and nothing is cut by the frame.
- AC-BZJGJ5.2: Both built-in logos keep the brand: the electron in shades surfing a sine wave, still 200 by 200 in the transparent mode with alpha 0 or 255 only, and the lettered one keeps the open-powerz lettering.
- AC-BZJGJ5.3: Every opaque pixel that touches a transparent one is dark outline ink, so no light or coloured fringe shows against the meter's background.
- AC-BZJGJ5.4: The brand README states the rule, and the re-render writes on-meter previews of both logos.

## Technical Design

### Root cause
The sea path extended past the canvas on both sides and the bottom, so the outline was clipped and the fill met the frame.

### Fix
The sea becomes a closed shape inside the canvas with rounded ends, outlined in ink, in the hand-kept plain logo source and in the generator's scene for the lettered one. The render script re-renders both PNGs and their on-meter mocks. Transparency already gives the meter's exact background.

## Testing

A cargo test in the protocol crate decodes both embedded PNGs and checks the border margin, the alpha values and the ink-only edge.

## Notes

Evidence: HIL13 photo 6707. The logo area holds 120,064 bytes, so the size stays 200 by 200.

## Implementation notes

- The AC-BZJGJ5.4 preview half was already met by the unchanged render script; the test pins it.
- The P747X7 edge-to-edge tests were rewritten as even-margin and centred-scene checks; its archived FR is left as history.
