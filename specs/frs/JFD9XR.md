---
title: Rate keys, paused Power and range announcements
milestone: M_R4WG4Q
status: active
archived_at: null
id: fr_01M5F1DA961WMGRZ6QA9JFD9XR
created_at: 2026-10-21T10:51:45.854Z
changelog_category: Fixed
kind: bug
---

# Rate keys, paused Power and range announcements

## Summary

Three bugs from the QA pass of the reload build.
The number keys changed the sample rate during a recording, although the rate picker is locked then.
The Power trace vanished whenever the chart was paused, because the stored history had no power.
The screen-reader status kept announcing an old range after pans, zooms and window presets.
All three get a fix and a regression test.

## Requirement

While recording, the rate keys follow the rate picker's lock; paused and history views draw Power like live ones; and the chart's status announces the visible range after every view change.

### Steps to reproduce
1. Connect the demo device at 50 SPS, press Space to record, then press 4. The recording continues at 1000 SPS in the mock, and the real backend shows an error banner.
2. Turn Power on, then pause the chart. The Power trace, its axis and its hover value disappear.
3. Pause the chart, then zoom with the wheel or click "1 min". The status still reads the first paused range.

### Expected
Rate keys do nothing while recording, Power stays drawn when paused, and the status describes what is on screen.

### Actual
The rate changes or errors, Power disappears, and the status stays stale.

## Acceptance Criteria

- AC-JFD9XR.1: Regression: `e2e/ui/qa-bugs.spec.ts › rate keys do nothing while recording` reproduces the reported failure; it fails before the fix and passes after it.
- AC-JFD9XR.2: While recording, keys 1 to 4 leave the rate unchanged and show the notice "Stop the recording to change the rate."; the mock backend refuses a rate change during a recording, as the real backend does.
- AC-JFD9XR.3: Regression: `e2e/ui/qa-bugs.spec.ts › Power stays drawn when the chart is paused` reproduces the reported failure; it fails before the fix and passes after it.
- AC-JFD9XR.4: The session history stores Power for every record (VBUS times IBUS), so paused, zoomed and whole-session views draw Power from its true minimum and maximum per bucket, and the hover readout includes it.
- AC-JFD9XR.5: Regression: `e2e/ui/qa-bugs.spec.ts › the range status follows every view change` reproduces the reported failure; it fails before the fix and passes after it.
- AC-JFD9XR.6: After every view change (pan, wheel or pinch zoom, slider, plus and minus, Alt-drag, overview drag, window preset, fit) the status announces the new view, "Live: 1 min" for a live window and "Paused: a to b" for a paused one, at most a few times a second during a drag, ending on the final view.

## Technical Design

### Root cause
App.svelte selectRate never checks for a recording, and the mock's setRate has no recording guard. The history series list in history.rs holds no power, so bucketsToFrame builds no Power column. The chart sets its announcement only when it moves between live and paused.

The rate action is ignored while recording and raises the picker's notice. The history gains a Power series computed per record at insert time; history_range accepts it and the chart asks for it. The announcement is recomputed in one place after every applied view change, throttled.

## Testing

Rust: Power buckets carry the true extremes of the per-record product. Vitest: the rate action is refused while recording; the announcement text for each view kind. Playwright: the three regression tests, in both browsers.

## Notes

Found by the QA click-through of build 9787adb (findings 1 to 3). The real backend already refused the rate change; the keys bypassed the picker lock and the mock lacked the refusal.
