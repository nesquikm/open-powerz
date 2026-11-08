---
title: Drop-down arrows sit against the right edge of the box
milestone: M_QFTCWW
status: active
archived_at: null
id: fr_01M6YC8Q2P5QM5P68GASK1GZTR
created_at: 2026-11-08T20:05:59.000Z
changelog_category: Fixed
kind: bug
---

# Drop-down arrows sit against the right edge of the box

## Summary

The user noticed that the arrow on every drop-down looks glued to the right edge of its box.
The browser draws that arrow itself, so the app's padding never reached it.
Every drop-down in the desktop app and the web edition now draws its own arrow with clear space after it.
The arrow follows the theme, stays visible in high-contrast mode, and the text never runs under it.

## Requirement

### Steps to reproduce
1. Open the app (desktop or web) in light or dark mode.
2. Look at any drop-down, for example the device menu or the rate menu.

### Expected
A visible gap between the arrow and the box's right edge.

### Actual
The arrow touches the right edge (user report, 2026-11-08).

## Acceptance Criteria

- AC-K1GZTR.1: Every drop-down in the app, desktop and web, uses one shared style that turns off the browser's own arrow and draws the app's chevron.
- AC-K1GZTR.2: The gap between the chevron's right end and the box's inner right edge is between 10 and 12 px in Chromium and in WebKit.
- AC-K1GZTR.3: The right padding leaves room for the chevron, so a long option label is cut before it reaches the chevron.
- AC-K1GZTR.4: The chevron takes the text color of the current theme in light and dark mode, and stays visible in forced-colors mode.
- AC-K1GZTR.5: Focus rings, hover and disabled states of drop-downs look as before, and the existing accessibility checks still pass.
- AC-K1GZTR.6: A screenshot of one drop-down before and after the change is saved with the HIL evidence.

## Technical Design

### Root cause
The shared select rule in the app stylesheet set padding but left the native arrow, which the engines draw flush with the border.

### Fix
In that rule: appearance none; a chevron drawn as two small background gradients coloured by the text token, placed so its right end sits about 11 px inside the box; right padding sized for chevron plus gap; a forced-colors override that brings back the system arrow.

## Testing

A Playwright spec measures the chevron gap and the padding on Chromium and WebKit; the a11y and focus specs run unchanged.

## Notes

User request relayed by the orchestrator on 2026-11-09.
