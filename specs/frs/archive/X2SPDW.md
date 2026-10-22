---
title: Real-meter fixes from the M2 QA run
milestone: M_R4WG4Q
status: archived
archived_at: 2026-10-22T07:41:09Z
id: fr_01M5FKRYX4X89X9EJ7WSX2SPDW
created_at: 2026-10-21T16:12:41.000Z
changelog_category: Fixed
kind: bug
---

# Real-meter fixes from the M2 QA run

## Summary

The first QA run with a real meter found seven problems.
At 1000 SPS, samples were lost while a save dialog was open, because macOS slowed the app.
The stop notice counted a gap from before the recording, and the temperature column stayed empty.
Recording and export disagreed on wall-clock times.
After a quit mid-stream the meter needed a replug, and a restarted web view stayed blank.
Error hints used a code font.

## Requirement

A real-meter session at any rate records every sample the meter sends, whatever the window is doing; recordings and exports agree with each other and carry the temperature; the app recovers by itself from a meter left mid-stream and from a restarted web view.

### Steps to reproduce
1. Connect a KM003C, switch to 1000 SPS, press Record and leave the save dialog open for 30 s, then save; later, stop and export.
2. Quit the app while it streams, start it again and connect.
3. Let macOS restart the app's web content process (memory pressure, or end it by hand).

### Expected
No samples lost; the stop notice reports no gap; a sample has the same Unix time in both files; TEMP is filled; the reconnect works; the window redraws.

### Actual
11.9 s lost in 242 holes while the dialog was open; "1 gap" for a gap-free file; Unix times 41 ms apart; TEMP empty while the readout shows 30.6 °C; connect timed out until a replug; the window stayed blank until quit. Error hints rendered in a monospace font.

## Acceptance Criteria

- AC-X2SPDW.1: Regression: with an event sink that blocks for 200 ms on every event and a fake meter that keeps only its latest 63 records at 1000 SPS, a worker run of 10 s loses no record and reports no gap.
- AC-X2SPDW.2: On macOS, while a meter session is open the app holds a latency-critical activity, so the system does not nap or throttle it, and releases it when the session ends.
- AC-X2SPDW.3: On the real meter at 1000 SPS, a recording made while a save dialog stays open for 30 s and the window is then hidden or covered for 30 s has no gaps; the result is recorded in the Notes.
- AC-X2SPDW.4: A recording's summary counts only the stream gaps after its first written row; a gap that ends at the first row is not counted.
- AC-X2SPDW.5: A sample has the same Unix time, to the millisecond, in a recording and in an export of the same session, also after the chart history was cleared.
- AC-X2SPDW.6: At 1000 SPS the temperature readout shows the latest snapshot temperature with the tooltip "Updated once a second at 1000 SPS", and "—" only when the meter reports none; the mock backend does the same. This replaces the "—" rule of AC-AH2AEK.9.
- AC-X2SPDW.7: Recordings, exports and `powerz stream` write, in each row's TEMP column, the latest valid snapshot temperature at or before that row, and leave it empty while there is none; the CLI reads a snapshot between queue polls at least once a second, as the app does, so a recording stays byte-for-byte what `powerz stream` writes.
- AC-X2SPDW.8: An error hint renders in the body font; commands and file paths quoted in a hint (the Linux udev and modprobe hints) render in monospace.
- AC-X2SPDW.9: Regression: when CONNECT over the bulk interface gets no matching ACCEPT within 500 ms, stale frames such as a leftover DISCONNECT are skipped, the halt is cleared on both bulk endpoints, stale data is drained, and CONNECT is retried once; if that fails too, the error asks the user to replug the meter.
- AC-X2SPDW.10: When starting the queue is refused because a stream from an earlier session still runs, the app stops it and starts again once, so the rate stays selectable.
- AC-X2SPDW.11: Quitting the app or closing its window mid-stream stops the queue and releases the interface within 1 s; on the real meter a relaunch connects without a replug, recorded in the Notes.
- AC-X2SPDW.12: The protocol document lists clearing an endpoint halt among the safe operations, with a confidence tag.
- AC-X2SPDW.13: On macOS, when the web content process ends, the app reloads the window, which re-attaches to the running session; on the real app, ending that process mid-stream gives a drawing window again within 5 s, recorded in the Notes.

## Technical Design

### Root cause
A: the meter keeps ~63 records (~60 ms at 1000 SPS, probed on the real meter); macOS throttles the app under a sheet or when hidden, so polls slip past 60 ms. B: the gap before the first row reaches the recorder. C: recording and history each fix their Unix origin at their first poll. D: CSV rows never get the snapshot temperature. F: no recovery after an aborted transfer; a running queue REJECTs 0x0E. G: no web-process termination handler.

Fixes: an NSProcessInfo activity (objc2-foundation, macOS); a sink forwarder thread; one session Unix anchor; a sparse temperature log; a transport recovery hook (nusb clear_halt); an exit hook stopping workers; reload on termination.

## Testing

Rust unit tests with fake transports and clocks for A, B, C, D, F; Vitest for the hint splitter and the temperature tooltip; visual check for E and D; real-meter checks for A, F and G, recorded in the Notes.

## Notes

Source: QA of build 9787adb (its item F is OS behaviour). A mid-session recording's first rows carry the warm-up temperature, which `powerz stream` lacks; parity allows that.

Real meter, 2026-10-21: .3 no holes at 1000 SPS through two 30 s dialogs and 30 s hidden; .11 quit in 0.23 s, relaunch connected unplugged; .13 drawing again within 5 s.
