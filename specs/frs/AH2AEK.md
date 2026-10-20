---
title: Desktop shell, live readout, themes and accessibility
milestone: M_R4WG4Q
status: active
archived_at: null
id: fr_01M5DDP0TWKNS6QSV917AH2AEK
created_at: 2026-10-20T20:07:53.038Z
changelog_category: Added
---

# Desktop shell, live readout, themes and accessibility

## Summary

The window becomes a modern instrument rather than a copy of the vendor app.
Voltage, current, power and temperature are shown large, with data lines and CC pins beside them.
Light and dark themes follow the system, and a toggle overrides that choice.
Everything works from the keyboard, respects reduced motion and meets contrast guidelines.

## Requirement

Build the desktop UI shell in Svelte 5: layout, device picker, live readout, error banner, themes, keyboard shortcuts, accessibility and the About dialog.

## Acceptance Criteria

- AC-AH2AEK.1: The layout has a top bar (device picker, connect/disconnect, rate, record), a main area (live readout and chart) and a side panel (stats); it works from a 900×600 to a 2560×1440 window without horizontal scrolling or overlapping text, and below 1200 px wide the side panel moves under the chart.
- AC-AH2AEK.2: The live readout shows VBUS (V), IBUS (A, signed, with an arrow for current direction), power (W) with 3 decimals and temperature (°C) with 1 decimal, in type at least 40 px high; D+, D−, CC1 and CC2 (V, 3 decimals) sit in a secondary row; values refresh at least 10 times per second with tabular digits and no layout shift, and show "—" while disconnected.
- AC-AH2AEK.3: The device picker lists real meters (model and serial) and "Demo device — <scenario>" entries, follows hotplug, and shows a state badge: disconnected, connecting, streaming, rate locked or error.
- AC-AH2AEK.4: Errors appear in a dismissible banner showing the backend message and hint (for example the Linux udev rule command); errors never open a modal dialog.
- AC-AH2AEK.5: The theme follows the system light/dark preference by default; a toggle cycles System, Light and Dark and persists across restarts; text in both themes has at least 4.5:1 contrast and UI elements and chart lines at least 3:1 (WCAG 2.2 AA), checked by the automated accessibility scan.
- AC-AH2AEK.6: Shortcuts: Space starts/stops recording, C connects/disconnects, R resets stats, 1–4 select 2/10/50/1000 SPS, L returns the chart to live, T cycles the theme, ? opens a shortcut overlay; shortcuts are ignored while a text input has focus; Tab order runs top bar → readout → chart → stats, with a visible focus ring on every control.
- AC-AH2AEK.7: With reduced motion requested by the system, transitions are disabled and the chart jumps to new data instead of scrolling smoothly.
- AC-AH2AEK.8: An About dialog shows the app and protocol-library versions and states that open-powerz is unofficial and not affiliated with ChargerLAB.
- AC-AH2AEK.9: When the meter reports no temperature (raw 0x8000; firmware 2.0.6 sends it while the queue runs at 1000 SPS), the readout shows "—" with the tooltip "Not reported by the meter at 1000 SPS" (or "Not reported by the meter" at other rates), and never a stale or zero value; a unit test covers the formatter and a visual check covers the 1000 SPS view.

## Technical Design

Svelte 5 runes components: `TopBar`, `DevicePicker`, `Readout`, `StatsPanel`, `ErrorBanner`, `ShortcutHelp`, `About`. Logic lives in `src/lib/`: `backend.ts` (a typed wrapper over `invoke` and the event channel, and the single seam the Playwright mock replaces), `session.svelte.ts` (session state built from backend events), `format.ts`, `shortcuts.ts` and `theme.ts`. Theme colours are CSS custom properties per theme with `color-scheme` set; the choice persists in `localStorage`. Numbers use the system font stack with `font-variant-numeric: tabular-nums`. Layout uses CSS grid with one breakpoint at 1200 px.

## Testing

Vitest for formatting, shortcut dispatch (including the text-input guard) and theme resolution. The Playwright suite (FR AH2AEN) covers layout, shortcuts, focus order and the accessibility scan. Visual checks per the milestone plan.

## Notes

The look is our own: a calm, dense instrument panel, not the vendor's design.
