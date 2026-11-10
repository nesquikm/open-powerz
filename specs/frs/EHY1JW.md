---
title: The stall notice flickers during a long stall
milestone: M_NA20AD
status: active
archived_at: null
id: fr_01M72V1NGETNSB9S9JRQEHY1JW
created_at: 2026-11-10T13:41:26.000Z
changelog_category: Fixed
kind: bug
---

# The stall notice flickers during a long stall

## Summary

While the command-line tool polls the meter over the serial port, the app says the meter stopped sending samples.
Mid-stall the meter now and then hands over one stray sample.
The notice went away on that one sample and came back two seconds later, two or three times per stall.
Now the notice stays until samples keep arriving for a second, in the desktop app and the web edition.
After a real recovery it still goes away within about a second.

## Requirement

### Steps to reproduce
1. Stream from the meter over USB in the web edition or the desktop app with the queue source.
2. Run the CLI watch over the CDC transport for a minute or more.

### Expected
The stall notice shows once and stays until the CLI stops and the stream comes back.

### Actual
The meter answers one queue poll with a single sample now and then; the notice clears on it for 2 to 4 s and returns, 2 to 3 times per 60 to 150 s stall, in both editions (HIL12b note A).

## Acceptance Criteria

- AC-EHY1JW.1: Regression: in the web edition, during a stall, a queue poll with samples followed by empty polls again does not clear the notice.
- AC-EHY1JW.2: In the web edition the notice clears once polls with samples have run for 1 s of polling, with no more than 750 ms of polling between two polls with samples; at 2, 10, 50 and 1000 SPS a recovered stream clears it within 1.2 s of its first samples.
- AC-EHY1JW.3: Stray samples further apart than 750 ms of polling start the 1 s over, so single samples 1.6 s apart never clear the notice.
- AC-EHY1JW.4: The desktop app clears its stall under the same rule, and sends one stall-ended event per stall.
- AC-EHY1JW.5: The stall still starts after 2 s of unheld empty polls, as before.

## Technical Design

### Root cause
The stall tracker in the web driver and the desktop worker ends a stall on the first poll that returns any records, and the meter returns a single record now and then while another program polls it over CDC `[device]`.

### Fix
Both trackers keep a polling-time clock (the same bounded step per poll as the empty run). While stalled, a poll with records starts or continues a recovery run; the run restarts when more than 750 ms of polling passed since the last poll with records, and the stall ends once the run spans 1 s.

## Testing

Vitest: the driver over a fake transport and a fake clock with a stall, stray single samples, and a recovery at each rate. Cargo: StallTracker unit tests plus the worker over a scripted session.

## Notes

The 750 ms gap fits the 2 SPS stream, which returns samples every 500 ms.
