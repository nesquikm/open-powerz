---
title: The app chart freezes without a word while the CLI polls over CDC
milestone: M_NA20AD
status: active
archived_at: null
id: fr_01M72Q3RK9DVJVYN94DCNVN6M8
created_at: 2026-11-10T12:32:23.000Z
changelog_category: Fixed
kind: bug
---

# The app chart freezes without a word while the CLI polls over CDC

## Summary

While the command-line tool polls readings over the serial port, the meter stops filling the app's USB stream.
The chart froze and the header still said Streaming, with no word about why.
The meter does this itself, so the app cannot prevent it.
Now the app says the meter stopped sending samples and that another program may be polling it.
The notice goes away once samples come back, in the desktop app and the web edition.

## Requirement

### Steps to reproduce
1. Stream from the meter over USB in the web edition (or the desktop app) with the default queue source.
2. Run the CLI watch over the CDC transport, which polls ADC snapshots.

### Expected
The app says the stream has stopped and why, and clears that once samples return.

### Actual
Every queue poll gets an empty reply for the whole CLI run; the chart freezes for 130 s, the header says Streaming, and nothing tells the user (HIL12, step 1).

## Acceptance Criteria

- AC-NVN6M8.1: Regression: in the web edition, when queue polls return no samples for 2 s of polling, the meter's view shows the notice "The meter stopped sending samples. Another program may be polling it, such as powerz watch over the serial port; the stream comes back when it stops."
- AC-NVN6M8.2: The notice goes away once a queue poll returns samples again, and it shows at most once per stall (FR EHY1JW later requires samples to keep arriving for 1 s first).
- AC-NVN6M8.3: Time with the stream's polling held (a settings write, a memory read) does not count toward the 2 s, and empty polls shorter than 2 s show no notice.
- AC-NVN6M8.4: The desktop app shows the same notice under the same rule for its queue source.
- AC-NVN6M8.5: The use-several-meters and record-a-csv pages and the CLI reference's watch section say that a CLI polling the meter over CDC pauses the app's queue stream.

## Technical Design

### Root cause
The meter answers ADC queue polls with an empty queue while another program polls ADC snapshots over CDC `[device]`. Neither the web driver nor the desktop worker treats a long run of empty replies as anything.

### Fix
A small stall tracker (first empty poll time; the stall starts after 2 s of unheld polling, ends on the first records), in the web driver with a new stall callback and in the worker with a new stall backend event. The session store holds a stalled flag; the meter's view shows the notice while it is set.

## Testing

Vitest: the driver over a fake transport with recorded empty queue replies and a fake clock, the session store, doc tests. Cargo: worker tests over a scripted session that answers empty queues.

## Notes

The snapshot source is unaffected. The chart keeps the stall as an ordinary pause in the samples.
While a CLI holds the meter's serial port, settings Apply is refused in both apps; that is by design (HIL12b note B), so the settings-write hold is checked without a CLI.

## Hardware evidence (HIL12 and HIL12b)

- [x] AC-NVN6M8.1: qa-runs/WEB.hil12b-20261110-1705 step 2; the web notice showed 2.3 s into a CLI poll watch with the exact wording.
- [x] AC-NVN6M8.2: same step; it cleared 0.55 s after the CLI exited. The mid-stall flicker seen there is fixed by FR EHY1JW.
- [x] AC-NVN6M8.3 (settings-write hold, short empty polls): qa-runs/WEB.hil12b-20261110-1705 steps 4 and 5; no notice during settings writes without a CLI, and none at 50 or 1000 SPS without a CLI.
- [ ] AC-NVN6M8.3 (memory-read hold): not exercised on the meter; the driver and worker tests cover it.
- [x] AC-NVN6M8.4: qa-runs/WEB.hil12b-20261110-1705 step 3; the desktop notice showed 2.7 s in and cleared 0.08 s after the CLI exited.
