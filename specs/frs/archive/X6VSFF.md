---
title: The save-image test stops copying every saved image to count them
milestone: M_7PKVA6
status: archived
archived_at: 2026-11-05T12:30:00Z
id: fr_01M6NMZB483W1TCNRFDEX6VSFF
created_at: 2026-11-05T12:00:00.000Z
changelog_category: Fixed
kind: bug
---

# The save-image test stops copying every saved image to count them

## Summary

One chart image test failed on a Windows runner while the app saved every image.
To see whether a save had arrived, the test copied every saved image out of the page and counted them.
While the page was busy drawing a tall image, one such copy took nine seconds and outlasted the five second wait.
The test now reads a plain count, and waits for the save itself rather than a fixed time.

## Requirement

### Steps to reproduce
1. Run the chart style spec in Chromium on a loaded Windows runner.

### Expected
Each save is seen once it reaches the backend.

### Actual
PR 12 CI on 89932a7, Windows shard 1: the 320 by 7680 PDF save was not seen within 5 s on either attempt. The trace shows each poll's page call taking 1 to 9 s, and no error.

## Acceptance Criteria

- AC-X6VSFF.1: Regression: the save-image tests wait for a save by polling the number of images the backend holds, not by copying every image out of the page, with up to 30 s for the save to arrive.
- AC-X6VSFF.2: The mock backend reports how many chart images were saved without copying them, and a unit test covers it.
- AC-X6VSFF.3: The chart style spec passes three times in a row in Chromium and WebKit with no retries.

## Technical Design

### Root cause
`saveImage` in the chart style spec polled `chartImages()`, which copies each saved image as a number array. While the page rendered and encoded a 320 by 7680 image, one poll took 9 s, so the 5 s poll gave up on a stale count.

### Fix
A `chartImageCount()` on the mock and a polled count with a 30 s ceiling; the image data is read once, after it arrives. No app code changes.

## Testing

The mock unit test; the chart style spec three times per browser; the full gate.

## Notes

Source: PR 12 CI on 89932a7, Windows shard 1, trace in the uploaded report. The app saved the image in the end; nothing was dropped. Test-only, so no CHANGELOG line. Archived on the shipped M_7PKVA6 by orchestrator decision.
