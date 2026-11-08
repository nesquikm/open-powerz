---
title: Connect a meter from the device menu
milestone: M_QFTCWW
status: active
archived_at: null
id: fr_01M6Y28R5BQYGD68XN0GAYR4W6
created_at: 2026-11-08T17:11:43.719Z
changelog_category: Changed
---

# Connect a meter from the device menu

## Summary

In the web edition, pairing a new meter used a separate button beside the device menu.
The user asked to move it into the device menu, as its last item.
The fallback for basic readings moves into the same menu.
On a first visit, when only the demo meter is listed, a small hint next to the menu still points to it.
The desktop app's device menu stays as it is.

## Requirement

The web edition drops the separate connect button. The device menu ends with a "Connect a meter…" item that opens the WebUSB picker, and the HID fallback item (when offered) sits in the same menu. With no paired meter, a visible hint next to the menu names that item.

## Acceptance Criteria

- AC-AYR4W6.1: In the web edition the device menu's last item is "Connect a meter…"; choosing it opens the WebUSB device picker, called directly in the user's click with no await before it.
- AC-AYR4W6.2: The web edition shows no separate connect button beside the device menu.
- AC-AYR4W6.3: When the browser offers WebHID, the HID fallback is an item in the same menu, just before or after the connect item, and it opens the WebHID picker the same way.
- AC-AYR4W6.4: When no meter is paired (the menu lists only the demo meter), a visible hint or link next to the menu tells the user to pick "Connect a meter…"; choosing the link opens the same picker. Once a meter is listed, the hint is gone.
- AC-AYR4W6.5: The desktop app's device menu shows no connect or HID item and its existing tests pass unchanged.
- AC-AYR4W6.6: The unit and end-to-end tests that clicked the old button use the menu item instead and pass.

## Technical Design

The picker component takes optional extra menu actions from the web backend; selecting one runs its handler synchronously in the change or click event, and the hint is a link that runs the same handler. Desktop passes none.

## Testing

Component tests for the menu items, the user-gesture call order and the hint; the Pages e2e spec updated to use the item.

## Notes

User request relayed by the orchestrator on 2026-11-08.
