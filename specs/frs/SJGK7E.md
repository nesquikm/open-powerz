---
title: Device discovery and transports
milestone: M_34BAXK
status: active
archived_at: null
id: fr_01M5CYB13ATHJ8G3GPNXSJGK7E
created_at: 2026-10-20T15:25:32.899Z
changelog_category: Added
---

# Device discovery and transports

## Summary

The library finds attached meters and opens a pipe to them.
The vendor bulk pipe is the main path, and the virtual serial port is a working second path.
HID only reads the basic snapshot, because larger replies reboot the meter.
All pipes share one interface, so tests never need real USB.
A guard refuses commands that could damage or brick the meter before any byte leaves the host.

## Requirement

In `powerz-protocol`, add device discovery, interface selection by class, a `Transport` trait with a scripted mock, a vendor-bulk transport on `nusb`, a CDC serial transport on `serialport` for the binary protocol, a restricted read-only HID fallback on `hidapi`, a command guard, and a Linux udev rule.

## Acceptance Criteria

- AC-SJGK7E.1: Discovery returns every attached device with VID 0x5FC9, mapping PID 0x0063 to KM003C, 0x0061 to KM002C and any other PID to `Unknown(pid)`, with serial string and bus/address; listing never opens or claims a device. Tested against an injected list of device descriptors.
- AC-SJGK7E.2: Interface selection picks interfaces by class, never by number: vendor bulk (class 0xFF with one bulk IN and one bulk OUT endpoint), HID (class 0x03) and CDC data (class 0x0A). It returns the right interfaces and endpoint addresses for the captured KM003C configuration (IF0 bulk 0x01/0x81, IF3 HID 0x05/0x85) and for a KM002C-ordered fixture (HID at IF1, CDC at IF2/IF3).
- AC-SJGK7E.3: `Transport` exposes one blocking `transact(request, timeout)` that writes a frame and returns the next reply buffer, plus `read(timeout)` for follow-up transfers; `&mut self` keeps one request in flight. `MockTransport` replays a script of expected request bytes and reply buffers and fails with a clear message on any unexpected request.
- AC-SJGK7E.4: The bulk transport claims the vendor interface (detaching a kernel driver such as Linux `powerz` hwmon first), drains and discards stale IN data at open (for example a leftover `03 xx 00 00`), reads each reply as one IN transfer of up to 4096 bytes that ends at a short packet, and sends a zero-length packet after any OUT write whose length is a non-zero multiple of 64. The ZLP and drain rules are unit-tested through the transport's write/open planning functions.
- AC-SJGK7E.5: Default timeouts are 1000 ms per reply and 2000 ms for CONNECT; an expired timeout returns `Error::Timeout`, an unplugged device `Error::Disconnected`, and an access failure `Error::Permission` whose message on Linux names the udev rule from AC .9.
- AC-SJGK7E.6: The HID transport opens the device's usage page 0xFF00 interface, writes report ID 0 plus the frame zero-padded to 64 bytes and returns one 64-byte input report. It only sends GET_DATA requests whose mask is exactly ADC (0x001); any other request (including PD_PACKET and QC_PACKET, whose replies exceed 64 bytes once a charger is attached) returns `Error::UnsupportedOnHid` without writing. If a reply's headers declare more than 64 bytes in total, the transport returns `Error::HidReplyTooLong` and does not read any further report.
- AC-SJGK7E.7: The CDC transport opens the meter's serial port (matched by VID 0x5FC9 and serial number), clears its input, waits 200 ms before the first frame, and reassembles a binary reply that arrives in several reads by walking the section headers until the frame is complete or 1000 ms pass. It refuses frame types 0x01, 0x02 and 0x03 with `Error::UnsupportedOnCdc` without writing, because on CDC those bytes start or stop the ADCNEW push stream rather than SYNC, CONNECT or DISCONNECT.
- AC-SJGK7E.8: A shared guard runs before every write on every transport and returns `Error::Forbidden` without sending for frame types 0x04, 0x08, 0x09, 0x42, 0x48 and 0x4A.
- AC-SJGK7E.9: The repo ships `packaging/linux/60-powerz.rules`, granting `uaccess` for 5fc9:0061 and 5fc9:0063 and setting `ID_MM_DEVICE_IGNORE=1`, and `docs/linux.md` explains installing it and detaching the `powerz` hwmon driver.

## Technical Design

Module `transport` with the trait, `MockTransport`, `BulkTransport` (nusb, blocking wrappers), `CdcTransport` (serialport) and `HidTransport` (hidapi, feature `hid`, default on). Discovery and interface selection are pure functions over descriptor structs, so they test without hardware. The guard lives in one function called by a `GuardedTransport` wrapper that every constructor returns. Bulk is the default; CDC is next, and HID is used only when chosen explicitly.

## Testing

Unit tests for discovery, interface selection (KM003C descriptor fixture from session 1, KM002C fixture built from prior-art layout), ZLP planning, drain-at-open with the mock, CDC reassembly over split reads, the CDC type refusal, the HID ADC-only allowlist, the HID over-length reply refusal and the guard. Real-device bulk/HID checks are a manual step documented in the PR.

## Notes

Multi-report HID replies rebooted the meter twice in session 1; PD and QC status only fit in 64 bytes with nothing attached. Decoding the ADCNEW push stream (CRC-8 poly 0x29) is out of M1 scope.
