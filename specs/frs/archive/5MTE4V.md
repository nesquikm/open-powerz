---
title: An app icon that says USB-C power meter
milestone: M_R4WG4Q
status: archived
archived_at: 2026-10-22T07:41:09Z
id: fr_01M5EZA05H5B5YY1EKR05MTE4V
created_at: 2026-10-21T10:15:00.020Z
changelog_category: Changed
---

# An app icon that says USB-C power meter

## Summary

The app icon was a generic yellow bolt on a blue square.
It does not say what the app is for, and it sits too large on the macOS grid.
A new original icon shows a USB-C plug with a live measurement trace.
It follows the current macOS icon grid and reads at every size down to 16 pixels.
One vector source in the repository produces every platform's icon files.

## Requirement

The desktop app ships an original icon that reads as a USB-C power meter on macOS, Windows and Linux, at every size the platforms use, generated from one vector source in the repository.

## Acceptance Criteria

- AC-5MTE4V.1: The icon source is one SVG in the repository; it is original artwork with no ChargerLAB or POWER-Z marks, and it shows a USB-C plug or port together with a voltage or current trace, not a lone lightning bolt.
- AC-5MTE4V.2: The 1024 px master follows the macOS Big Sur icon grid: a rounded-square body about 824 px wide centred with about 100 px of transparent margin, a subtle gradient or depth, and clear contrast on both a light and a dark Dock.
- AC-5MTE4V.3: At 16 and 32 px the icon is still recognisable as a plug with a trace, using a simplified small-size drawing if the master does not hold up.
- AC-5MTE4V.4: Every platform icon file (macOS icns, Windows ico and Square logos, Linux PNGs) is generated from the SVG source with the Tauri icon tool, and a test checks their sizes and the transparent margin of the master.
- AC-5MTE4V.5: Renders at 1024, 128, 32 and 16 px on light and dark backgrounds, and screenshots of the macOS Dock and app switcher from the app bundle, are reviewed before the change is committed.

## Technical Design

The master drawing lives in the icons folder of the desktop crate as the source SVG, with an optional small-size SVG for 16 and 32 px. A Bun script renders the SVGs to PNG and runs the Tauri icon tool, then puts the small-size renders in place of the generated small images and rebuilds the macOS icon set from them. A Vitest test reads the generated PNG headers for the expected sizes and samples the master's corners and edges for transparency.

## Testing

Vitest: the generated icon files exist at the sizes each platform expects, and the master has a transparent margin around an opaque body. Manual: the side-by-side renders and the Dock and app switcher screenshots, reviewed in light and dark.

## Notes

Requested after the hardware review: the old bolt did not say what the app does. The design must stay original; no vendor artwork is copied. Tauri bundle configuration keeps its existing icon paths.

Renders and the Dock icon were reviewed at the commit (plan task); the final QA re-checked the Dock icon from the bundle.
