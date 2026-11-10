---
title: A repeated settings change in the browser fails although it landed
milestone: M_ZFZQE7
status: archived
archived_at: 2026-11-10T07:04:51Z
id: fr_01M720KDQV9E1HW2JQV5X3H3JY
created_at: 2026-11-10T05:58:53.000Z
changelog_category: Fixed
kind: bug
---

# A repeated settings change in the browser fails although it landed

## Summary

In the web edition a settings change made right after another one sometimes failed.
It said the serial port was in use by another program, or that the SETTINGS block failed its check.
The meter had taken the change both times, and nothing else held the port.
The browser's serial reader took the meter's stream frames on that port for replies.
Now it reads the port as the desktop app does: it skips stream frames, text and noise, and only whole replies count.

## Requirement

### Steps to reproduce
1. In the web edition, connect a KM003C over WebUSB and let it stream.
2. Meter settings → Edit → brightness 50 → 60 → Apply. It works.
3. Edit → brightness 60 → 50 → Apply, a few seconds later.

### Expected
Each change ends with its read-back: success when the meter shows the change, failure when it does not.

### Actual
3 of 7 Applies, each right after a successful one, failed with "The meter's serial port is in use by another program." or "SETTINGS block B: CRC mismatch", although a Refresh showed the change.

## Acceptance Criteria

- AC-X3H3JY.1: Regression: two settings changes back to back on the same granted port both succeed when the meter's serial port also carries stream frames (ADCNEW, type 0x02) between and inside the replies, and partial frames left from the earlier session.
- AC-X3H3JY.2: Stream frames, PDM text lines and noise bytes on the settings port never count as another program's reply; a whole data reply for another attribute still stops the change with the in-use message before the write (AC-CJ3QC9.1, .2 unchanged).
- AC-X3H3JY.3: A SETTINGS reply that fails its block check is read again (a new request, at most 3 reads in all for that step) instead of failing the change; a read-back that shows the change reports success, one that does not still fails naming the field (AC-CJ3QCA.3 unchanged).
- AC-X3H3JY.4: Opening the settings port that this page itself still holds open (a close that did not finish) closes it and opens it again instead of reporting another program; a port another program holds still gives the in-use message at once.

## Technical Design

### Root cause
`[inferred]` from the HIL logs. The browser's settings pipe framed the serial bytes with the bare CDC frame length, which treats any frame that is not a data reply as a 4-byte header. While the browser streams over WebUSB the meter also puts type-0x02 stream frames on its serial port (the CLI's poll on CDC saw the same frames as `Unknown(2)` replies in the same HIL). The pipe then cut each stream frame's records into 4-byte "frames": one that began with 0x41 read as a data reply for another attribute, so the change stopped with the in-use message after its earlier reads, and a stream frame inside the SETTINGS reply broke block B's CRC. The desktop app reads the same port through the CDC demux (`CdcDemux`), which tells stream frames, text lines and replies apart, so it never saw either.

### Fix
- `powerz-wasm`: a `CdcReplyReader` binding over `powerz_protocol::adcnew::CdcDemux`: `push(bytes)`, `nextReply()` gives the next whole binary reply (stream frames, text lines and noise are dropped).
- `CdcPipe` frames the port's bytes with that reader instead of `cdcFrameLength`; held bytes at the start of a request go through the same reader. The foreign-reply check runs on whole replies only.
- The settings sequence reads a SETTINGS block again (new id) when its block check fails, up to 3 reads for that step; the write itself is never sent twice.
- `openSerialLink`: a port that still shows its own `readable`/`writable` (this page's close did not finish) is closed before `open()`; a port another program holds shows neither, so its failed `open()` still gives the in-use message at once.

## Testing

Vitest: a fake serial port that behaves like the meter (stream frames between and inside replies, a partial frame left over from the earlier session) running two Applies back to back through `MeterDriver.writeSettings` and `CdcPipe` (fails before the fix); the CJ3QC9 and CJ3QCA suites stay green. Rust: the binding's unit tests in `powerz-wasm`.

## Notes

Found by the web parity HIL 3 (2026-11-10, step P5, BUG-8). The re-check on the meter is a separate HIL step.
CLI note from the same HIL (no change here): `powerz --transport cdc watch` with the default poll source exits with `unexpected reply to GET_DATA: Unknown(2)` while the browser streams over WebUSB, because its GET_DATA on CDC meets those stream frames; the passive `--source cdc` holds the port cleanly, and the HIL runbook's P6 now uses it.

## Hardware evidence (parity HIL 4)

- [x] AC-X3H3JY.1: the parity HIL 4 (5198343, 2026-11-10, qa-runs/WEB.parity-hil4-20261110-1037, P5) 9 of 9 back-to-back Applies on the granted port succeeded while the stream ran at 50 SPS (BUG-8 gone).
- [x] AC-X3H3JY.2, the stream half: the parity HIL 4 (5198343, 2026-11-10, qa-runs/WEB.parity-hil4-20261110-1037, P5) no false in-use message from the stream frames on the port.
- [x] AC-X3H3JY.4, the other-program half: the parity HIL 4 (5198343, 2026-11-10, qa-runs/WEB.parity-hil4-20261110-1037, P6) a port held by `powerz … --source cdc` still gave the in-use message at once (27 and 53 ms).
