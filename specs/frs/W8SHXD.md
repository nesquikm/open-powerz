---
title: Rate keys over HID try to change the rate
milestone: M_ZFZQE7
status: active
archived_at: null
id: fr_01M6ZX9Y8BB9F6MCWZ2DW8SHXD
created_at: 2026-11-09T10:22:48.075Z
changelog_category: Fixed
kind: bug
---

# Rate keys over HID try to change the rate

## Summary

Over HID the meter gives basic readings only, so the rate picker is disabled.
The keyboard shortcuts 1 to 4 still asked the meter for a new rate, and the refusal showed as an error.
Now the keys follow the picker: they change nothing.
They show the same short note as the picker's tooltip.

## Requirement

### Steps to reproduce
1. Connect a meter over HID in the web edition.
2. Press 3.

### Expected
No rate change is attempted; an info note says the rate is not available over HID.

### Actual
The rate change is sent to the backend and its refusal shows as an error.

## Acceptance Criteria

- AC-W8SHXD.1: Regression: for a session that gives basic readings only, the keys 1–4 send no rate change and leave the requested rate unchanged.
- AC-W8SHXD.2: The keys then show an info note with the same wording as the disabled picker's label.
- AC-W8SHXD.3: Over WebUSB and in the desktop app the keys change the rate as before, and the recording lock still refuses them with its own note.

## Technical Design

### Root cause
The key path checks only the recording lock; the HID flag is read by the picker alone.

### Fix
The rate-change decision takes the basic-readings flag and refuses with the HID wording, used by both the picker and the keys.

## Testing

Vitest on the rate-change decision for the HID, recording and normal cases.

## Notes

Found during the web parity brief (2026-11-09).
