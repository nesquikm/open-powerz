---
title: The chart stops drawing after a meter settings write
milestone: M_PVFHM6
status: archived
archived_at: 2026-11-07T07:23:18Z
id: fr_01M6RMDMJEZ927P9DCDGJV54SN
created_at: 2026-11-06T14:32:51.022Z
changelog_category: Fixed
kind: bug
---

# The chart stops drawing after a meter settings write

## Summary

After a meter settings change, the Meter chart stopped drawing lines under a gray band.
The readouts kept updating the whole time.
The write likely resends the meter's own sample rate, so its stream slows down.
The app then reads every sample as a hole.
Now the app restarts the stream at its own rate after a write and logs each write and restart.
A stream that settles on another steady rate is read at that rate.

## Requirement

### Steps to reproduce
1. In the desktop app, connect a KM003C whose own sample rate is 10 SPS and stream at 50 SPS from the queue.
2. In the Meter view's settings editor, change the brightness and press Apply.
3. Watch the chart and its overview strip.

### Expected
The chart keeps drawing after the write. A short pause while the write runs shows as one closed gap.

### Actual
Device run, 2026-11-06, build 46f79cb: a gray band covered the plot from the Apply to the newest sample, with no lines in it, and stayed after a second Apply until the app was quit. The readouts, statistics and time axis kept updating, and the app log recorded nothing.

## Acceptance Criteria

- AC-JV54SN.1: Regression: on the simulator, a settings write while a queue stream runs at a rate other than the meter's stored rate leaves the live chart drawing lines after the write, with at most one closed gap for the write's pause and no gap per sample.
- AC-JV54SN.2: After a successful settings write while a queue stream runs, the stream is stopped and started again at the session's rate; a failed or refused write does not restart it.
- AC-JV54SN.3: The settings write still sends the sample rate key at the value it just read from the meter; the app never changes the meter's stored rate to work around this.
- AC-JV54SN.4: When the queue's records keep arriving at one steady interval other than the one the session expects, the session switches to that interval after five such records in a row instead of reporting a gap for every record; a real hole in the stream is still reported as a gap.
- AC-JV54SN.5: The app log gets one line for each settings write naming the changed fields and their new values, one line when the stream is restarted after it, and one line when the session switches to another interval, with no serial.
- AC-JV54SN.6: The protocol doc and the FR record the cause as inferred, not seen on the device, and say that v0.9.0 has the same write path.

## Technical Design

### Root cause
[inferred] A settings write resends key 0x15 at its read value. That likely puts the meter's own rate back on the running queue. This was not seen on the device, and v0.9.0 has the same write path. The session still expects the started stream's rate, so the gap check flags every record. The chart then breaks the line before each sample and shades back-to-back gap bands.

### Fix
The worker restarts the queue at the session rate after a successful write on a queue source. The protocol session re-derives its interval when consecutive gaps share one steady longer step, and reports that change on the poll. The worker logs writes, restarts and interval changes.

## Testing

Simulator: a settings write applies the written rate to the simulated queue. Protocol tests for the interval switch and real gaps; worker tests for restart, no restart on failure, and the log lines; Playwright chart check before and after an Apply on the mock.

## Hardware evidence

- [x] AC-JV54SN.1, the device half: the M7C verify run (d52c275, 2026-11-06, qa-runs/M7C.verify-20261106-2044, part C) wrote brightness 50 → 80 → 50 from the desktop app at 50 SPS on a meter storing 10 SPS; the chart kept drawing, with one short closed gap and no gray band.
- [x] AC-JV54SN.3, the device half: after both writes `powerz settings` still read 10 SPS stored.
- [x] AC-JV54SN.5, the device half: the app log had one `settings written` and one `queue restarted after settings write` line per write, no serial. No `queue interval switched` line appeared, so the cause stays inferred (AC-JV54SN.6).

## Notes

Found in the M7c carry-over device run, part S. The cause is inferred and needs a meter run with the new log lines to confirm.
