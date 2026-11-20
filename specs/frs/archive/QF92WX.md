---
title: The firmware probe cannot tell why an old bootloader stays silent
milestone: M_E84WQE
status: archived
archived_at: 2026-11-20T12:37:28Z
id: fr_01M7W21FGYSGKD7WNPE2QF92WX
created_at: 2026-11-20T08:46:36.074Z
changelog_category: Fixed
kind: bug
---

# The firmware probe cannot tell why an old bootloader stays silent

## Summary

On the old bootloader, the probe's CONNECT and HEAD both timed out.
The output showed no endpoints, no string descriptors, and no stalled direction.
Now the probe prints all of that.
A silent meter outside application mode gets a few read-only retry variants.
The probe reports which variant got an answer, if any.
It still sends only CONNECT and HEAD.

## Requirement

### Steps to reproduce
1. Run `powerz firmware probe` on a meter in the 2.1 bootloader.

### Expected
The output shows every endpoint and string descriptor, which direction timed out, and the outcome of each read-only retry variant.

### Actual
`CONNECT: timed out` and `HEAD: timed out`, with nothing else to go on.

## Acceptance Criteria

- AC-QF92WX.1: The probe prints, under each interface, every endpoint with its address, direction, transfer type, max packet size and interval.
- AC-QF92WX.2: The probe prints string descriptors 1 to 4 by index, each as its text, `(none)` or `(unavailable: <reason>)`, with index 2 included.
- AC-QF92WX.3: Regression: when CONNECT or HEAD times out, the output says whether the OUT write or the IN read timed out.
- AC-QF92WX.4: When the meter is not in application mode and the standard CONNECT times out, the probe retries CONNECT and HEAD with read-only variants: no stale-data drain and no clear-halt; the IN read posted before the OUT write; and reads on every IN endpoint, interrupt ones included. It prints each variant's outcome and names the first one that got an answer, or says none did.
- AC-QF92WX.5: Every frame the probe sends, in every variant, is CONNECT (0x02) or HEAD (0x40); a test over the simulator's frame log proves it, and a meter in application mode gets no variant retries.
- AC-QF92WX.6: `--json` carries the same data: endpoints per interface, strings 1 to 4, the timeout direction, and one entry per variant with its outcome.
- AC-QF92WX.7: The CLI reference documents the new probe output and the variants, and the firmware how-to's old-bootloader section asks for the probe output with `--trace` and `--json`.

## Technical Design

### Root cause
`BulkTransport` maps write and read timeouts to one `Error::Timeout`, and the probe reads interfaces from the OS list only.

### Fix
A read-only probe path: open the device (no claim) to read the active configuration's endpoints and strings 1 to 4. Timeouts carry a direction for the probe (a `last_timeout_direction` on the pipe or a probe-only error). Variants run over a `ProbePipe` trait whose only send is a pre-built CONNECT or HEAD frame; the bulk implementation claims the vendor interface and opens every IN endpoint. The simulator implements it, and the old-bootloader persona can be told to answer on one variant.

## Testing

Simulator tests per variant, the frame-type allowlist over the frame log, text and JSON golden output, and the application-mode no-retry case.

## Notes

Needs a hardware runbook: the probe on our 2.5 meter in application and bootloader mode (no variant retries in app mode, endpoint table matches lsusb).

## Implementation notes

- Variants send the standard probe's exact CONNECT and HEAD frames (20-byte HEAD); a write-site check refuses any other type.
- A variant counts as answered on any reply, not only ACCEPT; unexpected CONNECT replies are kept as raw bytes.
- `all-in-endpoints` reads the IN endpoints of the vendor interface only (the 2.1 shape has one interface).
- Real-USB paths (endpoint/string reads, variants) are covered by the hardware runbook, not by tests.
