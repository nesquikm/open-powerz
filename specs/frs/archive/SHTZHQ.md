---
title: The user docs lag behind the app
milestone: M_NA20AD
status: archived
archived_at: 2026-11-10T14:02:43Z
id: fr_01M72EAV9MJRE9WFRPB9SHTZHQ
created_at: 2026-11-10T09:58:56.000Z
changelog_category: Fixed
kind: bug
---

# The user docs lag behind the app

## Summary

An independent review read the README and every page under docs against the app as it is today.
Some pages still called features desktop-only that the web edition now has.
Some numbers, labels and command rules no longer matched the code.
The README showed an old demo screenshot.
Now the docs match the app, and the README shows a real recording.
Only docs change.

## Requirement

### Steps to reproduce
1. Read the README and the pages under docs.
2. Compare each claim with the CLI help, the UI labels and the backend code.

### Expected
Every claim matches what the app and the CLI do today.

### Actual
The review found about fifty stale or wrong claims, listed in the Notes.

## Acceptance Criteria

- AC-SHTZHQ.1: Regression: the README shows the real KM003C recording screenshot, and its alt text names the Apple 140 W charger and the 5, 9, 15 and 20 V steps.
- AC-SHTZHQ.2: Every relative link and anchor in the README and under docs resolves to an existing file and heading.
- AC-SHTZHQ.3: The CLI reference states the transport rules the CLI enforces: HID refuses the serial-port commands, and the CDC source works with any pipe but HID.
- AC-SHTZHQ.4: No page calls the web edition's recording, export, settings, records, logo, triggering, PD capture or PDF chart export desktop-only. Only firmware update, the recorder's folder and showing a saved file stay desktop-only.
- AC-SHTZHQ.5: Every doc-pinning test in Vitest and cargo test passes.

## Technical Design

### Root cause
Features moved to the web edition and the code grew new commands, options and limits faster than the pages that describe them.

### Fix
Edit the README and the pages under docs in place. Swap the README screenshot for a real window capture. No code changes.

## Testing

Vitest runs the README and docs pinning tests; cargo test runs the protocol and CLI doc tests; a link script checks every relative link and anchor.

## Notes

Changed: README, cli, record-a-csv, explore-a-session, read-the-meters-memory, use-several-meters, web-edition, analyze-pd-traffic, update-firmware, pdcap, desktop-backend, both API maps, architecture, protocol, linux and getting-started.
