---
title: Live batches at 20 Hz keep the web view's memory down
milestone: M_7PKVA6
status: active
archived_at: null
id: fr_01M6K1N6S1TNQRWKAKH7HR2JAG
created_at: 2026-11-04T10:28:43.937Z
changelog_category: Fixed
kind: bug
---

# Live batches at 20 Hz keep the web view's memory down

## Summary

The macOS app's web view grew past the 400 MB target at 1000 samples per second.
Each live batch reaches the page as a script the web view runs.
That memory piles up faster than the page's own garbage.
The same page on the browser test backend stays flat.
The app now sends 20 batches a second instead of 60, and 4 idle heartbeats instead of 20.
The live chart keeps scrolling smoothly between batches.

## Requirement

### Steps to reproduce
1. Stream the demo device at 1000 SPS in the macOS app with the live chart showing.
2. Read the web content process's footprint after 6 minutes.

### Expected
At or below 400 MB (AC-AXHK2W.1).

### Actual
484 MB on build 334d005 and 449 MB on 3d2ae16, demo device, 2026-11-04.

## Acceptance Criteria

- AC-HR2JAG.1: Regression: the backend flushes live samples at most 20 times a second, at least 50 ms apart, and a stream with no samples sends an empty heartbeat batch every 250 ms. A test streams the demo device at 1000 SPS for 10 s of simulated time and gets at most 200 batch messages carrying every record exactly once in device-time order; an idle stream sends at most 40 in 10 s.
- AC-HR2JAG.2: While the chart is live with smooth scrolling and one batch interval moves the plot by at least 2 px (the short live windows), the window's end moves on with the wall clock between batches. It trails the newest sample by at most one batch interval, never passes it and never moves back. When a batch moves the plot by less than 2 px, the end sits on the newest sample as before. Pure tests cover batches on time, late and early, and the 2 px switch.
- AC-HR2JAG.3: While smoothing per AC.2, live, visible and behind the newest sample, the chart redraws at most 30 times a second; otherwise it redraws only when data arrives. A hidden chart does not redraw, and reduced motion keeps its tenth-of-a-span steps.
- AC-HR2JAG.4: In the macOS app on the demo device at 1000 SPS, the web content footprint after 6 minutes is at or below 400 MB, and a 20-minute run's levels are in the Notes with the before numbers.

## Technical Design

### Root cause
Tauri hands every channel message to the web view as a script it evaluates, even large ones (a small fetch script). Each evaluation costs memory the web view frees only on its own schedule, so 80 messages a second piled up; the page's own data stays flat on the test backend.

### Fix
The batcher's flush interval becomes 50 ms and its heartbeat 250 ms. The chart keeps the newest sample's arrival time and, only where a batch step is 2 px or more, lays the live end at the newest time minus the batch interval plus the time since arrival, clamped and monotonic, redrawing at most every 33 ms until it catches up.

## Testing

Batcher and worker tests counting messages, a Vitest suite for the live-end and redraw functions, and a macOS demo measurement with a short screen recording of the 10 s window for stutter.

## Notes

Final build (adaptive smoothing), demo load-5v-1a at 1000 SPS in the default 10 s window on the MacBook screen. At 6 min: 384 and 377 MB, against 499 and 593 MB on 334d005. A 40-minute run flattened near 180 MB after 10 min. Per-run numbers are in AXHK2W.

## Implementation notes

- Smoothing every frame was tried first and rejected: about 0.7 MB/min of growth with no plateau, 969 MB at 20 min.
- Redrawing at 60 fps with no channel messages stays flat, so the cost needs messages and redraws together. The same page on the browser test backend is flat.
- Smoothing applies only where a batch step is at least 2 CSS px; every redraw is capped at 30 fps while it is on. The 10 s window measured 241 MB at 6 min; the recording showed 33 ms frames with under 1 CSS px of jitter.
- A rate change sends its pending batch at once, so one batch per rate change can follow the last one by under 50 ms.
- A history clear first sends the batcher's waiting samples. The store also records the session time of the last record cleared, and every batch drops samples at or before it, so no pre-clear sample reaches the page on either clear path. A post-clear sample with exactly that session time is also dropped from the page; the history keeps it.
- Measurement runs pin the window and log its visibility every sample. Runs on the external display had read up to 200 MB apart.
