---
title: A snapshot timeout ends the CLI watch
milestone: M_NA20AD
status: active
archived_at: null
id: fr_01M72Q3RK9DVJVYN94DCNVN6M7
created_at: 2026-11-10T12:32:23.000Z
changelog_category: Fixed
kind: bug
---

# A snapshot timeout ends the CLI watch

## Summary

The command-line watch quit when the meter left one reading request unanswered.
It happened over the serial port while a browser streamed from the same meter.
A watch should run until it is stopped.
Now a missed reading skips that refresh, says so once, and the watch tries again.
Only a real disconnect still ends it.

## Requirement

### Steps to reproduce
1. Stream from the meter in the web edition over WebUSB.
2. Run the CLI watch over the CDC transport with the CDC source, so it falls back to ADC snapshots.

### Expected
The watch keeps showing readings until stopped.

### Actual
One run in three exited about 13 s into the fallback with "powerz: timed out waiting for the meter" (HIL12, step 2a).

## Acceptance Criteria

- AC-NVN6M7.1: Regression: when one ADC snapshot request in the plain watch times out, the watch draws nothing for that refresh, keeps running and prints fresh readings from the next refresh on.
- AC-NVN6M7.2: The first snapshot timeout of a watch prints one note to standard error; later timeouts in the same watch print nothing more.
- AC-NVN6M7.3: The CLI watch with the CDC source, after its fallback to ADC snapshots, behaves as AC-NVN6M7.1 and AC-NVN6M7.2 describe, until stopped.
- AC-NVN6M7.4: A disconnect or any other failure that is not a timeout still ends the watch with its error and exit code 5.
- AC-NVN6M7.5: The CLI reference's watch section says a missed snapshot is skipped and retried.

## Technical Design

### Root cause
The poll loop in the watch command passes every snapshot error up with the question-mark operator, so a single timeout (the meter, busy with a WebUSB session, dropped or delayed one reply) ends the run.

### Fix
In the poll loop, match the transport timeout: skip the draw, write the note once to an error sink (the poll watch gains one, like the CDC watch), sleep the interval and go on. Every other error still returns. A late reply to the skipped request is dropped by its id (3B70S8).

## Testing

Scripted-port tests in the CLI crate: a port that drops one snapshot reply for the poll watch and for the CDC fallback, a port that disconnects, and the note counted once.

## Notes

Needs the HIL re-check: the CDC source watch with the browser streaming, three runs.

## Hardware evidence (HIL12 and HIL12b)

- [x] AC-NVN6M7.3: qa-runs/WEB.hil12b-20261110-1705 step 1; three `watch --source cdc` runs of 130 s beside the browser stream, none ended early, all exit 0 on SIGINT.
- [ ] AC-NVN6M7.1, AC-NVN6M7.2: no snapshot timed out in those runs, so the skip and its note were not seen on the meter; the CLI tests cover them.
