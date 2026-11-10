---
title: After a meter replug, Connect silently uses the demo device
milestone: M_4RN3SF
status: active
archived_at: null
id: fr_01M73H7QN90JR1NJ6TQHNWGY72
created_at: 2026-11-10T20:09:42.000Z
changelog_category: Fixed
kind: bug
---

# After a meter replug, Connect silently uses the demo device

## Summary

After the picked meter was unplugged and plugged back in, the device menu quietly moved to the demo device.
Connect opened the demo while the header still read KM003C.
A logo upload then reported success without touching the meter.
Now the meter stays picked as not connected, with Refresh, until it returns.
The demo is named as the demo in the menu, the tile header and every write.

## Requirement

### Steps to reproduce
1. Connect a KM003C with serial numbers hidden, then disconnect.
2. Unplug the meter, wait, plug it back in.
3. Press Connect and upload a startup logo.

### Expected
The meter is picked again, or the app says it is missing; the demo is used only when picked by hand.

### Actual
The demo device was picked and connected, titled KM003C, and took the upload.

## Acceptance Criteria

- AC-NWGY72.1: Regression: when the picked real meter leaves the device list while no session runs, it stays listed and picked, labelled as not connected; the pick never moves to a demo device on its own, on the desktop and in the web edition.
- AC-NWGY72.2: When a listing of that meter comes back (same serial, or the same id when it has no serial), it is picked again and the not-connected entry goes; a meter that appears while a demo was picked automatically, not by the user, is picked too.
- AC-NWGY72.3: While a not-connected meter is picked, Connect is disabled, and the picker says the meter is not connected and offers a Refresh button that reads the device list again.
- AC-NWGY72.4: A demo meter's tile header and control-bar label name it as the demo device with its scenario, also with serial numbers hidden; it is never titled by the model alone.
- AC-NWGY72.5: The startup logo confirm, the record delete confirm and the settings Apply row each name the target: the demo device by name with a note that no meter is written, or the meter by its picker name, with "serial hidden" when serials are hidden.

## Technical Design

### Root cause
When the picked entry vanished, the list update picked the first entry, which is the demo when no meter is listed. Hidden-serial tile titles used the model alone, demos included.

### Fix
Session remembers a meter chosen by select or beginConnect. If it vanishes while idle and no other meter is listed, an absent stand-in stays picked until a matching listing returns. An automatic demo pick yields to a meter while idle. Absent entries read " — not connected" and cannot connect; the picker offers Refresh. Demo tiles keep their demo name. writeTargetName labels the three write views' target.

## Testing

Vitest: session replug sequences (desktop and web ids), labels, the target helper, and dialog rendering through svelte/server; the existing device-picker specs stay green.

## Notes

Evidence: the HIL13 app log at 19:54:45Z shows "connected demo:idle" while the menu read KM003C. Related: the earlier replug FRs R13Y5V and AH2AEN.
