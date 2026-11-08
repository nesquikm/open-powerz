---
title: Browser transports and session driver
milestone: M_QFTCWW
status: active
archived_at: null
id: fr_01M6VHDG2BG62PZDW7K4QFTCWZ
created_at: 2026-11-07T17:39:28.900Z
changelog_category: Added
---

# Browser transports and session driver

## Summary

The web page reaches a real meter through the browser's USB access.
It picks the meter's vendor interface by its class, like the desktop app.
If that interface is taken, the page can use HID for basic readings only.
Anything more over HID reboots the meter.
A small driver connects, signs in, streams readings and captures PD traffic.
It can only read; it has no way to send a write.

## Requirement

Add TypeScript transports (WebUSB vendor bulk, WebHID fallback, the wasm demo meter) behind one interface, and a session driver that runs connect, auth, device info, queue streaming and passive PD capture using the wasm builders and decoders.

## Acceptance Criteria

- AC-QFTCWZ.1: A `WebTransport` interface offers `transact(frame, timeoutMs)`, `read(timeoutMs)` and `close()`; it has three implementations: WebUSB, WebHID and the demo meter.
- AC-QFTCWZ.2: The WebUSB transport opens a device with vendor ID 0x5FC9, selects the configuration, picks the interface whose class is 0xFF with one bulk IN and one bulk OUT endpoint (never by interface number), claims it and frames requests as the native bulk transport does.
- AC-QFTCWZ.3: The WebHID fallback is ADC-snapshot only (docs/protocol.md "HID limits", [device] firmware 2.0.6: any reply over 64 bytes hangs and reboots the meter, control commands get no reply). Permitted WebHID meters (vendor 0x5FC9) are listed as `webhid:N`; a HID session sends only GET_DATA with attribute mask 0x001 at the snapshot period, never CONNECT, auth, queue start/stop/poll, PD sniffer or memory read, and a guard in the WebHID transport refuses every other frame before it is sent (a test proves both). Each 64-byte input report is one reply; there is no multi-report assembly. When the WebUSB claim fails with `device_busy`, the backend connects an already-permitted HID meter instead; when there is none and the browser has WebHID, the `device_busy` error says so and the device picker shows a "Connect over HID…" button whose click calls `navigator.hid.requestDevice` before any await. A browser with WebHID but no WebUSB uses the HID picker for "Connect a meter…". A HID session shows "HID: basic readings only" and offers no PD capture or queue rates. `device_busy` stays the error when the HID open also fails or the browser has no WebHID.
- AC-QFTCWZ.4: The driver connects, authenticates, reads device info (model, firmware version, serial), starts the queue at the requested rate, polls it on the native poll period and delivers samples in batches, and stops the queue on disconnect; if authentication fails it falls back to ADC snapshots at the meter's rate, as the native worker does.
- AC-QFTCWZ.5: The driver's command set is closed: a test lists every frame it can emit and proves each is CONNECT, auth, GET_DATA, queue start/stop, PD sniffer start/stop/poll or a memory read on the safe list; there is no API that sends caller-supplied bytes.
- AC-QFTCWZ.6: A transfer error or USB disconnect event ends the session with a `disconnected` error, and a timeout with `timeout`; both reach the caller as `UiError` kinds the UI already handles.

## Technical Design

New `apps/desktop/src/lib/web/` holds `transport.ts` (interface), `webusb.ts`, `webhid.ts`, `demoTransport.ts` (wraps the wasm `DemoMeter`, advancing its clock by wall time between calls) and `driver.ts`. Frames come only from the wasm builders; the driver holds no byte literals. Unknown sections are skipped per the decoders. The poll loop uses `setTimeout`, not busy waits.

## Testing

Vitest runs the driver against the demo transport (the built wasm in Node) and against hand-written fake WebUSB/WebHID device objects for interface selection, claim failure fallback, the HID snapshot-only guard, disconnect and timeout paths. The closed-command-set test records every frame the driver sends across all paths.

## Notes

Real-meter behaviour of WebUSB and WebHID is checked later by the HIL tester in the user's Chrome; this FR never touches the meter.

## Hardware evidence (web HIL rounds 2 and 3)

- [x] AC-QFTCWZ.2: the web HIL round 3 (e426443, 2026-11-09, qa-runs/WEB.hil3-20261109-0101, W2) opened the KM003C over WebUSB by class and claimed it 21 times in a row, all streaming.
- [x] AC-QFTCWZ.4: the same run (W2, W3) connected, signed in, read the device info and streamed at 2, 10, 50 and 1000 SPS; each rate change was a STOP then START with no session restart. A 10-minute run at 50 SPS recorded 30,643 rows with no gaps.
- [x] AC-QFTCWZ.6, the disconnect half: round 2 (2060fb0, qa-runs/WEB.hil2-20261108-2223, W6) unplugged the meter mid-stream and the page said "The meter is disconnected." The timeout half was not provoked on hardware.
- AC-QFTCWZ.3 stays unticked: the WebHID fallback on the real meter is W10, pending the user's morning HIL (W2b/W6/W7/W9/W10).
