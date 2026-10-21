---
title: Keyboard and overlay behaviour
milestone: M_R4WG4Q
status: active
archived_at: null
id: fr_01M5F1DHS938ZADT2D2JM46CTX
created_at: 2026-10-21T10:51:53.514Z
changelog_category: Fixed
---

# Keyboard and overlay behaviour

## Summary

Four keyboard and overlay rough edges from the QA pass.
Escape did not cancel the clear confirmation.
The narrow-window menu stayed open after a pick or an outside click, and had no arrow keys.
Closing About from that menu lost keyboard focus, and the shortcut list wrapped and left out Escape.

## Requirement

Every overlay and confirmation closes the expected ways, keeps keyboard focus where the user was, and the shortcut list is complete and readable.

## Acceptance Criteria

- AC-M46CTX.1: Escape cancels the clear-history confirmation and returns focus to the "Clear chart history…" button.
- AC-M46CTX.2: The ⋯ menu follows the menu pattern: menu and menu item roles, arrow keys plus Home and End move between items, Enter or Space picks one, and it closes after a pick, on a click outside and on Escape, returning focus to ⋯.
- AC-M46CTX.3: Closing About returns focus to the control that opened it: the About button, or ⋯ when About was opened from the menu.
- AC-M46CTX.4: The shortcut overlay is wide enough that no description wraps between 900 and 1600 px, and it lists Escape.

## Technical Design

The stats panel's confirmation handles Escape like Cancel. The top bar menu gets menu roles, a roving focus index, an outside-click listener while open, and closes on a pick. The About dialog records the element that opened it and focuses it on close, falling back to ⋯ when that element is gone. The overlay gets a wider max width, and the shortcut list gains an Escape row.

## Testing

Vitest: the shortcut list includes Escape; menu keyboard movement. Playwright and axe: each behaviour at 900 and 1600 px, focus checked after every close.

## Notes

QA findings 6, 7, 8 and 16 on build 9787adb.
