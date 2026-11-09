---
title: Connect a meter leaves the demo selected
milestone: M_QFTCWW
status: archived
archived_at: 2026-11-08T21:34:13Z
id: fr_01M6YBWDNFBK8QTEB7F8XPT7PZ
created_at: 2026-11-08T19:59:34.000Z
changelog_category: Fixed
kind: bug
---

# Connect a meter leaves the demo selected

## Summary

After pairing a meter from the device menu, the menu still showed the demo meter.
The user expected the new meter to be picked so that Connect goes to it.
The pairing step added the meter to the list but never picked it.
It did nothing at all when the list had already been refreshed.
Now a paired meter is always picked; a cancelled or failed pairing leaves the previous pick.

## Requirement

### Steps to reproduce
1. Open the web edition with only the demo meter listed.
2. Choose "Connect a meter…" in the device menu and pick the KM003C.

### Expected
The device menu shows the KM003C.

### Actual
The device menu shows the demo meter (user report, web HIL run WEB.hil2-20261108-2223).

## Acceptance Criteria

- AC-XPT7PZ.1: Regression: the device picker test (`DevicePicker.web.test.ts`) shows that after the WebUSB picker grants a meter, the device menu selects that meter, whether or not the device list already holds it.
- AC-XPT7PZ.2: The same holds for "Connect over HID…" and the meter its picker grants.
- AC-XPT7PZ.3: A cancelled picker leaves the selection and the error banner as they were.
- AC-XPT7PZ.4: A picker that fails shows the error and leaves the previous selection.
- AC-XPT7PZ.5: Selecting the granted meter ends a wait for an unplugged meter the same way choosing it by hand does.

## Technical Design

### Root cause
The pairing helper appended the granted meter to the list but never set the selection, and returned early when the hotplug listing had already added the meter.

### Fix
After a grant, add the meter when it is missing and select it through the session's select, cancelling a pending wait when select asks for it.

## Testing

Vitest on the pairing helper and the device menu component.

## Notes

Pairing does not connect; the user still clicks Connect.

## Hardware evidence (web HIL rounds 2 and 3)

- [x] AC-XPT7PZ.1, the listed-meter half: round 3 (qa-runs/WEB.hil3-20261109-0101) showed the paired KM003C selected ahead of the demo on every connect. A fresh pairing through the picker was not re-run at e426443.
