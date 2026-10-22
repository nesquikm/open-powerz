---
title: Export and recording feedback, deep zoom and empty states
milestone: M_R4WG4Q
status: archived
archived_at: 2026-10-22T07:41:09Z
id: fr_01M5F1DHRMHJYHKJTN4V9CFMD3
created_at: 2026-10-21T10:51:53.493Z
changelog_category: Added
---

# Export and recording feedback, deep zoom and empty states

## Summary

A finished export said nothing, and a saved recording did not say where it went.
A very deep zoom showed an empty plot, and a disconnected chart was a blank box.
Exports and recordings now report their rows and file, with a way to show the file.
The narrowest zoom always shows samples, and the empty chart says how to start.

## Requirement

Saving a file always tells the user what was written and where, deep zooms keep showing the trace, and a chart with no meter explains what to do.

## Acceptance Criteria

- AC-9CFMD3.1: After a successful export, a notice shows the rows written and the file path, with "Reveal in Finder" on macOS or "Show in folder" on Windows and Linux, and "Copy path".
- AC-9CFMD3.2: The notice after a recording is saved gives the rows, any gaps and the file path, with the same two actions.
- AC-9CFMD3.3: The reveal action uses the official Tauri opener plugin, pinned like the other dependencies, with a capability that allows revealing a file only.
- AC-9CFMD3.4: The narrowest view is two sample periods at the current rate (never less than the existing 10 ms floor), and lines run to the view edges from the samples just outside it, so no zoom level shows an empty plot while data exists.
- AC-9CFMD3.5: With no meter connected, the chart shows "Connect a meter to start", the Live button is not shown pressed, and the zoom readout is blank.

## Technical Design

The export job and the recording stop already return rows and path; a notice component renders them with the two actions. The opener plugin is registered in the Tauri builder with a reveal-only permission; the UI calls it through the backend seam, and the mock records the request. The minimum span becomes a function of the rate. Frames for a view include one sample beyond each edge from the buffer or the history answer. The chart shows an empty-state overlay while no session runs.

## Testing

Vitest: notice text, minimum span per rate, edge samples in frames. Playwright: export and stop notices with both actions, deep zoom at 50 SPS draws lines, the disconnected empty state. Vitest also reads the capability file and the manifests: only the reveal permission, and a pinned plugin.

## Notes

QA findings 4, 5 and 9 on build 9787adb; deep zoom uses the clamp plus edge samples, as confirmed. The opener's reveal command accepts any path and offers no scope; it cannot read or run a file. After a disconnect the last session stays visible under a banner.
