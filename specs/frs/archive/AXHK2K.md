---
title: Notices and opened captures survive other meters' actions
milestone: M_7PKVA6
status: archived
archived_at: 2026-11-05T07:00:44Z
id: fr_01M69QVAS7BY12FMV0Y1AXHK2K
created_at: 2026-10-31T19:48:09.149Z
changelog_category: Fixed
kind: bug
---

# Notices and opened captures survive other meters' actions

## Summary

Several notices vanish when more than one meter is in use.
A tile Stop during a firmware update shows no saved-file notice.
A failed rate change on a tile stays hidden, and notices for parked meters are lost.
Connecting a second meter also releases an opened PD file while the page keeps showing it.
This fix shows each notice and lets the page decide when an opened file is released.

## Requirement

### Steps to reproduce
1. During a firmware update on one meter, press Stop on another meter's tile.
2. Open a PD capture file, then connect a second meter.

### Expected
The stop shows where the file was saved. The opened capture stays usable until the page closes it.

### Actual
No saved-file notice appears, and the backend releases the opened file while the page still shows it.

## Acceptance Criteria

- AC-AXHK2K.1: Regression: `apps/desktop/e2e/ui/` › `a tile Stop during an update shows the saved file` reproduces the failure; it fails before the fix and passes after it.
- AC-AXHK2K.2: Any Stop on any tile, during a firmware update or not, shows the same saved-file notice with Reveal.
- AC-AXHK2K.3: A failed rate change on a tile shows its error on that tile.
- AC-AXHK2K.4: A notice raised on a parked meter is kept and shows when that meter returns to the full view or in its tile.
- AC-AXHK2K.5: An opened PD file is released only by a command the page sends when it closes the file or replaces it. Connecting any meter, automatic reconnects included, no longer releases it.
- AC-AXHK2K.6: The mock backend matches: `#openedPd` follows the same rule, and a test pins it.
- AC-AXHK2K.7: After a page reload, a tile meter's PD capture state is read from the backend, not assumed idle.
- AC-AXHK2K.8: A recording start that times out keeps its file, because the worker may still apply the start and write to it. A definite refusal still removes the empty placeholder.
- AC-AXHK2K.9: A recording that starts after its start timed out shows as recording and can be stopped, and the timeout's error goes away.

## Technical Design

### Root cause
The notice lives on the full-view `Recording` object, so tile stops bypass it. The backend releases the opened file as a side effect of connect.

Add one per-meter notice store, read by tiles and the full view. Add a `pd_release_opened` command called from the page. Remove the release from the connect paths.

## Testing

Vitest for the notice store and mock rule, Rust unit tests for the command, Playwright for the tile flows.

## Notes

Source: `m7-extra-followups.md` and the M7a notes on parked meters.

## Implementation notes

- A page reload leaves an opened file held in the backend until the next open or clear. The page never reads it.
- A start that times out and is later refused leaves an empty file behind.
- A meter's notices stay in the page until dismissed or the app closes, and only a tile with the full view auto-dismisses info notices. A removed tile's last notice moves to the meter shown, which it does not name.
- `pd_capturing` reads as false if the worker does not answer right after the handover.
